use super::*;

use crate::config::test_fixtures::{self, unique_config_dir};

#[test]
fn load_bar_config_idle_when_no_program_config() {
    let dir = unique_config_dir("bar-load-no-program");
    assert!(matches!(load_bar_config(&dir), BarConfigLoad::Idle));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_bar_config_idle_when_preset_file_missing() {
    let dir = unique_config_dir("bar-load-no-preset");
    test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
    assert!(matches!(load_bar_config(&dir), BarConfigLoad::Idle));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_bar_config_idle_when_preset_invalid_toml() {
    let dir = unique_config_dir("bar-load-invalid");
    test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
    test_fixtures::write_preset(&dir, "default", "not valid {{{ toml\n");
    assert!(matches!(load_bar_config(&dir), BarConfigLoad::Idle));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_bar_config_idle_when_modules_key_missing() {
    let dir = unique_config_dir("bar-load-no-modules");
    test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
    test_fixtures::write_preset(&dir, "default", "separator = \" | \"\n");
    assert!(matches!(load_bar_config(&dir), BarConfigLoad::Idle));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_bar_config_idle_when_modules_empty() {
    let dir = unique_config_dir("bar-load-empty-modules");
    test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
    test_fixtures::write_preset(&dir, "default", "modules = []\n");
    assert!(matches!(load_bar_config(&dir), BarConfigLoad::Idle));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_bar_config_ready_when_modules_configured() {
    let dir = unique_config_dir("bar-load-ready");
    test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
    test_fixtures::write_preset(&dir, "default", "modules = [\"cpu\"]\n");
    match load_bar_config(&dir) {
        BarConfigLoad::Ready { config } => {
            assert_eq!(config.module_names().unwrap(), vec!["cpu".to_string()]);
        }
        BarConfigLoad::Idle => panic!("expected Ready"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}
