use std::path::Path;

use crate::error::Result;

use super::BarConfig;
use super::io::atomic_write;

impl BarConfig {
    pub(crate) fn write_separator(path: &Path, new_value: &str) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        doc["separator"] = toml_edit::value(new_value);
        atomic_write(path, &doc.to_string())
    }

    pub(crate) fn write_module_order(path: &Path, modules: &[String]) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        let array = doc
            .get_mut("modules")
            .and_then(|item| item.as_array_mut())
            .ok_or_else(|| format!("{}: `modules` is not an array", path.display()))?;

        let current = extract_module_strings(array, path)?;
        let mut current_sorted = current.clone();
        current_sorted.sort_unstable();
        let mut modules_sorted = modules.to_vec();
        modules_sorted.sort_unstable();
        if current_sorted != modules_sorted {
            return Err(format!(
                "{}: modules list changed on disk since it was last loaded; reload and try again",
                path.display()
            )
            .into());
        }

        let mut indices_by_name: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, name) in current.iter().enumerate() {
            indices_by_name.entry(name.as_str()).or_default().push(i);
        }
        let mut originals: Vec<Option<toml_edit::Value>> = Vec::with_capacity(current.len());
        for _ in 0..current.len() {
            originals.push(Some(array.remove(0)));
        }
        for target_name in modules {
            let idx = indices_by_name
                .get_mut(target_name.as_str())
                .and_then(|indices| {
                    if indices.is_empty() {
                        None
                    } else {
                        Some(indices.remove(0))
                    }
                })
                .expect("the content-equality check above guarantees a matching original index");
            let value = originals[idx]
                .take()
                .expect("each original index is drained exactly once");
            array.push_formatted(value);
        }

        atomic_write(path, &doc.to_string())
    }

    pub(crate) fn write_module_add(
        path: &Path,
        expected_current: &[String],
        new_entry: &str,
    ) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        let array = doc
            .get_mut("modules")
            .and_then(|item| item.as_array_mut())
            .ok_or_else(|| format!("{}: `modules` is not an array", path.display()))?;

        let current = extract_module_strings(array, path)?;
        if current != expected_current {
            return Err(format!(
                "{}: modules list changed on disk since it was last loaded; reload and try again",
                path.display()
            )
            .into());
        }
        if current.iter().any(|m| m == new_entry) {
            return Err(format!(
                "{}: module `{new_entry}` is already present",
                path.display()
            )
            .into());
        }

        array.push(new_entry);
        atomic_write(path, &doc.to_string())
    }

    pub(crate) fn write_module_remove(
        path: &Path,
        expected_current: &[String],
        remove_index: usize,
    ) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        let array = doc
            .get_mut("modules")
            .and_then(|item| item.as_array_mut())
            .ok_or_else(|| format!("{}: `modules` is not an array", path.display()))?;

        let current = extract_module_strings(array, path)?;
        if current != expected_current {
            return Err(format!(
                "{}: modules list changed on disk since it was last loaded; reload and try again",
                path.display()
            )
            .into());
        }
        if remove_index >= expected_current.len() {
            return Err(format!(
                "{}: remove index {remove_index} out of range for a {}-entry list",
                path.display(),
                expected_current.len()
            )
            .into());
        }

        array.remove(remove_index);
        atomic_write(path, &doc.to_string())
    }
}

fn extract_module_strings(array: &toml_edit::Array, path: &Path) -> Result<Vec<String>> {
    let mut current: Vec<String> = Vec::with_capacity(array.len());
    for item in array.iter() {
        match item.as_str() {
            Some(s) => current.push(s.to_string()),
            None => {
                return Err(
                    format!("{}: `modules` entries must be strings", path.display()).into(),
                );
            }
        }
    }
    Ok(current)
}
