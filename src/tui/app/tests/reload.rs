use super::*;

#[test]
fn refresh_config_logs_once_for_a_persisting_error() {
    let path = unique_temp_path("invalid-toml");
    std::fs::write(&path, "this is not valid toml [[[").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };

    app.refresh_config();
    assert_eq!(app.separator, None);
    assert_eq!(app.action_log.len(), 1);
    assert!(
        app.action_log[0].starts_with("Failed to read config:"),
        "unexpected message: {}",
        app.action_log[0]
    );
    assert!(app.last_separator_error.is_some());

    app.refresh_config();
    let _ = std::fs::remove_file(&path);
    assert_eq!(app.action_log.len(), 1);
}

#[test]
fn refresh_config_recovers_and_clears_dedup_state_once_file_is_fixed() {
    let path = unique_temp_path("recovers");
    std::fs::write(&path, "this is not valid toml [[[").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };

    app.refresh_config();
    assert_eq!(app.action_log.len(), 1);
    assert!(app.last_separator_error.is_some());

    std::fs::write(&path, "separator = \" :: \"\nmodules = [\"cpu\"]\n").unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.separator, Some(" :: ".to_string()));
    assert!(app.last_separator_error.is_none());
    assert_eq!(app.modules, Some(vec!["cpu".to_string()]));
    assert_eq!(app.action_log.len(), 1);
}

#[test]
fn refresh_config_missing_modules_key_does_not_clobber_working_separator() {
    let path = unique_temp_path("missing-modules");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };

    app.refresh_config();
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.separator, Some(" | ".to_string()));
    assert_eq!(app.modules, None);
    assert_eq!(app.action_log.len(), 1);
    assert!(
        app.action_log[0].starts_with("Failed to read modules:"),
        "unexpected message: {}",
        app.action_log[0]
    );
    assert!(app.last_modules_error.is_some());
    assert!(app.last_separator_error.is_none());
}

#[test]
fn refresh_config_logs_modules_error_once_for_a_persisting_error() {
    let path = unique_temp_path("modules-persisting-error");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };

    app.refresh_config();
    assert_eq!(app.action_log.len(), 1);
    assert!(app.last_modules_error.is_some());

    app.refresh_config();
    assert_eq!(app.action_log.len(), 1);

    std::fs::write(
        &path,
        "separator = \" | \"\nmodules = [\"cpu\", \"disk#root\"]\n",
    )
    .unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "disk#root".to_string()])
    );
    assert!(app.last_modules_error.is_none());
    assert_eq!(app.action_log.len(), 1);
}

#[test]
fn refresh_config_does_not_reflap_a_persisting_modules_error_across_a_whole_file_failure() {
    let path = unique_temp_path("modules-error-survives-whole-file-failure");
    let working_separator_malformed_modules = "separator = \" | \"\nmodules = \"not-a-list\"\n";
    std::fs::write(&path, working_separator_malformed_modules).unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };

    app.refresh_config();
    assert_eq!(app.action_log.len(), 1);
    assert!(
        app.action_log[0].starts_with("Failed to read modules:"),
        "unexpected message: {}",
        app.action_log[0]
    );
    assert!(app.last_modules_error.is_some());
    let modules_error = app.last_modules_error.clone();

    std::fs::write(&path, "this is not valid toml [[[").unwrap();
    app.refresh_config();
    assert_eq!(app.action_log.len(), 2);
    assert!(
        app.action_log[1].starts_with("Failed to read config:"),
        "unexpected message: {}",
        app.action_log[1]
    );
    assert_eq!(app.last_modules_error, modules_error);

    std::fs::write(&path, working_separator_malformed_modules).unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);
    assert_eq!(app.action_log.len(), 2);
}

#[test]
fn refresh_config_reload_clamps_selected_index_and_keeps_it_visible() {
    let path = unique_temp_path("reload-clamp-viewport");
    std::fs::write(
        &path,
        "separator = \" | \"\nmodules = [\"m0\", \"m1\", \"m2\", \"m3\", \"m4\"]\n",
    )
    .unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        selected_index: Some(4),
        module_scroll_offset: 0,
        modules_viewport_height: 2,
        ..App::default()
    };

    app.refresh_config();
    assert_eq!(app.selected_index, Some(4));
    assert_eq!(app.module_scroll_offset, 3);

    std::fs::write(&path, "separator = \" | \"\nmodules = [\"m0\"]\n").unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);
    assert_eq!(app.selected_index, Some(0));
    assert_eq!(app.module_scroll_offset, 0);
}

