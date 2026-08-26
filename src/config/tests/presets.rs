use super::*;

use crate::config::test_fixtures::{self, unique_config_dir as unique_temp_config_dir};

#[test]
fn read_active_name_round_trip() {
    let dir = unique_temp_config_dir("round-trip");
    test_fixtures::write_preset(&dir, "default", "modules = []\n");
    test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");

    assert_eq!(read_active_name(&dir).unwrap(), "default");

    write_active_name(&dir, "default").unwrap();
    let content = std::fs::read_to_string(program_config_path(&dir)).unwrap();
    assert!(content.contains("active = \"default\""));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_active_name_preserves_unrelated_keys() {
    let dir = unique_temp_config_dir("preserve-keys");
    test_fixtures::write_preset(&dir, "work", "modules = []\n");
    test_fixtures::write_program_config(
        &dir,
        "other_key = \"value\"\n[presets]\nactive = \"default\"\n",
    );
    test_fixtures::write_preset(&dir, "default", "modules = []\n");

    write_active_name(&dir, "work").unwrap();
    let content = std::fs::read_to_string(program_config_path(&dir)).unwrap();
    assert!(content.contains("other_key = \"value\""));
    assert!(content.contains("active = \"work\""));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn active_config_path_errors_when_program_config_missing() {
    let dir = unique_temp_config_dir("no-program");
    assert!(active_config_path(&dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn active_config_path_errors_when_active_missing() {
    let dir = unique_temp_config_dir("no-active");
    test_fixtures::write_program_config(&dir, "[other]\nkey = \"value\"\n");
    assert!(active_config_path(&dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn active_config_path_errors_when_preset_file_missing() {
    let dir = unique_temp_config_dir("no-preset-file");
    test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
    assert!(active_config_path(&dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn preset_name_validation_rejects_unsafe_names() {
    let dir = unique_temp_config_dir("unsafe-names");
    assert!(preset_file(&dir, "../x").is_err());
    assert!(preset_file(&dir, "").is_err());
    assert!(preset_file(&dir, "a/b").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn create_default_preset_writes_both_files() {
    let dir = unique_temp_config_dir("create-default");
    create_default_preset(&dir).unwrap();

    assert!(program_config_path(&dir).is_file());
    assert!(preset_file(&dir, DEFAULT_PRESET_NAME).unwrap().is_file());
    assert_eq!(read_active_name(&dir).unwrap(), DEFAULT_PRESET_NAME);
    assert_eq!(
        active_config_path(&dir).unwrap(),
        preset_file(&dir, DEFAULT_PRESET_NAME).unwrap()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_preset_names_returns_sorted_basenames() {
    let dir = unique_temp_config_dir("list-names");
    test_fixtures::write_preset(&dir, "work", "modules = []\n");
    test_fixtures::write_preset(&dir, "default", "modules = []\n");
    test_fixtures::write_preset(&dir, "minimal", "modules = []\n");

    assert_eq!(
        list_preset_names(&dir).unwrap(),
        vec![
            "default".to_string(),
            "minimal".to_string(),
            "work".to_string()
        ]
    );

    let _ = std::fs::remove_dir_all(&dir);
}
