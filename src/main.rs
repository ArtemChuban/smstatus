use std::time::Duration;

use bslstatus::module::host::Host;
use std::path::PathBuf;
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
    allowed_sysfs_paths: Vec<PathBuf>,
}

impl Host for HostState {
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn read_sysfs(&mut self, path: String) -> Result<String, String> {
        let requested =
            std::fs::canonicalize(&path).map_err(|e| format!("cannot resolve path: {e}"))?;
        if !self.allowed_sysfs_paths.contains(&requested) {
            return Err(format!("permission denied: {path}"));
        }

        std::fs::read_to_string(&requested).map_err(|e| format!("read failed: {e}"))
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

fn instantiate_module(
    engine: &Engine,
    component: &Component,
    linker: &Linker<HostState>,
    fuel: u64,
    allowed_sysfs_paths: Vec<PathBuf>,
) -> Result<(Store<HostState>, Module), Box<dyn std::error::Error>> {
    let allowed_sysfs_paths = allowed_sysfs_paths
        .into_iter()
        .filter_map(|p| std::fs::canonicalize(&p).ok())
        .collect();
    let state = HostState {
        wasi_ctx: WasiCtxBuilder::new().build(),
        table: ResourceTable::new(),
        limits: StoreLimitsBuilder::new()
            .memory_size(10 * 1024 * 1024)
            .instances(3)
            .build(),
        allowed_sysfs_paths,
    };
    let mut store = Store::new(engine, state);
    store.limiter(|state| &mut state.limits);
    store.set_fuel(fuel)?;
    let module = Module::instantiate(&mut store, component, linker)?;
    Ok((store, module))
}

fn load_module_config(config_path: &std::path::Path, module_name: &str) -> String {
    let Ok(contents) = std::fs::read_to_string(config_path) else {
        return "{}".to_string();
    };
    let Ok(parsed): Result<toml::Table, _> = toml::from_str(&contents) else {
        return "{}".to_string();
    };
    match parsed.get(module_name) {
        Some(section) => serde_json::to_string(section).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
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
    let module_name = "datetime";
    let component = Component::from_file(&engine, modules_dir.join(format!("{module_name}.wasm")))?;
    let module_config = load_module_config(&config_dir.join("config.toml"), module_name);

    let mut linker = Linker::new(&engine);
    Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    const FUEL_PER_TICK: u64 = 10_000_000;
    let (mut store, mut module) = instantiate_module(
        &engine,
        &component,
        &linker,
        FUEL_PER_TICK,
        vec![PathBuf::from("/sys/class/power_supply/BAT1/capacity")],
    )?;
    module
        .bslstatus_module_guest()
        .call_init(&mut store, &module_config)?;

    let (connection, screen_num) = x11rb::connect(None)?;
    let screen = &connection.setup().roots[screen_num];
    let root = screen.root;

    loop {
        store.set_fuel(FUEL_PER_TICK)?;

        let output = match module.bslstatus_module_guest().call_update(&mut store) {
            Ok(output) => output,
            Err(err) => {
                eprintln!("module tick failed: {err}");
                eprintln!("re-instantiating module after trap");
                let (new_store, new_module) = instantiate_module(
                    &engine,
                    &component,
                    &linker,
                    FUEL_PER_TICK,
                    vec![PathBuf::from("/sys/class/power_supply/BAT1/capacity")],
                )?;
                store = new_store;
                module = new_module;
                std::thread::sleep(Duration::from_millis(1_000));
                continue;
            }
        };

        connection.change_property8(
            PropMode::REPLACE,
            root,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            output.text.as_bytes(),
        )?;
        connection.flush()?;

        println!("root name set to: {}", output.text);
        std::thread::sleep(Duration::from_millis(output.interval_ms as u64));
    }
}
