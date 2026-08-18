use notify::{Event, EventKind, RecursiveMode, Watcher, event::ModifyKind};
use smstatus::module::host::{DiskUsage, Host, TimeState, XkbState};
use std::collections::HashMap;
use std::path::PathBuf;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let config_dir: PathBuf = dirs::config_dir()
        .ok_or("could not determine config directory")?
        .join("smstatus");
    let modules_dir = config_dir.join("modules");
    let config = load_config(&config_dir.join("config.toml"))?;
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
            .join(" | ");

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
