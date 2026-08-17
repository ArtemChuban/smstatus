use bslstatus::module::host::Host;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, PropMode};
use x11rb::wrapper::ConnectionExt;

wasmtime::component::bindgen!({
    path: "wit",
    world: "module",
});

struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl Host for HostState {
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn read_sysfs(&mut self, path: String) -> Result<String, String> {
        std::fs::read_to_string(&path).map_err(|e| format!("read failed: {e}"))
    }

    fn local_offset_seconds(&mut self) -> i32 {
        chrono::Local::now().offset().local_minus_utc()
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
) -> Result<(Store<HostState>, Module), Box<dyn std::error::Error>> {
    let state = HostState {
        wasi_ctx: WasiCtxBuilder::new().build(),
        table: ResourceTable::new(),
        limits: StoreLimitsBuilder::new()
            .memory_size(10 * 1024 * 1024)
            .instances(3)
            .build(),
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let config_dir: PathBuf = dirs::config_dir()
        .ok_or("could not determine config directory")?
        .join("bslstatus");
    let modules_dir = config_dir.join("modules");
    let config = load_config(&config_dir.join("config.toml"))?;
    const FUEL_PER_TICK: u64 = 10_000_000;

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;

    let mut modules = Vec::new();
    for name in module_names(&config)? {
        let config = module_config_json(&config, &name);
        let component = Component::from_file(&engine, modules_dir.join(format!("{name}.wasm")))?;
        let (mut store, module) = instantiate_module(&engine, &component, &linker, FUEL_PER_TICK)?;
        module
            .bslstatus_module_guest()
            .call_init(&mut store, &config)?;
        modules.push(ModuleState {
            name,
            component,
            store,
            module,
            config,
            last_output: String::new(),
            next_due: Instant::now(),
        })
    }

    let (connection, screen_num) = x11rb::connect(None)?;
    let screen = &connection.setup().roots[screen_num];
    let root = screen.root;

    loop {
        let now = Instant::now();

        for state in modules.iter_mut() {
            if state.next_due > now {
                continue;
            }

            state.store.set_fuel(FUEL_PER_TICK)?;

            match state
                .module
                .bslstatus_module_guest()
                .call_update(&mut state.store)
            {
                Ok(output) => {
                    state.last_output = output.text;
                    state.next_due = now + Duration::from_millis(output.interval_ms as u64);
                }
                Err(err) => {
                    eprintln!("module tick failed: {err}");
                    eprintln!("re-instantiating module after trap");
                    match instantiate_module(&engine, &state.component, &linker, FUEL_PER_TICK) {
                        Ok((mut store, module)) => {
                            if let Err(err) = module
                                .bslstatus_module_guest()
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
        std::thread::sleep(sleep_for);
    }
}
