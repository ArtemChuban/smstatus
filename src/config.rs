use std::path::{Path, PathBuf};

use crate::error::Result;

pub(crate) fn default_config_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or("could not determine config directory")?
        .join("smstatus"))
}

pub(crate) struct BarConfig(toml::Table);

impl BarConfig {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Ok(Self(toml::from_str(&content)?))
    }

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

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp_path, content)
        .map_err(|e| format!("cannot write {}: {e}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("cannot replace {}: {e}", path.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_path(purpose: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEST_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "smstatus-config-test-{purpose}-{}-{nanos}-{counter}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn write_separator_updates_value_while_preserving_comments_and_other_keys() {
        let path = unique_temp_path("preserve");
        std::fs::write(
            &path,
            "# a helpful comment\nother_key = \"other_value\"\nseparator = \" | \"\n",
        )
        .unwrap();

        let result = BarConfig::write_separator(&path, " :: ");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(content.contains("# a helpful comment"));
        assert!(content.contains("other_key = \"other_value\""));
        assert!(content.contains("separator = \" :: \""));
    }

    #[test]
    fn write_separator_inserts_key_when_missing() {
        let path = unique_temp_path("insert");
        std::fs::write(&path, "other_key = \"other_value\"\n").unwrap();

        let result = BarConfig::write_separator(&path, " | ");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(content.contains("other_key = \"other_value\""));
        assert!(content.contains("separator = \" | \""));
    }

    #[test]
    fn write_separator_accepts_empty_string() {
        let path = unique_temp_path("empty");
        std::fs::write(&path, "separator = \" | \"\n").unwrap();

        let result = BarConfig::write_separator(&path, "");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(content.contains("separator = \"\""));
    }

    #[test]
    fn write_separator_errors_when_file_does_not_exist() {
        let path = unique_temp_path("missing");
        let result = BarConfig::write_separator(&path, " | ");
        assert!(result.is_err());
    }

    #[test]
    fn write_separator_cleans_up_temp_file_when_rename_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = unique_temp_path("rename-fail-dir");
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "separator = \" | \"\n").unwrap();

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = BarConfig::write_separator(&path, " :: ");

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));
        if result.is_ok() {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir(&dir);
            eprintln!(
                "skipping assertions in write_separator_cleans_up_temp_file_when_rename_fails: rename unexpectedly succeeded (likely running as root)"
            );
            return;
        }

        assert!(
            !tmp_path.exists(),
            "temp file should have been cleaned up after rename failure"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn write_separator_errors_when_content_is_not_valid_toml() {
        let path = unique_temp_path("invalid");
        std::fs::write(&path, "this is not valid toml [[[").unwrap();

        let result = BarConfig::write_separator(&path, " | ");
        let _ = std::fs::remove_file(&path);

        assert!(result.is_err());
    }

    #[test]
    fn write_module_order_swaps_entries_while_preserving_comments_and_other_keys() {
        let path = unique_temp_path("module-order-preserve");
        std::fs::write(
            &path,
            "# a helpful comment\nother_key = \"other_value\"\nmodules = [\"cpu\", \"disk\" # keep disk\n, \"battery\"]\n",
        )
        .unwrap();

        let result = BarConfig::write_module_order(
            &path,
            &["disk".to_string(), "cpu".to_string(), "battery".to_string()],
        );
        assert!(result.is_ok(), "{result:?}");

        let content = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(content.contains("# a helpful comment"));
        assert!(content.contains("other_key = \"other_value\""));
        assert!(content.contains("# keep disk"));

        let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
        let array = doc["modules"].as_array().unwrap();
        let names: Vec<&str> = array.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(names, vec!["disk", "cpu", "battery"]);

        assert!(content.contains("\"disk\" # keep disk"));
        assert!(!content.contains("\"cpu\" # keep disk"));
    }

    #[test]
    fn write_module_order_errors_when_modules_key_missing() {
        let path = unique_temp_path("module-order-missing-key");
        std::fs::write(&path, "separator = \" | \"\n").unwrap();

        let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
        let _ = std::fs::remove_file(&path);

        assert!(result.is_err());
    }

    #[test]
    fn write_module_order_errors_when_modules_is_not_an_array() {
        let path = unique_temp_path("module-order-not-array");
        std::fs::write(&path, "modules = \"not-a-list\"\n").unwrap();

        let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
        let _ = std::fs::remove_file(&path);

        assert!(result.is_err());
    }

    #[test]
    fn write_module_order_errors_on_length_mismatch() {
        let path = unique_temp_path("module-order-length-mismatch");
        std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();

        let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
        let _ = std::fs::remove_file(&path);

        assert!(result.is_err());
    }

    #[test]
    fn write_module_order_errors_when_an_entry_was_renamed_on_disk() {
        let path = unique_temp_path("module-order-renamed-entry");
        std::fs::write(&path, "modules = [\"cpu\", \"ram\"]\n").unwrap();

        let result = BarConfig::write_module_order(&path, &["disk".to_string(), "cpu".to_string()]);
        let _ = std::fs::remove_file(&path);

        assert!(
            result.is_err(),
            "expected an error when the on-disk entry no longer matches the last-loaded list"
        );
    }

    #[test]
    fn write_module_order_errors_when_file_does_not_exist() {
        let path = unique_temp_path("module-order-missing-file");
        let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn write_module_order_cleans_up_temp_file_when_rename_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = unique_temp_path("module-order-rename-fail-dir");
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = BarConfig::write_module_order(&path, &["disk".to_string(), "cpu".to_string()]);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));
        if result.is_ok() {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir(&dir);
            eprintln!(
                "skipping assertions in write_module_order_cleans_up_temp_file_when_rename_fails: rename unexpectedly succeeded (likely running as root)"
            );
            return;
        }

        assert!(
            !tmp_path.exists(),
            "temp file should have been cleaned up after rename failure"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn default_config_dir_ends_with_smstatus_component() {
        let path = default_config_dir().unwrap();
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("smstatus"));
    }

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
