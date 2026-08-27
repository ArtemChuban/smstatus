use super::*;

use crate::config::test_fixtures::{self, unique_config_dir};

#[test]
fn init_config_layout_creates_expected_directories() {
    let dir = unique_config_dir("config-init-layout");
    init_config_layout(&dir, false).unwrap();

    assert!(dir.join("modules").is_dir());
    assert!(dir.join("extensions").is_dir());
    assert!(program_config_path(&dir).is_file());
    assert!(preset_file(&dir, DEFAULT_PRESET_NAME).unwrap().is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_config_layout_active_pointer_resolves() {
    let dir = unique_config_dir("config-init-active");
    init_config_layout(&dir, false).unwrap();

    assert_eq!(read_active_name(&dir).unwrap(), DEFAULT_PRESET_NAME);
    assert_eq!(
        active_config_path(&dir).unwrap(),
        preset_file(&dir, DEFAULT_PRESET_NAME).unwrap()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_config_layout_force_refreshes_skeleton() {
    let dir = unique_config_dir("config-init-force");
    init_config_layout(&dir, false).unwrap();
    test_fixtures::write_program_config(&dir, "broken\n");
    test_fixtures::write_preset(&dir, "default", "modules = [\"cpu\"]\n");

    init_config_layout(&dir, true).unwrap();

    let program = std::fs::read_to_string(program_config_path(&dir)).unwrap();
    assert!(program.contains("[presets]"));
    assert!(program.contains("active = \"default\""));

    let preset = std::fs::read_to_string(preset_file(&dir, "default").unwrap()).unwrap();
    assert!(preset.contains("modules = []"));
    assert!(preset.contains("# bar layout preset"));
    assert!(active_config_path(&dir).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}
