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
            Some(section) => serde_json::to_string(section).unwrap_or_else(|err| {
                eprintln!("failed to serialize config for module `{module_name}`: {err}");
                "{}".to_string()
            }),
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

    #[test]
    fn split_module_entry_should_return_entry_twice_when_no_hash_present() {
        assert_eq!(BarConfig::split_module_entry("disk"), ("disk", "disk"));
    }

    #[test]
    fn split_module_entry_should_split_kind_from_full_instance_name_when_hash_present() {
        assert_eq!(
            BarConfig::split_module_entry("disk#root"),
            ("disk", "disk#root")
        );
    }

    #[test]
    fn split_module_entry_should_split_on_first_hash_only() {
        assert_eq!(
            BarConfig::split_module_entry("disk#root#extra"),
            ("disk", "disk#root#extra")
        );
    }

    #[test]
    fn bar_config_module_config_json_should_return_section_matching_full_instance_name() {
        let table: toml::Table = toml::from_str(
            r#"
            ["disk#root"]
            path = "/"

            ["disk#home"]
            path = "/home"
            "#,
        )
        .unwrap();
        let config = BarConfig::from_table(table);

        assert_eq!(config.module_config_json("disk#root"), r#"{"path":"/"}"#);
        assert_eq!(
            config.module_config_json("disk#home"),
            r#"{"path":"/home"}"#
        );
    }

    #[test]
    fn bar_config_two_instances_of_same_kind_should_get_independent_configs() {
        let table: toml::Table = toml::from_str(
            r#"
            modules = ["disk#root", "disk#home"]

            ["disk#root"]
            path = "/"

            ["disk#home"]
            path = "/home"
            "#,
        )
        .unwrap();
        let config = BarConfig::from_table(table);
        let names = config.module_names().unwrap();
        assert_eq!(
            names,
            vec!["disk#root".to_string(), "disk#home".to_string()]
        );

        let configs: Vec<(String, String, String)> = names
            .iter()
            .map(|entry| {
                let (kind, instance) = BarConfig::split_module_entry(entry);
                (
                    kind.to_string(),
                    instance.to_string(),
                    config.module_config_json(instance),
                )
            })
            .collect();

        assert_eq!(configs[0].0, "disk");
        assert_eq!(configs[1].0, "disk");
        assert_ne!(configs[0].2, configs[1].2);
        assert_eq!(configs[0].2, r#"{"path":"/"}"#);
        assert_eq!(configs[1].2, r#"{"path":"/home"}"#);
    }
}
