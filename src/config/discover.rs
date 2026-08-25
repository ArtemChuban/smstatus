use std::path::Path;

use crate::error::Result;

pub(crate) fn discover_module_kinds(modules_dir: &Path) -> Result<Vec<String>> {
    if !modules_dir.exists() {
        return Ok(Vec::new());
    }
    let mut kinds = Vec::new();
    for entry in std::fs::read_dir(modules_dir)
        .map_err(|e| format!("cannot read {}: {e}", modules_dir.display()))?
    {
        let entry =
            entry.map_err(|e| format!("cannot read entry in {}: {e}", modules_dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if path.join("manifest.toml").is_file() && path.join("module.wasm").is_file() {
            kinds.push(name.to_string());
        }
    }
    kinds.sort_unstable();
    Ok(kinds)
}
