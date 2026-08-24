use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use super::client;

const BASE_BACKOFF: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(5);
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SPAWN_TIMEOUT: Duration = Duration::from_secs(3);

struct FailureState {
    count: u32,
    retry_after: Instant,
}

fn backoff_for(count: u32) -> Duration {
    let millis = BASE_BACKOFF
        .as_millis()
        .saturating_mul(1u128 << count.min(6));
    Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX)).min(MAX_BACKOFF)
}

pub(crate) struct HostModuleRegistry {
    host_modules_dir: PathBuf,
    socket_dir: PathBuf,
    connections: RefCell<HashMap<String, UnixStream>>,
    failures: RefCell<HashMap<String, FailureState>>,
}

impl HostModuleRegistry {
    pub(crate) fn new(host_modules_dir: PathBuf, socket_dir: PathBuf) -> Self {
        Self {
            host_modules_dir,
            socket_dir,
            connections: RefCell::new(HashMap::new()),
            failures: RefCell::new(HashMap::new()),
        }
    }

    fn binary_path(&self, name: &str) -> PathBuf {
        self.host_modules_dir.join(name)
    }

    fn socket_path(&self, name: &str) -> PathBuf {
        self.socket_dir.join(format!("{name}.sock"))
    }

    pub(crate) fn is_installed(&self, name: &str) -> bool {
        self.binary_path(name).exists()
    }

    fn spawn_and_connect(&self, name: &str) -> Result<UnixStream, String> {
        let socket_path = self.socket_path(name);
        std::fs::create_dir_all(&self.socket_dir).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&socket_path);

        Command::new(self.binary_path(name))
            .arg(&socket_path)
            .spawn()
            .map_err(|e| format!("failed to spawn host module `{name}`: {e}"))?;

        let deadline = Instant::now() + SPAWN_TIMEOUT;
        while !socket_path.exists() {
            if Instant::now() >= deadline {
                return Err(format!(
                    "host module `{name}` did not create its socket in time"
                ));
            }
            thread::sleep(SPAWN_POLL_INTERVAL);
        }

        client::connect_and_handshake(&socket_path)
    }

    fn record_failure(&self, name: &str) {
        let mut failures = self.failures.borrow_mut();
        let state = failures.entry(name.to_string()).or_insert(FailureState {
            count: 0,
            retry_after: Instant::now(),
        });
        state.count += 1;
        state.retry_after = Instant::now() + backoff_for(state.count);
    }

    pub(crate) fn call(&self, name: &str, method: &str, payload: &str) -> Result<String, String> {
        if !self.is_installed(name) {
            return Err(format!("host module `{name}` is not installed"));
        }

        if let Some(state) = self.failures.borrow().get(name)
            && Instant::now() < state.retry_after
        {
            return Err(format!(
                "host module `{name}` is backing off after repeated failures"
            ));
        }

        if let Some(mut stream) = self.connections.borrow_mut().remove(name)
            && let Ok(value) = client::call(&mut stream, method, payload)
        {
            self.connections
                .borrow_mut()
                .insert(name.to_string(), stream);
            self.failures.borrow_mut().remove(name);
            return Ok(value);
        }

        match self
            .spawn_and_connect(name)
            .and_then(|mut stream| client::call(&mut stream, method, payload).map(|v| (stream, v)))
        {
            Ok((stream, value)) => {
                self.connections
                    .borrow_mut()
                    .insert(name.to_string(), stream);
                self.failures.borrow_mut().remove(name);
                Ok(value)
            }
            Err(err) => {
                self.record_failure(name);
                Err(format!("host module `{name}` call failed: {err}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "smstatus-host-module-registry-test-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn echo_host_module_path() -> PathBuf {
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        if path.ends_with("deps") {
            path.pop();
        }
        path.push("echo_host_module");
        path
    }

    fn install_echo(host_modules_dir: &Path) {
        std::fs::create_dir_all(host_modules_dir).unwrap();
        symlink(echo_host_module_path(), host_modules_dir.join("echo")).unwrap();
    }

    fn install_failing(host_modules_dir: &Path, name: &str) {
        std::fs::create_dir_all(host_modules_dir).unwrap();
        let path = host_modules_dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn call_spawns_and_talks_to_the_echo_fixture() {
        let base = temp_dir();
        let host_modules_dir = base.join("host_modules");
        let socket_dir = base.join("sockets");
        install_echo(&host_modules_dir);

        let registry = HostModuleRegistry::new(host_modules_dir, socket_dir);
        let result = registry.call("echo", "ping", "hello").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn call_on_uninstalled_module_fails_without_spawning() {
        let base = temp_dir();
        let host_modules_dir = base.join("host_modules");
        let socket_dir = base.join("sockets");

        let registry = HostModuleRegistry::new(host_modules_dir, socket_dir.clone());
        assert!(!registry.is_installed("missing"));
        let err = registry.call("missing", "ping", "hello").unwrap_err();
        assert!(err.contains("not installed"));
        assert!(!socket_dir.exists());
    }

    #[test]
    fn call_on_a_module_that_exits_immediately_fails_instead_of_hanging() {
        let base = temp_dir();
        let host_modules_dir = base.join("host_modules");
        let socket_dir = base.join("sockets");
        install_failing(&host_modules_dir, "broken");

        let registry = HostModuleRegistry::new(host_modules_dir, socket_dir);
        let err = registry.call("broken", "ping", "hello").unwrap_err();
        assert!(!err.is_empty());
    }
}
