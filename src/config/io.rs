use std::path::{Path, PathBuf};

use crate::error::Result;

pub(super) fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp_path, content)
        .map_err(|e| format!("cannot write {}: {e}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("cannot replace {}: {e}", path.display())
    })?;
    Ok(())
}
