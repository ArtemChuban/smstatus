use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::client;

const BASE_BACKOFF: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(5);
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(20);
const SPAWN_TIMEOUT: Duration = Duration::from_secs(1);

struct FailureState {
    count: u32,
    retry_after: Instant,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn backoff_for(count: u32) -> Duration {
    let millis = BASE_BACKOFF
        .as_millis()
        .saturating_mul(1u128 << count.min(6));
    Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX)).min(MAX_BACKOFF)
}

fn is_safe_module_name(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }
    let path = Path::new(name);
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(part)) if part == name => components.next().is_none(),
        _ => false,
    }
}

pub(crate) struct HostModuleRegistry {
    host_modules_dir: PathBuf,
    socket_dir: PathBuf,
    connections: Mutex<HashMap<String, UnixStream>>,
    children: Mutex<HashMap<String, Child>>,
    failures: Mutex<HashMap<String, FailureState>>,
}

impl HostModuleRegistry {
    pub(crate) fn new(host_modules_dir: PathBuf, socket_dir: PathBuf) -> Self {
        Self {
            host_modules_dir,
            socket_dir,
            connections: Mutex::new(HashMap::new()),
            children: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    fn binary_path(&self, name: &str) -> Result<PathBuf, String> {
        if !is_safe_module_name(name) {
            return Err(format!("invalid host module name `{name}`"));
        }
        Ok(self.host_modules_dir.join(name))
    }

    fn socket_path(&self, name: &str) -> PathBuf {
        self.socket_dir.join(format!("{name}.sock"))
    }

    pub(crate) fn is_installed(&self, name: &str) -> bool {
        self.binary_path(name)
            .map(|path| path.exists())
            .unwrap_or(false)
    }

    fn kill_child(&self, name: &str) {
        if let Some(mut child) = lock(&self.children).remove(name) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn spawn_and_connect(&self, name: &str) -> Result<UnixStream, String> {
        let binary = self.binary_path(name)?;
        if !binary.exists() {
            return Err(format!("host module `{name}` is not installed"));
        }

        let socket_path = self.socket_path(name);
        std::fs::create_dir_all(&self.socket_dir).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&socket_path);

        self.kill_child(name);

        let child = Command::new(&binary)
            .arg(&socket_path)
            .spawn()
            .map_err(|e| format!("failed to spawn host module `{name}`: {e}"))?;
        lock(&self.children).insert(name.to_string(), child);

        let deadline = Instant::now() + SPAWN_TIMEOUT;
        while !socket_path.exists() {
            if Instant::now() >= deadline {
                self.kill_child(name);
                return Err(format!(
                    "host module `{name}` did not create its socket in time"
                ));
            }
            {
                let mut children = lock(&self.children);
                if let Some(child) = children.get_mut(name)
                    && let Ok(Some(status)) = child.try_wait()
                {
                    children.remove(name);
                    return Err(format!(
                        "host module `{name}` exited before creating its socket: {status}"
                    ));
                }
            }
            thread::sleep(SPAWN_POLL_INTERVAL);
        }

        client::connect_and_handshake(&socket_path)
    }

    fn record_failure(&self, name: &str) {
        let mut failures = lock(&self.failures);
        let state = failures.entry(name.to_string()).or_insert(FailureState {
            count: 0,
            retry_after: Instant::now(),
        });
        state.count += 1;
        state.retry_after = Instant::now() + backoff_for(state.count);
    }

    fn on_success(&self, name: &str, stream: UnixStream, value: String) -> String {
        lock(&self.connections).insert(name.to_string(), stream);
        lock(&self.failures).remove(name);
        value
    }

    pub(crate) fn call(&self, name: &str, method: &str, payload: &str) -> Result<String, String> {
        if !is_safe_module_name(name) {
            return Err(format!("invalid host module name `{name}`"));
        }

        if let Some(state) = lock(&self.failures).get(name)
            && Instant::now() < state.retry_after
        {
            return Err(format!(
                "host module `{name}` is backing off after repeated failures"
            ));
        }

        let reused = lock(&self.connections).remove(name);
        if let Some(mut stream) = reused {
            match client::call(&mut stream, method, payload) {
                Ok(value) => return Ok(self.on_success(name, stream, value)),
                Err(_) => {}
            }
        }

        match self
            .spawn_and_connect(name)
            .and_then(|mut stream| client::call(&mut stream, method, payload).map(|v| (stream, v)))
        {
            Ok((stream, value)) => Ok(self.on_success(name, stream, value)),
            Err(err) => {
                self.record_failure(name);
                Err(format!("host module `{name}` call failed: {err}"))
            }
        }
    }
}

impl Drop for HostModuleRegistry {
    fn drop(&mut self) {
        let mut children = lock(&self.children);
        for (_, mut child) in children.drain() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;

    use super::*;

    fn temp_dir() -> PathBuf {
        crate::host_module::test_temp_dir("registry")
    }

    fn echo_host_module_path() -> PathBuf {
        if let Ok(path) = std::env::var("CARGO_BIN_EXE_echo_host_module") {
            return PathBuf::from(path);
        }

        let mut path = std::env::current_exe().unwrap();
        path.pop();
        if path.ends_with("deps") {
            path.pop();
        }
        path.push("echo_host_module");
        assert!(
            path.exists(),
            "echo_host_module fixture missing at {}; build with `cargo build -p smstatus --bin echo_host_module`",
            path.display()
        );
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
    fn call_reuses_a_live_connection_on_a_second_call() {
        let base = temp_dir();
        let host_modules_dir = base.join("host_modules");
        let socket_dir = base.join("sockets");
        install_echo(&host_modules_dir);

        let registry = HostModuleRegistry::new(host_modules_dir, socket_dir);
        assert_eq!(registry.call("echo", "ping", "one").unwrap(), "one");
        assert_eq!(registry.call("echo", "ping", "two").unwrap(), "two");
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
    fn call_rejects_path_escape_module_names() {
        let base = temp_dir();
        let registry = HostModuleRegistry::new(base.join("host_modules"), base.join("sockets"));
        for name in ["/usr/bin/id", "../etc/passwd", "a/b", ".", ".."] {
            let err = registry.call(name, "ping", "").unwrap_err();
            assert!(
                err.contains("invalid host module name"),
                "name {name:?} err {err}"
            );
            assert!(!registry.is_installed(name));
        }
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
