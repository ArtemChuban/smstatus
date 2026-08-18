use clap::{Parser, Subcommand};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal::{Signal, kill};
use nix::unistd::{Pid, setsid};
use notify::{Event, EventKind, RecursiveMode, Watcher, event::ModifyKind};
use smstatus::module::host::{DiskUsage, Host, MemUsage, TimeState, XkbState};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use x11rb::connection::Connection;
use x11rb::protocol::xkb;
use x11rb::protocol::xkb::ConnectionExt as _;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

const DAEMON_ENV_VAR: &str = "SMSTATUS_DAEMON_CHILD";
const EXIT_ALREADY_RUNNING: u8 = 3;

#[derive(Parser)]
#[command(
    name = "smstatus",
    about = "suckmore status",
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true
)]
struct Cli {
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start smstatus as a background daemon.
    Start,
    /// Stop the running smstatus daemon.
    Stop,
    /// Run smstatus in the foreground (for debugging).
    Run,
}

wasmtime::component::bindgen!({
    path: "wit",
    world: "module",
});

struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    connection: Arc<RustConnection>,
}

impl Host for HostState {
    fn read_sysfs(&mut self, path: String) -> Result<String, String> {
        std::fs::read_to_string(&path).map_err(|e| format!("read failed: {e}"))
    }

    fn read_time_state(&mut self) -> TimeState {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let offset_seconds = chrono::Local::now().offset().local_minus_utc();
        TimeState {
            now_ms,
            offset_seconds,
        }
    }

    fn read_xkb_state(&mut self) -> Result<XkbState, String> {
        let group = self
            .connection
            .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .group;

        let names_reply = self
            .connection
            .xkb_get_names(xkb::ID::USE_CORE_KBD.into(), xkb::NameDetail::SYMBOLS)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;

        let symbols_atom = names_reply
            .value_list
            .symbols_name
            .ok_or("no symbols name reported")?;

        let symbols = self
            .connection
            .get_atom_name(symbols_atom)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())
            .map(|r| String::from_utf8_lossy(&r.name).into_owned())?;

        Ok(XkbState {
            active_group: u8::from(group),
            symbols,
        })
    }

    fn read_disk_usage(&mut self, device: String) -> Result<DiskUsage, String> {
        let target = std::fs::canonicalize(&device).unwrap_or_else(|_| PathBuf::from(&device));

        let mounts = std::fs::read_to_string("/proc/mounts")
            .map_err(|e| format!("cannot read /proc/mounts: {e}"))?;
        let mount_point = mounts
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let dev = fields.next()?;
                let mount_point = fields.next()?;
                let dev_canon = std::fs::canonicalize(dev).unwrap_or_else(|_| PathBuf::from(dev));
                (dev_canon == target).then(|| mount_point.to_string())
            })
            .next()
            .ok_or_else(|| format!("device `{device}` not found in /proc/mounts"))?;

        let stat = nix::sys::statvfs::statvfs(mount_point.as_str())
            .map_err(|e| format!("statvfs failed for `{mount_point}`: {e}"))?;
        let block_size = stat.fragment_size();
        let total_bytes = stat.blocks() * block_size;
        let free_bytes = stat.blocks_free() * block_size;
        let used_bytes = total_bytes.saturating_sub(free_bytes);

        Ok(DiskUsage {
            total_bytes,
            used_bytes,
            free_bytes,
        })
    }

    fn read_mem_usage(&mut self) -> Result<MemUsage, String> {
        let meminfo = std::fs::read_to_string("/proc/meminfo")
            .map_err(|e| format!("cannot read /proc/meminfo: {e}"))?;

        let field = |key: &str| -> Option<u64> {
            meminfo.lines().find_map(|line| {
                let rest = line.strip_prefix(key)?;
                rest.split_whitespace().next()?.parse().ok()
            })
        };

        let total_kb = field("MemTotal:").ok_or("MemTotal not found in /proc/meminfo")?;
        let available_kb =
            field("MemAvailable:").ok_or("MemAvailable not found in /proc/meminfo")?;

        let total_bytes = total_kb * 1024;
        let free_bytes = available_kb * 1024;
        let used_bytes = total_bytes.saturating_sub(free_bytes);

        Ok(MemUsage {
            total_bytes,
            used_bytes,
            free_bytes,
        })
    }

    fn read_process_running(&mut self, name: String) -> Result<bool, String> {
        let entries = std::fs::read_dir("/proc").map_err(|e| format!("cannot read /proc: {e}"))?;
        for entry in entries.flatten() {
            let is_pid_dir = entry
                .file_name()
                .to_str()
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
            if !is_pid_dir {
                continue;
            }
            let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
                continue;
            };
            if comm.trim() == name {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.table,
        }
    }
}

