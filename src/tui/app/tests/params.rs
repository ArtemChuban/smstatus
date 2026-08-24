use super::*;

#[test]
fn enter_and_right_focus_params_when_module_selected() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Params);
    app.panel_focus = PanelFocus::Modules;
    app.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Params);
}

#[test]
fn a_enters_adding_param_key_when_params_focused() {
    let path = unique_temp_path("params-add-key");
    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        separator: Some(" | ".to_string()),
        config_path: Some(path),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Missing {
                section: "cpu".to_string(),
            },
            entries: vec![],
            selected_index: None,
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(matches!(
        app.mode,
        Mode::AddingParamKey {
            section,
            ..
        } if section == "cpu"
    ));
    assert_eq!(app.panel_focus, PanelFocus::Params);
    let _ = std::fs::remove_file(app.config_path.as_ref().unwrap());
}

#[test]
fn e_and_enter_edit_selected_param_value() {
    let path = unique_temp_path("params-edit-value");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nformat = \"old\"\n").unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(path.clone()),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![(
                "format".to_string(),
                ModuleParamValue::String("old".to_string()),
            )],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('e'), KeyModifiers::NONE));
    match &app.mode {
        Mode::EditingParamValue {
            key,
            buffer,
            expect,
            ..
        } => {
            assert_eq!(key, "format");
            assert_eq!(buffer, "old");
            assert_eq!(expect, &ParamWriteExpect::ExistingString("old".to_string()));
        }
        other => panic!("expected EditingParamValue, got {other:?}"),
    }
    app.mode = Mode::Normal;
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(app.mode, Mode::EditingParamValue { .. }));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn e_d_r_noop_on_missing_params_but_a_works() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(unique_temp_path("params-missing-noop")),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Missing {
                section: "cpu".to_string(),
            },
            entries: vec![],
            selected_index: None,
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('e'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    app.handle_key(key(KeyCode::Char('r'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    app.handle_key(key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(matches!(app.mode, Mode::AddingParamKey { .. }));
}

#[test]
fn double_d_removes_param_and_esc_cancel_edit_stays_params() {
    install_test_log();
    let path = unique_temp_path("params-remove-edit");
    std::fs::write(
        &path,
        "modules = [\"cpu\"]\n\n[cpu]\nformat = \"x\"\nother = \"y\"\n",
    )
    .unwrap();
    let config = BarConfig::load(&path).unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(path.clone()),
        config_cache: Some(config),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![
                (
                    "format".to_string(),
                    ModuleParamValue::String("x".to_string()),
                ),
                (
                    "other".to_string(),
                    ModuleParamValue::String("y".to_string()),
                ),
            ],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('e'), KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.panel_focus, PanelFocus::Params);
    assert!(action_log().is_empty());
    let before = std::fs::read_to_string(&path).unwrap();

    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
    assert!(matches!(app.mode, Mode::ConfirmingRemoveParam { .. }));
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().iter().any(|m| m == "Removed format"));
    let after = std::fs::read_to_string(&path).unwrap();
    assert_ne!(before, after);
    assert!(!after.contains("format"));
    assert!(after.contains("other"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn add_param_flow_writes_and_esc_on_value_returns_to_key() {
    install_test_log();
    let path = unique_temp_path("params-add-flow");
    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();
    let config = BarConfig::load(&path).unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(path.clone()),
        config_cache: Some(config),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Missing {
                section: "cpu".to_string(),
            },
            entries: vec![],
            selected_index: None,
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('a'), KeyModifiers::NONE));
    for c in "format".chars() {
        app.handle_key(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        app.mode,
        Mode::EditingParamValue {
            expect: ParamWriteExpect::KeyAbsent,
            ..
        }
    ));
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    match &app.mode {
        Mode::AddingParamKey { buffer, .. } => assert_eq!(buffer, "format"),
        other => panic!("expected AddingParamKey, got {other:?}"),
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    for c in "ok".chars() {
        app.handle_key(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().iter().any(|m| m == "Added format"));
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("[cpu]"));
    assert!(content.contains("format"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn rename_param_via_r() {
    install_test_log();
    let path = unique_temp_path("params-rename");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nold = \"v\"\n").unwrap();
    let config = BarConfig::load(&path).unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(path.clone()),
        config_cache: Some(config),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![("old".to_string(), ModuleParamValue::String("v".to_string()))],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('r'), KeyModifiers::NONE));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    }
    for c in "new".chars() {
        app.handle_key(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().iter().any(|m| m == "Renamed old → new"));
    let sel = &app.module_params.as_ref().unwrap().entries
        [app.module_params.as_ref().unwrap().selected_index.unwrap()];
    assert_eq!(sel.0, "new");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("new"));
    assert!(!content.contains("old ="));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn q_quits_even_with_params_focus() {
    let mut app = App {
        panel_focus: PanelFocus::Params,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(app.should_quit);
}

#[test]
fn params_up_down_move_selection() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        modules_viewport_height: 10,
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![
                ("a".to_string(), ModuleParamValue::String("1".to_string())),
                ("b".to_string(), ModuleParamValue::String("2".to_string())),
                ("c".to_string(), ModuleParamValue::String("3".to_string())),
            ],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.module_params.as_ref().unwrap().selected_index, Some(1));
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.module_params.as_ref().unwrap().selected_index, Some(0));
}

#[test]
fn add_param_keeps_previous_selection_when_a_row_was_already_selected() {
    install_test_log();
    let path = unique_temp_path("params-add-keep-selection");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nkeep = \"me\"\n").unwrap();
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
                "keep".to_string(),
                ModuleParamValue::String("me".to_string()),
            )],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('a'), KeyModifiers::NONE));
    for c in "newbie".chars() {
        app.handle_key(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    for c in "val".chars() {
        app.handle_key(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().iter().any(|m| m == "Added newbie"));
    let params = app.module_params.as_ref().unwrap();
    let sel = params.selected_index.unwrap();
    assert_eq!(params.entries[sel].0, "keep");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn empty_value_commit_succeeds() {
    install_test_log();
    let path = unique_temp_path("params-empty-value");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nformat = \"old\"\n").unwrap();
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
                "format".to_string(),
                ModuleParamValue::String("old".to_string()),
            )],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('e'), KeyModifiers::NONE));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().iter().any(|m| m == "Updated format"));
    let content = std::fs::read_to_string(&path).unwrap();
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(doc["cpu"]["format"].as_str(), Some(""));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn invalid_charset_on_add_and_rename_refuses_and_logs() {
    install_test_log();
    let path = unique_temp_path("params-invalid-charset");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nold = \"v\"\n").unwrap();
    let config = BarConfig::load(&path).unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(path.clone()),
        config_cache: Some(config),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![("old".to_string(), ModuleParamValue::String("v".to_string()))],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };

    app.handle_key(key(KeyCode::Char('a'), KeyModifiers::NONE));
    for c in "bad.key".chars() {
        app.handle_key(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(app.mode, Mode::AddingParamKey { .. }));
    assert!(action_log().iter().any(|m| m.contains("Invalid param key")));

    app.mode = Mode::Normal;
    clear_action_log();
    app.handle_key(key(KeyCode::Char('r'), KeyModifiers::NONE));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    }
    for c in "also.bad".chars() {
        app.handle_key(key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(app.mode, Mode::RenamingParamKey { .. }));
    assert!(action_log().iter().any(|m| m.contains("Invalid param key")));
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(before.contains("old ="));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn non_string_edit_begins_with_empty_buffer() {
    let path = unique_temp_path("params-non-string-edit");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\ncount = 42\n").unwrap();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: Some(path.clone()),
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![("count".to_string(), ModuleParamValue::NonString)],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('e'), KeyModifiers::NONE));
    match &app.mode {
        Mode::EditingParamValue {
            key,
            buffer,
            cursor,
            expect,
            ..
        } => {
            assert_eq!(key, "count");
            assert_eq!(buffer, "");
            assert_eq!(*cursor, 0);
            assert_eq!(expect, &ParamWriteExpect::ExistingNonString);
        }
        other => panic!("expected EditingParamValue, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn begin_remove_param_without_config_path_pushes_message_and_stays_normal() {
    install_test_log();
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        config_path: None,
        module_params: Some(ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![(
                "format".to_string(),
                ModuleParamValue::String("x".to_string()),
            )],
            selected_index: Some(0),
            scroll_offset: 0,
        }),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        action_log(),
        vec!["cannot remove param: config path unknown"]
    );
}
