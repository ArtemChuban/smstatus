use std::path::Path;
use std::time::Duration;

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

use crate::bindings::{GuestModule, Metadata};
use crate::error::Result;
use crate::host::HostState;
use crate::module::wait_wasm_stable;

const FUEL_PER_PROBE: u64 = 10_000_000;

pub(crate) struct MetadataProbe {
    engine: Engine,
    linker: Linker<HostState>,
    http_agent: ureq::Agent,
}

impl MetadataProbe {
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

        Ok(Self {
            engine,
            linker,
            http_agent,
        })
    }

    pub(crate) fn read(&self, modules_dir: &Path, kind: &str) -> Result<Metadata> {
        self.read_path(&modules_dir.join(format!("{kind}.wasm")), false)
    }

    pub(crate) fn read_after_stable(&self, modules_dir: &Path, kind: &str) -> Result<Metadata> {
        self.read_path(&modules_dir.join(format!("{kind}.wasm")), true)
    }

    fn read_path(&self, path: &Path, wait_stable: bool) -> Result<Metadata> {
        if wait_stable {
            wait_wasm_stable(path);
        }
        let component = Component::from_file(&self.engine, path)?;
        let state = HostState::new_without_display(self.http_agent.clone());
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| state.limits());
        store.set_fuel(FUEL_PER_PROBE)?;
        let module = GuestModule::instantiate(&mut store, &component, &self.linker)?;
        Ok(module
            .smstatus_module_guest()
            .call_get_metadata(&mut store)?)
    }
}
