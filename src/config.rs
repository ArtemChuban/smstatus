use std::path::Path;

use crate::error::Result;

pub(crate) struct BarConfig(toml::Table);

impl BarConfig {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Ok(Self(toml::from_str(&content)?))
    }

    pub(crate) fn module_config_json(&self, module_name: &str) -> String {
        match self.0.get(module_name) {
            Some(section) => serde_json::to_string(section).unwrap_or_else(|_| "{}".to_string()),
            None => "{}".to_string(),
        }
    }

    pub(crate) fn separator(&self) -> String {
        self.0
            .get("separator")
            .and_then(|v| v.as_str())
            .unwrap_or(" | ")
            .to_string()
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

    #[cfg(test)]
    fn from_table(table: toml::Table) -> Self {
        Self(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_config_module_names_should_return_error_when_modules_key_missing() {
        let config = BarConfig::from_table(toml::Table::new());
        assert!(config.module_names().is_err());
    }

    #[test]
    fn bar_config_module_names_should_return_error_when_entry_is_not_a_string() {
        let table: toml::Table = toml::from_str("modules = [1, 2]").unwrap();
        let config = BarConfig::from_table(table);
        assert!(config.module_names().is_err());
    }

    #[test]
    fn bar_config_separator_should_default_to_pipe_when_not_configured() {
        let config = BarConfig::from_table(toml::Table::new());
        assert_eq!(config.separator(), " | ");
    }

    #[test]
    fn bar_config_module_config_json_should_return_empty_object_when_module_section_absent() {
        let config = BarConfig::from_table(toml::Table::new());
        assert_eq!(config.module_config_json("battery"), "{}");
    }
}
