use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

use crate::bindings::GuestModule;
use crate::error::Result;
use crate::host::HostState;
use crate::host_module::HostModuleRegistry;
use crate::lock;

const FUEL_PER_PROBE: u64 = 10_000_000;

pub(crate) struct WasmProbe {
    engine: Engine,
    linker: Linker<HostState>,
    http_agent: ureq::Agent,
    host_modules: Arc<HostModuleRegistry>,
}

impl WasmProbe {
    pub(crate) fn new() -> Result<Self> {
        let mut wasm_config = Config::new();
        wasm_config.wasm_component_model(true);
        wasm_config.consume_fuel(true);
        let engine = Engine::new(&wasm_config)?;

        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        GuestModule::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
            &mut linker,
            |state: &mut HostState| state,
        )?;

        let http_agent = {
            let agent_config = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .build();
            ureq::Agent::new_with_config(agent_config)
        };

        let host_modules = Arc::new(HostModuleRegistry::new(
            crate::config::default_config_dir()?.join("host_modules"),
            lock::lock_dir()?.join("host-modules"),
        ));

        Ok(Self {
            engine,
            linker,
            http_agent,
            host_modules,
        })
    }

    pub(crate) fn instantiate(&self, path: &Path) -> Result<(Store<HostState>, GuestModule)> {
        let component = Component::from_file(&self.engine, path)?;
        let state =
            HostState::new_without_display(self.http_agent.clone(), Arc::clone(&self.host_modules));
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| state.limits());
        store.set_fuel(FUEL_PER_PROBE)?;
        let module = GuestModule::instantiate(&mut store, &component, &self.linker)?;
        Ok((store, module))
    }
}
