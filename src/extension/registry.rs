use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use extension_protocol::{self as protocol, FromExtension, Request, Response};

use super::bus::{ExtensionEvent, ExtensionEventBus};
use super::client;

const BASE_BACKOFF: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(5);
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(20);
const SPAWN_TIMEOUT: Duration = Duration::from_secs(1);
const RPC_TIMEOUT: Duration = Duration::from_secs(2);

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum ExtensionLiveState {
    Idle,
    Live,
    BackingOff { retry_in_secs: u64 },
}

type PendingResponseTx = mpsc::Sender<Result<String, String>>;

struct LiveConnection {
    writer: Mutex<UnixStream>,
    pending: Arc<Mutex<Option<PendingResponseTx>>>,
    rpc_gate: Mutex<()>,
}

impl Drop for LiveConnection {
    fn drop(&mut self) {
        if let Ok(writer) = self.writer.get_mut() {
            let _ = writer.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl LiveConnection {
    fn start(extension: String, stream: UnixStream, bus: Arc<ExtensionEventBus>) -> Self {
        let writer = stream
            .try_clone()
            .expect("UnixStream::try_clone for extension writer");
        let mut reader = stream;
        let pending: Arc<Mutex<Option<PendingResponseTx>>> = Arc::new(Mutex::new(None));
        let pending_reader = Arc::clone(&pending);

        thread::spawn(move || {
            loop {
                match protocol::read_frame::<_, FromExtension>(&mut reader) {
                    Ok(FromExtension::Event(event)) => {
                        bus.publish(ExtensionEvent {
                            extension: extension.clone(),
                            event: event.name,
                            payload: event.payload,
                        });
                    }
                    Ok(FromExtension::Response(response)) => {
                        let tx = lock(&pending_reader).take();
                        if let Some(tx) = tx {
                            let result = match response {
                                Response::Ok(value) => Ok(value),
                                Response::Err(err) => Err(err),
                            };
                            let _ = tx.send(result);
                        }
                    }
                    Err(_) => {
                        if let Some(tx) = lock(&pending_reader).take() {
                            let _ = tx.send(Err("extension connection closed".to_string()));
                        }
                        break;
                    }
                }
            }
        });

        Self {
            writer: Mutex::new(writer),
            pending,
            rpc_gate: Mutex::new(()),
        }
    }

    fn rpc(&self, method: &str, payload: &str) -> Result<String, String> {
        let _gate = lock(&self.rpc_gate);
        let (tx, rx) = mpsc::channel();
        {
            let mut pending = lock(&self.pending);
            if pending.is_some() {
                return Err("extension already has an in-flight request".to_string());
            }
            *pending = Some(tx);
        }

        {
            let mut writer = lock(&self.writer);
            writer
                .set_write_timeout(Some(RPC_TIMEOUT))
                .map_err(|e| e.to_string())?;
            if let Err(err) = protocol::write_frame(
                &mut *writer,
                &Request {
                    method: method.to_string(),
                    payload: payload.to_string(),
                },
            ) {
                lock(&self.pending).take();
                return Err(err.to_string());
            }
        }

        match rx.recv_timeout(RPC_TIMEOUT) {
            Ok(result) => result,
            Err(_) => {
                lock(&self.pending).take();
                Err("timed out waiting for extension response".to_string())
            }
        }
    }
}

pub(crate) struct ExtensionRegistry {
    extensions_dir: PathBuf,
    socket_dir: PathBuf,
    bus: Arc<ExtensionEventBus>,
    connections: Mutex<HashMap<String, Arc<LiveConnection>>>,
    children: Mutex<HashMap<String, Child>>,
    failures: Mutex<HashMap<String, FailureState>>,
}

impl ExtensionRegistry {
    pub(crate) fn new(extensions_dir: PathBuf, socket_dir: PathBuf) -> Self {
        Self::with_bus(
            extensions_dir,
            socket_dir,
            Arc::new(ExtensionEventBus::new()),
        )
    }

    pub(crate) fn with_bus(
        extensions_dir: PathBuf,
        socket_dir: PathBuf,
        bus: Arc<ExtensionEventBus>,
    ) -> Self {
        Self {
            extensions_dir,
            socket_dir,
            bus,
            connections: Mutex::new(HashMap::new()),
            children: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn event_bus(&self) -> Arc<ExtensionEventBus> {
        Arc::clone(&self.bus)
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

    pub(crate) fn installed_package_version(&self, name: &str) -> Result<(u32, u32, u32), String> {
        let manifest = crate::manifest::read_extension_manifest(&self.extensions_dir, name)
            .map_err(|e| e.to_string())?;
        crate::version::parse_package_version(&manifest.version).map_err(|e| e.to_string())
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

    fn attach_connection(&self, name: &str, stream: UnixStream) -> Arc<LiveConnection> {
        let conn = Arc::new(LiveConnection::start(
            name.to_string(),
            stream,
            Arc::clone(&self.bus),
        ));
        lock(&self.connections).insert(name.to_string(), Arc::clone(&conn));
        conn
    }

    pub(crate) fn drop_running(&self, names: &[String]) {
        for name in names {
            if !is_safe_extension_name(name) {
                log::warn!("skipping extension reload for invalid name `{name}`");
                continue;
            }
            lock(&self.connections).remove(name);
            self.kill_child(name);
            log::info!("extension `{name}` reloaded; next host call will use the binary on disk");
        }
    }

    pub(crate) fn live_state(&self, name: &str) -> ExtensionLiveState {
        if let Some(state) = lock(&self.failures).get(name) {
            let now = Instant::now();
            if now < state.retry_after {
                return ExtensionLiveState::BackingOff {
                    retry_in_secs: state
                        .retry_after
                        .saturating_duration_since(now)
                        .as_secs()
                        .max(1),
                };
            }
        }
        if lock(&self.connections).contains_key(name) {
            return ExtensionLiveState::Live;
        }
        if lock(&self.children).contains_key(name) {
            return ExtensionLiveState::Live;
        }
        ExtensionLiveState::Idle
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

        if let Some(conn) = lock(&self.connections).get(name).cloned() {
            match conn.rpc(method, payload) {
                Ok(value) => {
                    lock(&self.failures).remove(name);
                    return Ok(value);
                }
                Err(_) => {
                    lock(&self.connections).remove(name);
                }
            }
        }

        match self.spawn_and_connect(name) {
            Ok(stream) => {
                let conn = self.attach_connection(name, stream);
                match conn.rpc(method, payload) {
                    Ok(value) => {
                        lock(&self.failures).remove(name);
                        Ok(value)
                    }
                    Err(err) => {
                        lock(&self.connections).remove(name);
                        self.record_failure(name);
                        Err(format!("extension `{name}` call failed: {err}"))
                    }
                }
            }
            Err(err) => {
                self.record_failure(name);
                Err(format!("extension `{name}` call failed: {err}"))
            }
        }
    }
}

impl Drop for ExtensionRegistry {
    fn drop(&mut self) {
        lock(&self.connections).clear();
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

    #[test]
    fn drop_running_skips_unsafe_extension_names() {
        let base = temp_dir();
        let registry = ExtensionRegistry::new(base.join("extensions"), base.join("sockets"));
        registry.drop_running(&["../etc/passwd".to_string()]);
    }

    #[test]
    fn drop_running_clears_live_extension_connection() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_echo(&extensions_dir);

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        assert_eq!(registry.call("echo", "ping", "before").unwrap(), "before");
        registry.drop_running(&["echo".to_string()]);
        assert_eq!(registry.call("echo", "ping", "after").unwrap(), "after");
    }

    #[test]
    fn live_state_is_live_after_successful_call() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_echo(&extensions_dir);

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        assert_eq!(registry.live_state("echo"), ExtensionLiveState::Idle);
        registry.call("echo", "ping", "hello").unwrap();
        assert_eq!(registry.live_state("echo"), ExtensionLiveState::Live);
    }

    #[test]
    fn live_state_is_idle_after_drop_running() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_echo(&extensions_dir);

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        registry.call("echo", "ping", "hello").unwrap();
        assert_eq!(registry.live_state("echo"), ExtensionLiveState::Live);
        registry.drop_running(&["echo".to_string()]);
        assert_eq!(registry.live_state("echo"), ExtensionLiveState::Idle);
    }

    #[test]
    fn live_state_reports_backoff_after_repeated_failures() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_failing(&extensions_dir, "broken");

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        let _ = registry.call("broken", "ping", "hello");
        match registry.live_state("broken") {
            ExtensionLiveState::BackingOff { retry_in_secs } => {
                assert!(retry_in_secs > 0);
            }
            other => panic!("expected backoff, got {other:?}"),
        }
    }

    #[test]
    fn bus_receives_events_emitted_around_rpc() {
        use std::os::unix::net::UnixListener;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let socket_path = std::env::temp_dir().join(format!("smstatus-bus-event-{nanos}.sock"));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            protocol::perform_handshake_server(&mut stream).unwrap();
            let request: Request = protocol::read_frame(&mut stream).unwrap();
            assert_eq!(request.method, "ping");
            protocol::write_event(&mut stream, "tick", "1").unwrap();
            protocol::write_frame(
                &mut stream,
                &FromExtension::Response(Response::Ok(format!("pong:{}", request.payload))),
            )
            .unwrap();
        });

        let mut stream = None;
        for _ in 0..100 {
            match UnixStream::connect(&socket_path) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
        let mut stream = stream.expect("connect");
        protocol::perform_handshake_client(&mut stream).unwrap();

        let bus = Arc::new(ExtensionEventBus::new());
        let rx = bus.subscribe();
        let registry = ExtensionRegistry::with_bus(
            temp_dir().join("extensions"),
            temp_dir().join("sockets"),
            Arc::clone(&bus),
        );
        registry.attach_connection("emitter", stream);

        let result = registry.call("emitter", "ping", "hi").unwrap();
        assert_eq!(result, "pong:hi");

        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(event.extension, "emitter");
        assert_eq!(event.event, "tick");
        assert_eq!(event.payload, "1");

        drop(registry);
        server.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }
}
