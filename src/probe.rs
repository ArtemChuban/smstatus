use std::path::Path;
use std::sync::Arc;

use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use crate::bindings::GuestModule;
use crate::error::Result;
use crate::extension::ExtensionRegistry;
use crate::host::{self, HostState};
use crate::lock;

const FUEL_PER_PROBE: u64 = 10_000_000;

pub(crate) struct WasmProbe {
    engine: Engine,
    linker: Linker<HostState>,
    extensions: Arc<ExtensionRegistry>,
}

impl WasmProbe {
    pub(crate) fn new() -> Result<Self> {
        let (engine, linker) = host::build_engine_and_linker()?;

        let extensions = Arc::new(ExtensionRegistry::new(
            crate::config::default_config_dir()?.join("extensions"),
            lock::lock_dir()?.join("extensions"),
        ));

        Ok(Self {
            engine,
            linker,
            extensions,
        })
    }

    pub(crate) fn instantiate(&self, path: &Path) -> Result<(Store<HostState>, GuestModule)> {
        let component = Component::from_file(&self.engine, path)?;
        host::instantiate_component(
            &self.engine,
            &self.linker,
            &component,
            Arc::clone(&self.extensions),
            Arc::<[extension_protocol::PermissionEntry]>::from([]),
            FUEL_PER_PROBE,
        )
    }
}
