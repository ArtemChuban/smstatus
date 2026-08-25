use std::path::Path;

use crate::bindings::ConfigParam;
use crate::error::Result;
use crate::module::wait_wasm_stable;
use crate::probe::WasmProbe;

pub(crate) struct SchemaProbe {
    probe: WasmProbe,
}

impl SchemaProbe {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            probe: WasmProbe::new()?,
        })
    }

    pub(crate) fn read(&self, modules_dir: &Path, kind: &str) -> Result<Vec<ConfigParam>> {
        self.read_path(&crate::manifest::module_wasm_path(modules_dir, kind), false)
    }

    pub(crate) fn read_after_stable(
        &self,
        modules_dir: &Path,
        kind: &str,
    ) -> Result<Vec<ConfigParam>> {
        self.read_path(&crate::manifest::module_wasm_path(modules_dir, kind), true)
    }

    fn read_path(&self, path: &Path, wait_stable: bool) -> Result<Vec<ConfigParam>> {
        if wait_stable {
            wait_wasm_stable(path);
        }
        let (mut store, module) = self.probe.instantiate(path)?;
        Ok(module
            .smstatus_module_guest()
            .call_config_schema(&mut store)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_err_for_missing_wasm_path() {
        let probe = SchemaProbe::new().unwrap();
        let result = probe.read(Path::new("/nonexistent/modules/dir"), "cpu");
        assert!(result.is_err());
    }
}
