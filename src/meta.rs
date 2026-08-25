use std::path::Path;

use crate::error::Result;
use crate::manifest::{self, Metadata};
use crate::module::wait_wasm_stable;

pub(crate) struct MetadataProbe;

impl MetadataProbe {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self)
    }

    pub(crate) fn read(&self, modules_dir: &Path, kind: &str) -> Result<Metadata> {
        Ok(manifest::read_module_manifest(modules_dir, kind)?.to_metadata())
    }

    pub(crate) fn read_after_stable(&self, modules_dir: &Path, kind: &str) -> Result<Metadata> {
        wait_wasm_stable(&manifest::module_wasm_path(modules_dir, kind));
        self.read(modules_dir, kind)
    }
}