struct ModuleState {
    name: String,
    component: Component,
    store: Store<HostState>,
    module: Module,
    config: String,
    last_output: String,
    next_due: Instant,
}

fn instantiate_module(
    engine: &Engine,
    component: &Component,
    linker: &Linker<HostState>,
    fuel: u64,
    connection: Arc<RustConnection>,
) -> Result<(Store<HostState>, Module), Box<dyn std::error::Error>> {
    let state = HostState {
        wasi_ctx: WasiCtxBuilder::new().build(),
        table: ResourceTable::new(),
        limits: StoreLimitsBuilder::new()
            .memory_size(10 * 1024 * 1024)
            .instances(3)
            .build(),
        connection,
    };
    let mut store = Store::new(engine, state);
    store.limiter(|state| &mut state.limits);
    store.set_fuel(fuel)?;
    let module = Module::instantiate(&mut store, component, linker)?;
    Ok((store, module))
}

fn load_config(path: &std::path::Path) -> Result<toml::Table, Box<dyn std::error::Error>> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("cannot ead {}: {e}", path.display()))?;
    Ok(toml::from_str(&content)?)
}

fn module_config_json(config: &toml::Table, module_name: &str) -> String {
    match config.get(module_name) {
        Some(section) => serde_json::to_string(section).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

fn read_separator(config: &toml::Table) -> String {
    config
        .get("separator")
        .and_then(|v| v.as_str())
        .unwrap_or(" | ")
        .to_string()
}

fn module_names(config: &toml::Table) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let modules = config
        .get("modules")
        .and_then(|v| v.as_array())
        .ok_or("config.toml must have a top-level modules = [...] list")?;
    modules
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| "`modules` entries must be strings".into())
        })
        .collect()
}

fn start_module(
    engine: &Engine,
    linker: &Linker<HostState>,
    modules_dir: &std::path::Path,
    name: &str,
    config: &str,
    fuel: u64,
    connection: Arc<RustConnection>,
) -> Result<ModuleState, Box<dyn std::error::Error>> {
    let component = Component::from_file(engine, modules_dir.join(format!("{name}.wasm")))?;
    let (mut store, module) = instantiate_module(engine, &component, linker, fuel, connection)?;
    module
        .smstatus_module_guest()
        .call_init(&mut store, config)?;
    Ok(ModuleState {
        name: name.to_string(),
        component,
        store,
        module,
        config: config.to_string(),
        last_output: String::new(),
        next_due: Instant::now(),
    })
}

