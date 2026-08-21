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
        if path.extension().and_then(|ext| ext.to_str()) == Some("wasm")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            kinds.push(stem.to_string());
        }
    }
    kinds.sort_unstable();
    Ok(kinds)
}
