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

pub(crate) fn is_safe_extension_name(name: &str) -> bool {
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

pub(crate) struct ExtensionRegistry {
    extensions_dir: PathBuf,
    socket_dir: PathBuf,
    connections: Mutex<HashMap<String, UnixStream>>,
    children: Mutex<HashMap<String, Child>>,
    failures: Mutex<HashMap<String, FailureState>>,
}

impl ExtensionRegistry {
    pub(crate) fn new(extensions_dir: PathBuf, socket_dir: PathBuf) -> Self {
        Self {
            extensions_dir,
            socket_dir,
            connections: Mutex::new(HashMap::new()),
            children: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    fn binary_path(&self, name: &str) -> Result<PathBuf, String> {
        if !is_safe_extension_name(name) {
            return Err(format!("invalid extension name `{name}`"));
        }
        Ok(crate::manifest::extension_binary_path(
            &self.extensions_dir,
            name,
        ))
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
            return Err(format!("extension `{name}` is not installed"));
        }

        let manifest = crate::manifest::read_extension_manifest(&self.extensions_dir, name)
            .map_err(|e| e.to_string())?;
        crate::version::check_extensions_api_compatible(
            name,
            (
                manifest.extensions_api.major,
                manifest.extensions_api.minor,
                0,
            ),
        )
        .map_err(|e| e.to_string())?;

        let socket_path = self.socket_path(name);
        std::fs::create_dir_all(&self.socket_dir).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&socket_path);

        self.kill_child(name);

        let child = Command::new(&binary)
            .arg(&socket_path)
            .spawn()
            .map_err(|e| format!("failed to spawn extension `{name}`: {e}"))?;
        lock(&self.children).insert(name.to_string(), child);

        let deadline = Instant::now() + SPAWN_TIMEOUT;
        while !socket_path.exists() {
            if Instant::now() >= deadline {
                self.kill_child(name);
                return Err(format!(
                    "extension `{name}` did not create its socket in time"
                ));
            }
            {
                let mut children = lock(&self.children);
                if let Some(child) = children.get_mut(name)
                    && let Ok(Some(status)) = child.try_wait()
                {
                    children.remove(name);
                    return Err(format!(
                        "extension `{name}` exited before creating its socket: {status}"
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
        if !is_safe_extension_name(name) {
            return Err(format!("invalid extension name `{name}`"));
        }

        if let Some(state) = lock(&self.failures).get(name)
            && Instant::now() < state.retry_after
        {
            return Err(format!(
                "extension `{name}` is backing off after repeated failures"
            ));
        }

        let reused = lock(&self.connections).remove(name);
        if let Some(mut stream) = reused
            && let Ok(value) = client::call(&mut stream, method, payload)
        {
            return Ok(self.on_success(name, stream, value));
        }

        match self
            .spawn_and_connect(name)
            .and_then(|mut stream| client::call(&mut stream, method, payload).map(|v| (stream, v)))
        {
            Ok((stream, value)) => Ok(self.on_success(name, stream, value)),
            Err(err) => {
                self.record_failure(name);
                Err(format!("extension `{name}` call failed: {err}"))
            }
        }
    }
}

impl Drop for ExtensionRegistry {
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
        crate::extension::test_temp_dir("registry")
    }

    fn echo_extension_path() -> PathBuf {
        use std::sync::OnceLock;

        static ECHO: OnceLock<PathBuf> = OnceLock::new();
        ECHO.get_or_init(find_or_build_echo).clone()
    }

    fn find_or_build_echo() -> PathBuf {
        if let Ok(path) = std::env::var("CARGO_BIN_EXE_echo") {
            let path = PathBuf::from(path);
            if path.exists() {
                return path;
            }
        }

        let mut dir = std::env::current_exe().unwrap();
        dir.pop();
        if dir.ends_with("deps") {
            dir.pop();
        }
        let bin = dir.join("echo");
        if bin.exists() {
            return bin;
        }

        let target_dir = dir.parent().expect("debug dir has a target-dir parent");
        let status = std::process::Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "-p", "echo", "--target-dir"])
            .arg(target_dir)
            .status()
            .expect("failed to spawn cargo build -p echo");
        assert!(
            status.success() && bin.exists(),
            "echo fixture missing at {}; build with `cargo build -p echo --target-dir {}`",
            bin.display(),
            target_dir.display()
        );
        bin
    }

    fn install_echo(extensions_dir: &Path) {
        let pkg = extensions_dir.join("echo");
        std::fs::create_dir_all(&pkg).unwrap();
        symlink(echo_extension_path(), pkg.join("extension")).unwrap();
        std::fs::write(
            pkg.join("manifest.toml"),
            "name = \"echo\"\nversion = \"0.1.0\"\nauthor = \"ArtemChuban\"\nextensions-api = { major = 0, minor = 1 }\n",
        )
        .unwrap();
    }

    fn install_script_extension(
        extensions_dir: &Path,
        name: &str,
        script: &str,
        api_major: u32,
        api_minor: u32,
    ) {
        let pkg = extensions_dir.join(name);
        std::fs::create_dir_all(&pkg).unwrap();
        let path = pkg.join("extension");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            pkg.join("manifest.toml"),
            format!(
                "name = \"{name}\"\nversion = \"0.1.0\"\nauthor = \"test\"\nextensions-api = {{ major = {api_major}, minor = {api_minor} }}\n"
            ),
        )
        .unwrap();
    }

    fn install_failing(extensions_dir: &Path, name: &str) {
        install_script_extension(extensions_dir, name, "#!/bin/sh\nexit 1\n", 0, 1);
    }

    fn install_incompatible(extensions_dir: &Path, name: &str) {
        install_script_extension(extensions_dir, name, "#!/bin/sh\nexit 0\n", 9, 0);
    }

    #[test]
    fn call_spawns_and_talks_to_the_echo_fixture() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_echo(&extensions_dir);

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        let result = registry.call("echo", "ping", "hello").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn call_reuses_a_live_connection_on_a_second_call() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_echo(&extensions_dir);

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        assert_eq!(registry.call("echo", "ping", "one").unwrap(), "one");
        assert_eq!(registry.call("echo", "ping", "two").unwrap(), "two");
    }

    #[test]
    fn call_on_uninstalled_extension_fails_without_spawning() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir.clone());
        assert!(!registry.is_installed("missing"));
        let err = registry.call("missing", "ping", "hello").unwrap_err();
        assert!(err.contains("not installed"));
        assert!(!socket_dir.exists());
    }

    #[test]
    fn call_rejects_path_escape_extension_names() {
        let base = temp_dir();
        let registry = ExtensionRegistry::new(base.join("extensions"), base.join("sockets"));
        for name in ["/usr/bin/id", "../etc/passwd", "a/b", ".", ".."] {
            let err = registry.call(name, "ping", "").unwrap_err();
            assert!(
                err.contains("invalid extension name"),
                "name {name:?} err {err}"
            );
            assert!(!registry.is_installed(name));
        }
    }

    #[test]
    fn call_on_an_extension_that_exits_immediately_fails_instead_of_hanging() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_failing(&extensions_dir, "broken");

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        let err = registry.call("broken", "ping", "hello").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn call_rejects_incompatible_extensions_api_before_spawn() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_incompatible(&extensions_dir, "oldapi");

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir.clone());
        let err = registry.call("oldapi", "ping", "hello").unwrap_err();
        assert!(
            err.contains("extensions-api"),
            "expected extensions-api mismatch, got {err}"
        );
        assert!(!socket_dir.exists());
    }
}
