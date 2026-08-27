use super::*;
use crate::config::{
    active_config_path, create_default_preset, preset_file, read_active_name, test_fixtures,
    write_active_name,
};

fn unique_config_dir(purpose: &str) -> std::path::PathBuf {
    test_fixtures::unique_config_dir(purpose)
}

fn app_with_presets(config_dir: &std::path::Path) -> App {
    App {
        config_dir: Some(config_dir.to_path_buf()),
        config_path: active_config_path(config_dir).ok(),
        active_preset: read_active_name(config_dir).ok(),
        modules_dir: Some(config_dir.join("modules")),
        extensions_dir: Some(config_dir.join("extensions")),
        ..App::default()
    }
}

#[test]
fn switch_preset_updates_modules_from_new_file() {
    install_test_log();
    let dir = unique_config_dir("switch-modules");
    create_default_preset(&dir, false).unwrap();
    std::fs::write(
        active_config_path(&dir).unwrap(),
        "modules = [\"cpu\"]\nseparator = \" | \"\n",
    )
    .unwrap();
    test_fixtures::write_preset(&dir, "work", "modules = [\"ram\"]\nseparator = \" :: \"\n");

    let mut app = app_with_presets(&dir);
    app.refresh_config();
    assert_eq!(app.modules, Some(vec!["cpu".to_string()]));

    app.mode = Mode::ChoosingPreset {
        names: vec!["default".to_string(), "work".to_string()],
        selected: 1,
        scroll_offset: 0,
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::empty()));
    assert_eq!(app.modules, Some(vec!["ram".to_string()]));
    assert_eq!(app.separator, Some(" :: ".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_preset_creates_file_and_appears_in_list() {
    install_test_log();
    let dir = unique_config_dir("save-list");
    create_default_preset(&dir, false).unwrap();
    std::fs::write(
        active_config_path(&dir).unwrap(),
        "modules = [\"cpu\"]\nseparator = \" | \"\n",
    )
    .unwrap();

    let mut app = app_with_presets(&dir);
    app.mode = Mode::NamingPreset {
        buffer: "work".to_string(),
        cursor: 4,
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::empty()));

    assert!(preset_file(&dir, "work").unwrap().is_file());
    assert!(matches!(
        app.mode,
        Mode::ChoosingPreset {
            names,
            ..
        } if names.contains(&"work".to_string())
    ));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cannot_remove_active_preset_pushes_message() {
    install_test_log();
    clear_action_log();
    let dir = unique_config_dir("remove-active");
    create_default_preset(&dir, false).unwrap();
    test_fixtures::write_preset(&dir, "work", "modules = []\n");

    let mut app = app_with_presets(&dir);
    app.mode = Mode::ChoosingPreset {
        names: vec!["default".to_string(), "work".to_string()],
        selected: 0,
        scroll_offset: 0,
    };
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::empty()));

    assert!(matches!(app.mode, Mode::ChoosingPreset { .. }));
    assert!(
        action_log()
            .iter()
            .any(|m| m.contains("cannot remove active preset")),
        "log: {:?}",
        action_log()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn switch_preset_notifies_daemon_when_not_running() {
    install_test_log();
    clear_action_log();
    let dir = unique_config_dir("switch-notify");
    create_default_preset(&dir, false).unwrap();
    test_fixtures::write_preset(&dir, "work", "modules = []\n");

    let mut app = app_with_presets(&dir);
    app.mode = Mode::ChoosingPreset {
        names: vec!["default".to_string(), "work".to_string()],
        selected: 1,
        scroll_offset: 0,
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::empty()));

    assert_eq!(
        action_log(),
        vec![
            "active preset set to `work`".to_string(),
            "smstatus is not running; config saved but bar not updated".to_string(),
        ]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refresh_config_drops_stale_choosing_preset_when_list_changes() {
    let dir = unique_config_dir("stale-choosing");
    create_default_preset(&dir, false).unwrap();

    let mut app = app_with_presets(&dir);
    app.mode = Mode::ChoosingPreset {
        names: vec!["default".to_string()],
        selected: 0,
        scroll_offset: 0,
    };

    test_fixtures::write_preset(&dir, "work", "modules = []\n");
    app.refresh_config();

    assert!(matches!(
        app.mode,
        Mode::ChoosingPreset {
            names,
            ..
        } if names.contains(&"work".to_string())
    ));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refresh_config_resyncs_active_preset_after_external_switch() {
    let dir = unique_config_dir("external-switch");
    create_default_preset(&dir, false).unwrap();
    test_fixtures::write_preset(&dir, "work", "modules = [\"ram\"]\nseparator = \" :: \"\n");
    std::fs::write(
        active_config_path(&dir).unwrap(),
        "modules = [\"cpu\"]\nseparator = \" | \"\n",
    )
    .unwrap();

    let mut app = app_with_presets(&dir);
    app.refresh_config();
    assert_eq!(app.active_preset.as_deref(), Some("default"));
    assert_eq!(app.modules, Some(vec!["cpu".to_string()]));

    write_active_name(&dir, "work").unwrap();
    app.refresh_config();

    assert_eq!(app.active_preset.as_deref(), Some("work"));
    assert_eq!(app.modules, Some(vec!["ram".to_string()]));
    assert_eq!(app.separator, Some(" :: ".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}
