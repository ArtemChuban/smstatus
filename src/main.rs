use std::time::Duration;

use bslstatus::module::host::Host;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, PropMode};
use x11rb::wrapper::ConnectionExt;

wasmtime::component::bindgen!({
    path: "modules/datetime/wit",
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let component = Component::from_file(
        &engine,
        "modules/datetime/target/wasm32-wasip2/debug/datetime.wasm",
    )?;

    let mut linker = Linker::new(&engine);
    Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    const FUEL_PER_TICK: u64 = 10_000_000;
    let (mut store, mut module) = instantiate_module(&engine, &component, &linker, FUEL_PER_TICK)?;

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
                let (new_store, new_module) =
                    instantiate_module(&engine, &component, &linker, FUEL_PER_TICK)?;
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
