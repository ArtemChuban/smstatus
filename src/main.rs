use bslstatus::module::host::Host;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    path: "modules/example/wit",
    world: "module",
});

struct HostState {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
}

impl Host for HostState {
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn read_sysfs(&mut self, _path: String) -> Result<String, String> {
        Err("not implemented".to_string())
    }

    fn read_config(&mut self, _key: String) -> Option<String> {
        None
    }

    fn log(&mut self, msg: String) {
        println!("[module log] {msg}");
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    let component = Component::from_file(
        &engine,
        "modules/example/target/wasm32-wasip2/debug/example.wasm",
    )?;

    let mut linker = Linker::new(&engine);
    Module::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    let state = HostState {
        wasi_ctx: WasiCtxBuilder::new().build(),
        table: ResourceTable::new(),
    };
    let mut store = Store::new(&engine, state);
    let module = Module::instantiate(&mut store, &component, &linker)?;

    let output = module.bslstatus_module_guest().call_update(&mut store)?;
    println!(
        "update() -> text={:?} interval_ms={}",
        output.text, output.interval_ms
    );

    Ok(())
}
