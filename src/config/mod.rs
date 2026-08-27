use std::path::{Path, PathBuf};

use crate::error::Result;

mod bar_load;
mod discover;
mod io;
mod params;
mod presets;
mod write;

pub(crate) const DEFAULT_LOG_DAYS: u64 = 7;

pub(crate) use bar_load::{BarConfigLoad, IDLE_STATUS_MESSAGE, load_bar_config};

pub(crate) use presets::{
    active_config_path, copy_preset, init_config_layout, list_preset_names, preset_file,
    read_active_name, remove_preset_file, write_active_name,
};

#[cfg(test)]
pub(crate) use presets::{DEFAULT_PRESET_NAME, create_default_preset, program_config_path};

#[cfg(test)]
pub(crate) use presets::test_fixtures;

pub(crate) use discover::discover_module_kinds;

pub(crate) fn default_config_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or("could not determine config directory")?
        .join("smstatus"))
}

pub(crate) struct BarConfig(toml::Table);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModuleSectionView {
    Missing,
    Empty,
    Entries(Vec<(String, ModuleParamValue)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModuleParamValue {
    String(String),
    NonString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParamWriteExpect {
    ExistingString(String),
    ExistingNonString,
    KeyAbsent,
}

impl BarConfig {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Ok(Self(toml::from_str(&content)?))
    }

    pub(crate) fn empty() -> Self {
        Self(toml::Table::new())
    }

    pub(crate) fn module_config_json(&self, module_name: &str) -> String {
        match self.0.get(module_name) {
            Some(section) => serde_json::to_string(section).unwrap_or_else(|err| {
                log::error!("failed to serialize config for module `{module_name}`: {err}");
                "{}".to_string()
            }),
            None => "{}".to_string(),
        }
    }

    pub(crate) fn module_section_string_entries(&self, name: &str) -> ModuleSectionView {
        match self.0.get(name) {
            None => ModuleSectionView::Missing,
            Some(value) => {
                let Some(table) = value.as_table() else {
                    return ModuleSectionView::Missing;
                };
                if table.is_empty() {
                    return ModuleSectionView::Empty;
                }
                let entries = table
                    .iter()
                    .map(|(key, val)| {
                        let param = match val.as_str() {
                            Some(s) => ModuleParamValue::String(s.to_string()),
                            None => ModuleParamValue::NonString,
                        };
                        (key.clone(), param)
                    })
                    .collect();
                ModuleSectionView::Entries(entries)
            }
        }
    }

    pub(crate) fn separator(&self) -> String {
        self.0
            .get("separator")
            .and_then(|v| v.as_str())
            .unwrap_or(" | ")
            .to_string()
    }

    pub(crate) fn log_days(&self) -> u64 {
        self.0
            .get("log_days")
            .and_then(|v| v.as_integer())
            .filter(|&n| n >= 0)
            .map(|n| n as u64)
            .unwrap_or(DEFAULT_LOG_DAYS)
    }

    pub(crate) fn module_names(&self) -> Result<Vec<String>> {
        let modules = self
            .0
            .get("modules")
            .and_then(|v| v.as_array())
            .ok_or("config.toml must have a top-level modules = [...] list")?;
        modules
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "`modules` entries must be strings".into())
            })
            .collect()
    }

    pub(crate) fn split_module_entry(entry: &str) -> (&str, &str) {
        match entry.split_once('#') {
            Some((kind, _)) => (kind, entry),
            None => (entry, entry),
        }
    }

    #[cfg(test)]
    fn from_table(table: toml::Table) -> Self {
        Self(table)
    }
}

#[cfg(test)]
mod tests;
