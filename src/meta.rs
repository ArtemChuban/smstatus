use std::path::Path;

use crate::error::Result;
use crate::manifest::{self, Metadata};

pub(crate) fn read(modules_dir: &Path, kind: &str) -> Result<Metadata> {
    Ok(manifest::read_module_manifest(modules_dir, kind)?.to_metadata())
}
