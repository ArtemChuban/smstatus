use std::path::Path;

use crate::error::Result;

use super::io::atomic_write;
use super::{BarConfig, ParamWriteExpect};

impl BarConfig {
    pub(crate) fn write_module_param_set(
        path: &Path,
        section: &str,
        key: &str,
        new_value: &str,
        expected: &ParamWriteExpect,
    ) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        let table = module_section_table_mut(
            &mut doc,
            section,
            path,
            /*create_if_missing*/ matches!(expected, ParamWriteExpect::KeyAbsent),
        )?;
        match expected {
            ParamWriteExpect::KeyAbsent => {
                if table.contains_key(key) {
                    return Err(format!(
                        "{}: key `{key}` already present in [{section}]",
                        path.display()
                    )
                    .into());
                }
            }
            ParamWriteExpect::ExistingString(want) => match table.get(key) {
                None => {
                    return Err(format!(
                        "{}: key `{key}` missing or not a string in [{section}]",
                        path.display()
                    )
                    .into());
                }
                Some(item) => match item.as_str() {
                    Some(got) if got == want => {}
                    Some(_) | None => {
                        return Err(format!(
                            "{}: [{section}].{key} changed on disk since it was last loaded; reload and try again",
                            path.display()
                        )
                        .into());
                    }
                },
            },
            ParamWriteExpect::ExistingNonString => {
                let Some(item) = table.get(key) else {
                    return Err(
                        format!("{}: key `{key}` missing in [{section}]", path.display()).into(),
                    );
                };
                if item.as_str().is_some() {
                    return Err(format!(
                        "{}: [{section}].{key} changed on disk since it was last loaded; reload and try again",
                        path.display()
                    )
                    .into());
                }
            }
        }
        match table.get_mut(key) {
            Some(item) => {
                if let Some(existing) = item.as_value_mut() {
                    let decor = existing.decor().clone();
                    *existing = new_value.into();
                    *existing.decor_mut() = decor;
                } else {
                    *item = toml_edit::value(new_value);
                }
            }
            None => {
                table.insert(key, toml_edit::value(new_value));
            }
        }
        atomic_write(path, &doc.to_string())
    }

    pub(crate) fn write_module_param_remove(path: &Path, section: &str, key: &str) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        let table =
            module_section_table_mut(&mut doc, section, path, /*create_if_missing*/ false)?;
        if !table.contains_key(key) {
            return Err(format!("{}: key `{key}` missing in [{section}]", path.display()).into());
        }
        table.remove(key);
        atomic_write(path, &doc.to_string())
    }

    pub(crate) fn write_module_param_rename(
        path: &Path,
        section: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
        let table =
            module_section_table_mut(&mut doc, section, path, /*create_if_missing*/ false)?;
        if !table.contains_key(old_key) {
            return Err(
                format!("{}: key `{old_key}` missing in [{section}]", path.display()).into(),
            );
        }
        if table.contains_key(new_key) {
            return Err(format!(
                "{}: key `{new_key}` already present in [{section}]",
                path.display()
            )
            .into());
        }
        let order: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
        let pos = order
            .iter()
            .position(|k| k == old_key)
            .ok_or_else(|| format!("{}: key `{old_key}` missing in [{section}]", path.display()))?;
        let (old_fmt_key, item) = table
            .remove_entry(old_key)
            .ok_or_else(|| format!("{}: key `{old_key}` missing in [{section}]", path.display()))?;
        let mut new_fmt_key = toml_edit::Key::new(new_key);
        *new_fmt_key.leaf_decor_mut() = old_fmt_key.leaf_decor().clone();
        *new_fmt_key.dotted_decor_mut() = old_fmt_key.dotted_decor().clone();
        let trailing: Vec<(toml_edit::Key, toml_edit::Item)> = order
            .get(pos + 1..)
            .unwrap_or(&[])
            .iter()
            .filter_map(|k| table.remove_entry(k))
            .collect();
        table.insert_formatted(&new_fmt_key, item);
        for (k, v) in trailing {
            table.insert_formatted(&k, v);
        }
        atomic_write(path, &doc.to_string())
    }
}

fn module_section_table_mut<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    section: &str,
    path: &Path,
    create_if_missing: bool,
) -> Result<&'a mut toml_edit::Table> {
    match doc.get(section) {
        None => {
            if !create_if_missing {
                return Err(format!("{}: section [{section}] is missing", path.display()).into());
            }
            doc[section] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        Some(item) if item.as_table().is_some() => {}
        Some(_) => {
            return Err(format!(
                "{}: top-level key `{section}` exists but is not a table; refusing to overwrite",
                path.display()
            )
            .into());
        }
    }
    doc.get_mut(section)
        .and_then(|item| item.as_table_mut())
        .ok_or_else(|| format!("{}: section [{section}] is not a table", path.display()).into())
}