#[test]
fn refresh_config_selected_index_resets_to_none_when_list_becomes_empty_then_to_zero_when_repopulated()
 {
    let path = unique_temp_path("selected-index-empty-cycle");
    std::fs::write(
        &path,
        "separator = \" | \"\nmodules = [\"cpu\", \"disk\"]\n",
    )
    .unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };
    app.refresh_config();
    assert_eq!(app.selected_index, Some(0));

    std::fs::write(&path, "separator = \" | \"\nmodules = []\n").unwrap();
    app.refresh_config();
    assert_eq!(app.selected_index, None);

    std::fs::write(&path, "separator = \" | \"\nmodules = [\"cpu\"]\n").unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);
    assert_eq!(app.selected_index, Some(0));
}

#[test]
fn refresh_config_resets_selected_index_to_none_when_whole_file_load_fails() {
    let path = unique_temp_path("selected-index-whole-file-failure");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };
    app.refresh_config();
    assert_eq!(app.selected_index, Some(0));

    std::fs::write(&path, "this is not valid toml [[[").unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);
    assert_eq!(app.modules, None);
    assert_eq!(app.selected_index, None);
}

#[test]
fn refresh_config_drops_stale_confirming_remove_mode_when_armed_entry_disappears() {
    let path = unique_temp_path("confirming-remove-armed-entry-disappears");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\", \"battery\"]\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        modules: Some(vec![
            "cpu".to_string(),
            "disk".to_string(),
            "battery".to_string(),
        ]),
        selected_index: Some(2),
        mode: Mode::ConfirmingRemove {
            index: 2,
            name: "battery".to_string(),
        },
        ..App::default()
    };

    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert!(app.action_log.is_empty());
}

#[test]
fn refresh_config_keeps_confirming_remove_mode_when_armed_entry_still_matches() {
    let path = unique_temp_path("confirming-remove-armed-entry-still-valid");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\", \"battery\"]\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        modules: Some(vec![
            "cpu".to_string(),
            "disk".to_string(),
            "battery".to_string(),
        ]),
        selected_index: Some(1),
        mode: Mode::ConfirmingRemove {
            index: 1,
            name: "disk".to_string(),
        },
        ..App::default()
    };

    std::fs::write(
        &path,
        "separator = \" | \"\nmodules = [\"cpu\", \"disk\", \"battery\"]\n",
    )
    .unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        app.mode,
        Mode::ConfirmingRemove {
            index: 1,
            name: "disk".to_string(),
        }
    );
}

#[test]
fn refresh_config_resets_selected_index_to_none_when_modules_key_becomes_missing() {
    let path = unique_temp_path("selected-index-modules-key-missing");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        ..App::default()
    };
    app.refresh_config();
    assert_eq!(app.selected_index, Some(0));

    std::fs::write(&path, "separator = \" | \"\n").unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);
    assert_eq!(app.modules, None);
    assert_eq!(app.selected_index, None);
}

#[test]
fn refresh_config_drops_key_absent_edit_when_key_appears_on_disk() {
    let path = unique_temp_path("params-key-absent-appears");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nother = \"y\"\n").unwrap();
    let config = BarConfig::load(&path).unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(path.clone()),
        config_cache: Some(config),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![(
                "other".to_string(),
                ModuleParamValue::String("y".to_string()),
            )],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        mode: Mode::EditingParamValue {
            section: "cpu".to_string(),
            key: "format".to_string(),
            buffer: "x".to_string(),
            cursor: 1,
            expect: ParamWriteExpect::KeyAbsent,
        },
        ..App::default()
    };

    std::fs::write(
        &path,
        "modules = [\"cpu\"]\n\n[cpu]\nother = \"y\"\nformat = \"disk\"\n",
    )
    .unwrap();
    app.refresh_config();
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert!(app.action_log.is_empty());
}