fn reload_config(
    engine: &Engine,
    linker: &Linker<HostState>,
    modules_dir: &std::path::Path,
    old_modules: Vec<ModuleState>,
    new_config: &toml::Table,
    fuel: u64,
    connection: &Arc<RustConnection>,
) -> Vec<ModuleState> {
    let new_names = match module_names(new_config) {
        Ok(names) => names,
        Err(err) => {
            eprintln!("reload aborted, bad `modules` list: {err}");
            return old_modules;
        }
    };

    let mut old_by_name: HashMap<String, Vec<ModuleState>> = HashMap::new();
    for module in old_modules {
        old_by_name
            .entry(module.name.clone())
            .or_default()
            .push(module);
    }

    let mut new_modules = Vec::with_capacity(new_names.len());
    for name in new_names {
        let config = module_config_json(new_config, &name);
        let reused = old_by_name
            .get_mut(&name)
            .filter(|v| !v.is_empty())
            .map(|v| v.remove(0));

        match reused {
            Some(existing) if existing.config == config => {
                new_modules.push(existing);
            }
            Some(mut existing) => {
                let reinit_result = existing
                    .store
                    .set_fuel(fuel)
                    .map_err(|e| e.to_string())
                    .and_then(|()| {
                        existing
                            .module
                            .smstatus_module_guest()
                            .call_init(&mut existing.store, &config)
                            .map_err(|e| e.to_string())
                    });
                match reinit_result {
                    Ok(()) => {
                        existing.config = config;
                        existing.next_due = Instant::now();
                    }
                    Err(err) => {
                        eprintln!(
                            "failed to re-init `{name}` with new config, keeping old config running: {err}"
                        );
                    }
                }
                new_modules.push(existing);
            }
            None => match start_module(
                engine,
                linker,
                modules_dir,
                &name,
                &config,
                fuel,
                Arc::clone(connection),
            ) {
                Ok(state) => new_modules.push(state),
                Err(err) => eprintln!("failed to start new module `{name}`: {err}"),
            },
        }
    }
    new_modules
}

enum LockOutcome {
    Acquired(Flock<File>),
    AlreadyRunning(Option<i32>),
}

fn lock_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(runtime_dir) = dirs::runtime_dir() {
        return Ok(runtime_dir.join("smstatus"));
    }
    let config_dir = dirs::config_dir().ok_or("could not determine config directory")?;
    Ok(config_dir.join("smstatus").join("run"))
}

fn lock_file_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(lock_dir()?.join("smstatus.lock"))
}

fn log_file_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(lock_dir()?.join("smstatus.log"))
}

fn read_pid(file: &mut File) -> Option<i32> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    contents.trim().parse().ok()
}

fn acquire_lock() -> Result<LockOutcome, Box<dyn std::error::Error>> {
    let path = lock_file_path()?;
    std::fs::create_dir_all(path.parent().ok_or("lock file has no parent directory")?)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;

    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(mut flock) => {
            flock.set_len(0)?;
            flock.seek(SeekFrom::Start(0))?;
            write!(flock, "{}", std::process::id())?;
            flock.flush()?;
            Ok(LockOutcome::Acquired(flock))
        }
        Err((mut file, Errno::EWOULDBLOCK)) => Ok(LockOutcome::AlreadyRunning(read_pid(&mut file))),
        Err((_, err)) => Err(format!("failed to lock {}: {err}", path.display()).into()),
    }
}

