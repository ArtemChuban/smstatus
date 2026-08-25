use std::path::Path;

use crate::bindings::Metadata;
use crate::error::Result;
use crate::module::wait_wasm_stable;
use crate::probe::WasmProbe;

pub(crate) struct MetadataProbe {
    probe: WasmProbe,
}

impl MetadataProbe {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            probe: WasmProbe::new()?,
        })
    }

    pub(crate) fn read(&self, modules_dir: &Path, kind: &str) -> Result<Metadata> {
        self.read_path(&modules_dir.join(format!("{kind}.wasm")), false)
    }

    pub(crate) fn read_after_stable(&self, modules_dir: &Path, kind: &str) -> Result<Metadata> {
        self.read_path(&modules_dir.join(format!("{kind}.wasm")), true)
    }

    pub(crate) fn read_path(&self, path: &Path, wait_stable: bool) -> Result<Metadata> {
        if wait_stable {
            wait_wasm_stable(path);
        }
        let (mut store, module) = self.probe.instantiate(path)?;
        Ok(module
            .smstatus_module_guest()
            .call_get_metadata(&mut store)?)
    }
}