fn run_daemon() -> ExitCode {
    match acquire_lock() {
        Ok(LockOutcome::AlreadyRunning(_)) => ExitCode::from(EXIT_ALREADY_RUNNING),
        Ok(LockOutcome::Acquired(_flock)) => match run_bar_loop() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("smstatus exited with an error: {err}");
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            eprintln!("failed to acquire lock: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_run() -> ExitCode {
    match acquire_lock() {
        Ok(LockOutcome::AlreadyRunning(pid)) => {
            match pid {
                Some(pid) => eprintln!("smstatus is already running (pid {pid})"),
                None => eprintln!("smstatus is already running"),
            }
            ExitCode::from(EXIT_ALREADY_RUNNING)
        }
        Ok(LockOutcome::Acquired(_flock)) => match run_bar_loop() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("smstatus exited with an error: {err}");
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            eprintln!("failed to acquire lock: {err}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_start() -> ExitCode {
    let log_path = match log_file_path() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine log file path: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(parent) = log_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!("failed to create {}: {err}", parent.display());
        return ExitCode::FAILURE;
    }
    let log_file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    {
        Ok(file) => file,
        Err(err) => {
            eprintln!("failed to open log file {}: {err}", log_path.display());
            return ExitCode::FAILURE;
        }
    };
    let stdout_log = match log_file.try_clone() {
        Ok(file) => file,
        Err(err) => {
            eprintln!("failed to duplicate log file handle: {err}");
            return ExitCode::FAILURE;
        }
    };

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine current executable: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut command = Command::new(current_exe);
    command
        .env(DAEMON_ENV_VAR, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(log_file));
    unsafe {
        command.pre_exec(|| setsid().map(|_| ()).map_err(std::io::Error::from));
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("failed to spawn smstatus daemon: {err}");
            return ExitCode::FAILURE;
        }
    };

    let lock_path = match lock_file_path() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine lock file path: {err}");
            return ExitCode::FAILURE;
        }
    };

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.code() == Some(EXIT_ALREADY_RUNNING as i32) {
                    let pid = File::open(&lock_path)
                        .ok()
                        .and_then(|mut f| read_pid(&mut f));
                    match pid {
                        Some(pid) => eprintln!("smstatus is already running (pid {pid})"),
                        None => eprintln!("smstatus is already running"),
                    }
                    return ExitCode::from(EXIT_ALREADY_RUNNING);
                }
                eprintln!("smstatus failed to start, see {}:", log_path.display());
                if let Ok(contents) = std::fs::read_to_string(&log_path) {
                    eprint!("{contents}");
                }
                return ExitCode::FAILURE;
            }
            Ok(None) => {
                if let Some(pid) = File::open(&lock_path)
                    .ok()
                    .and_then(|mut f| read_pid(&mut f))
                    && pid == child.id() as i32
                {
                    println!("smstatus started (pid {pid})");
                    return ExitCode::SUCCESS;
                }
            }
            Err(err) => {
                eprintln!("failed to check daemon status: {err}");
                return ExitCode::FAILURE;
            }
        }

        if Instant::now() >= deadline {
            eprintln!(
                "smstatus did not confirm startup in time; check {}",
                log_path.display()
            );
            return ExitCode::FAILURE;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn cmd_stop() -> ExitCode {
    let lock_path = match lock_file_path() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine lock file path: {err}");
            return ExitCode::FAILURE;
        }
    };

    if !lock_path.exists() {
        println!("smstatus is not running");
        return ExitCode::SUCCESS;
    }

    let file = match OpenOptions::new().read(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("failed to open {}: {err}", lock_path.display());
            return ExitCode::FAILURE;
        }
    };

    let mut file = match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(flock) => {
            drop(flock);
            println!("smstatus is not running");
            return ExitCode::SUCCESS;
        }
        Err((file, Errno::EWOULDBLOCK)) => file,
        Err((_, err)) => {
            eprintln!("failed to check lock on {}: {err}", lock_path.display());
            return ExitCode::FAILURE;
        }
    };

    let pid = match read_pid(&mut file) {
        Some(pid) => pid,
        None => {
            eprintln!("smstatus is running, but its pid file is unreadable");
            return ExitCode::FAILURE;
        }
    };
    let target = Pid::from_raw(pid);

    match kill(target, Signal::SIGTERM) {
        Ok(()) => {}
        Err(Errno::ESRCH) => {
            println!("smstatus is not running");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("failed to signal smstatus (pid {pid}): {err}");
            return ExitCode::FAILURE;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Err(Errno::ESRCH) = kill(target, None) {
            println!("smstatus stopped (pid {pid})");
            return ExitCode::SUCCESS;
        }
        if Instant::now() >= deadline {
            eprintln!("sent SIGTERM to smstatus (pid {pid}) but it has not exited yet");
            return ExitCode::FAILURE;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn main() -> ExitCode {
    if std::env::var_os(DAEMON_ENV_VAR).is_some() {
        return run_daemon();
    }

    match Cli::parse().command {
        Commands::Start => cmd_start(),
        Commands::Stop => cmd_stop(),
        Commands::Run => cmd_run(),
    }
}

fn run_bar_loop() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let config_dir: PathBuf = dirs::config_dir()
        .ok_or("could not determine config directory")?
        .join("smstatus");
    let modules_dir = config_dir.join("modules");
    let config = load_config(&config_dir.join("config.toml"))?;
    let mut separator = read_separator(&config);
    const FUEL_PER_TICK: u64 = 10_000_000;

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;

    let (connection, screen_num) = x11rb::connect(None)?;
    connection.xkb_use_extension(1, 0)?.reply()?;
    let connection = Arc::new(connection);
    let screen = &connection.setup().roots[screen_num];
    let root = screen.root;

    let mut modules = Vec::new();
    for name in module_names(&config)? {
        let config = module_config_json(&config, &name);
        modules.push(start_module(
            &engine,
            &linker,
            &modules_dir,
            &name,
            &config,
            FUEL_PER_TICK,
            Arc::clone(&connection),
        )?)
    }

    let (reload_tx, reload_rx) = mpsc::channel::<()>();
    let watch_target = config_dir.join("config.toml");
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| match res {
        Ok(event) => {
            let is_content_change = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Data(_))
                    | EventKind::Modify(ModifyKind::Name(_))
            );
            if is_content_change && event.paths.contains(&watch_target) {
                let _ = reload_tx.send(());
            }
        }
        Err(err) => eprintln!("config watcher error: {err}"),
    })?;
    watcher.watch(&config_dir, RecursiveMode::NonRecursive)?;
    let mut watcher_alive = true;

    loop {
        let now = Instant::now();

        for state in modules.iter_mut() {
            if state.next_due > now {
                continue;
            }

            state.store.set_fuel(FUEL_PER_TICK)?;

            match state
                .module
                .smstatus_module_guest()
                .call_update(&mut state.store)
            {
                Ok(output) => {
                    state.last_output = output.text;
                    state.next_due = now + Duration::from_millis(output.interval_ms as u64);
                }
                Err(err) => {
                    eprintln!("module tick failed: {err}");
                    eprintln!("re-instantiating module after trap");
                    match instantiate_module(
                        &engine,
                        &state.component,
                        &linker,
                        FUEL_PER_TICK,
                        Arc::clone(&connection),
                    ) {
                        Ok((mut store, module)) => {
                            if let Err(err) = module
                                .smstatus_module_guest()
                                .call_init(&mut store, &state.config)
                            {
                                eprintln!("failed to re-init `{}`: {err}", state.name);
                            }
                            state.store = store;
                            state.module = module;
                        }
                        Err(err) => eprintln!("failed to re-instantiate `{}`: {err}", state.name),
                    }
                    state.next_due = now + Duration::from_millis(1_000);
                }
            };
        }

        let combined = modules
            .iter()
            .map(|s| s.last_output.as_str())
            .collect::<Vec<_>>()
            .join(&separator);

        connection.change_property8(
            PropMode::REPLACE,
            root,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            combined.as_bytes(),
        )?;

        connection.flush()?;

        println!("root name set to: {}", combined);
        let sleep_for = modules
            .iter()
            .map(|s| s.next_due.saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(Duration::from_millis(100).max(Duration::from_millis(20)));

        if watcher_alive {
            match reload_rx.recv_timeout(sleep_for) {
                Ok(()) => {
                    while reload_rx.recv_timeout(Duration::from_millis(100)).is_ok() {}
                    match load_config(&config_dir.join("config.toml")) {
                        Ok(new_config) => {
                            separator = read_separator(&new_config);
                            modules = reload_config(
                                &engine,
                                &linker,
                                &modules_dir,
                                modules,
                                &new_config,
                                FUEL_PER_TICK,
                                &connection,
                            );
                        }
                        Err(err) => eprintln!(
                            "config reload failed ({err}); keeping previous configuration runnong"
                        ),
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    eprintln!(
                        "config watcher channel disconnected; disabling hot-reload for the rest of this run"
                    );
                    watcher_alive = false;
                }
            }
        } else {
            std::thread::sleep(sleep_for);
        }
    }
}
