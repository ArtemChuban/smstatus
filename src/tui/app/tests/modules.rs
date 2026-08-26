use super::*;

#[test]
fn up_key_in_normal_mode_selects_previous_module() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.selected_index, Some(0));
}

#[test]
fn down_key_in_normal_mode_selects_next_module() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(0),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.selected_index, Some(1));
}

#[test]
fn select_previous_module_is_noop_at_top() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(0),
        ..App::default()
    };
    app.select_previous_module();
    assert_eq!(app.selected_index, Some(0));
}

#[test]
fn select_next_module_is_noop_at_bottom() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        ..App::default()
    };
    app.select_next_module();
    assert_eq!(app.selected_index, Some(1));
}

#[test]
fn select_next_module_is_noop_when_modules_is_none() {
    let mut app = App {
        modules: None,
        selected_index: None,
        ..App::default()
    };
    app.select_next_module();
    assert_eq!(app.selected_index, None);
}

#[test]
fn select_next_module_pulls_scroll_offset_down_when_selection_moves_below_viewport() {
    let mut app = App {
        modules: Some(vec![
            "m0".to_string(),
            "m1".to_string(),
            "m2".to_string(),
            "m3".to_string(),
        ]),
        selected_index: Some(1),
        module_scroll_offset: 0,
        modules_viewport_height: 2,
        ..App::default()
    };
    app.select_next_module();
    assert_eq!(app.selected_index, Some(2));
    assert_eq!(app.module_scroll_offset, 1);
}

#[test]
fn select_previous_module_pulls_scroll_offset_up_when_selection_moves_above_viewport() {
    let mut app = App {
        modules: Some(vec![
            "m0".to_string(),
            "m1".to_string(),
            "m2".to_string(),
            "m3".to_string(),
        ]),
        selected_index: Some(2),
        module_scroll_offset: 2,
        modules_viewport_height: 2,
        ..App::default()
    };
    app.select_previous_module();
    assert_eq!(app.selected_index, Some(1));
    assert_eq!(app.module_scroll_offset, 1);
}

#[test]
fn move_module_up_swaps_with_previous_and_persists() {
    install_test_log();
    let path = unique_temp_path("move-up");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\", \"battery\"]\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        modules: Some(vec![
            "cpu".to_string(),
            "disk".to_string(),
            "battery".to_string(),
        ]),
        selected_index: Some(1),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        app.modules,
        Some(vec![
            "disk".to_string(),
            "cpu".to_string(),
            "battery".to_string()
        ])
    );
    assert_eq!(app.selected_index, Some(0));
    assert_eq!(action_log(), vec!["Moved disk up"]);
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    let names: Vec<&str> = doc["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["disk", "cpu", "battery"]);
}

#[test]
fn move_module_down_swaps_with_next_and_persists() {
    install_test_log();
    let path = unique_temp_path("move-down");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\", \"battery\"]\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        modules: Some(vec![
            "cpu".to_string(),
            "disk".to_string(),
            "battery".to_string(),
        ]),
        selected_index: Some(1),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Down, KeyModifiers::CONTROL));
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        app.modules,
        Some(vec![
            "cpu".to_string(),
            "battery".to_string(),
            "disk".to_string()
        ])
    );
    assert_eq!(app.selected_index, Some(2));
    assert_eq!(action_log(), vec!["Moved disk down"]);
}

#[test]
fn move_module_up_at_top_is_noop_no_write_no_log() {
    install_test_log();
    let path = unique_temp_path("move-up-top-noop");
    let mut app = App {
        config_path: Some(path),
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(0),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
    assert_eq!(app.selected_index, Some(0));
    assert!(action_log().is_empty());
}

#[test]
fn move_module_down_at_bottom_is_noop_no_write_no_log() {
    install_test_log();
    let path = unique_temp_path("move-down-bottom-noop");
    let mut app = App {
        config_path: Some(path),
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Down, KeyModifiers::CONTROL));
    assert_eq!(app.selected_index, Some(1));
    assert!(action_log().is_empty());
}

#[test]
fn move_module_without_config_path_logs_failure() {
    install_test_log();
    let mut app = App {
        config_path: None,
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
    assert_eq!(
        action_log(),
        vec!["cannot reorder modules: config path unknown"]
    );
    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "disk".to_string()])
    );
    assert_eq!(app.selected_index, Some(1));
}

#[test]
fn move_module_when_write_fails_logs_failure_and_leaves_state_unchanged() {
    install_test_log();
    let path = unique_temp_path("move-write-fails");
    let mut app = App {
        config_path: Some(path),
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
    assert_eq!(action_log().len(), 1);
    assert!(
        action_log()[0].starts_with("Failed to reorder modules:"),
        "unexpected message: {}",
        action_log()[0]
    );
    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "disk".to_string()])
    );
    assert_eq!(app.selected_index, Some(1));
}

#[test]
fn move_module_is_noop_when_modules_is_none() {
    install_test_log();
    let mut app = App {
        modules: None,
        selected_index: None,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
    assert!(action_log().is_empty());
}

#[test]
fn begin_add_module_without_modules_dir_pushes_message_and_stays_normal() {
    install_test_log();
    let mut app = App {
        modules_dir: None,
        ..App::default()
    };
    app.begin_add_module();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        action_log(),
        vec!["cannot add module: modules directory unknown"]
    );
}

#[test]
fn begin_add_module_when_directory_missing_opens_empty_picker_without_message() {
    install_test_log();
    let dir = unique_temp_path("modules-dir-missing");
    let mut app = App {
        modules_dir: Some(dir),
        ..App::default()
    };
    app.begin_add_module();
    assert_eq!(
        app.mode,
        Mode::AddingModule {
            available: vec![],
            selected: 0,
            scroll_offset: 0,
        }
    );
    assert!(action_log().is_empty());
}

#[test]
fn begin_add_module_lists_available_wasm_kinds_sorted_and_enters_adding_mode() {
    install_test_log();
    let dir = unique_temp_path("modules-dir-with-kinds");
    std::fs::create_dir(&dir).unwrap();
    for name in ["ram", "cpu"] {
        let pkg = dir.join(name);
        std::fs::create_dir(&pkg).unwrap();
        std::fs::write(pkg.join("manifest.toml"), b"").unwrap();
        std::fs::write(pkg.join("module.wasm"), b"").unwrap();
    }
    let mut app = App {
        modules_dir: Some(dir.clone()),
        ..App::default()
    };
    app.begin_add_module();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        app.mode,
        Mode::AddingModule {
            available: vec!["cpu".to_string(), "ram".to_string()],
            selected: 0,
            scroll_offset: 0,
        }
    );
    assert!(action_log().is_empty());
}

#[test]
fn adding_module_down_key_clamps_at_last_available_entry() {
    let mut app = App {
        mode: Mode::AddingModule {
            available: vec!["a".to_string(), "b".to_string()],
            selected: 0,
            scroll_offset: 0,
        },
        overlay_viewport_height: 2,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::AddingModule {
            available: vec!["a".to_string(), "b".to_string()],
            selected: 1,
            scroll_offset: 0,
        }
    );
}

#[test]
fn adding_module_up_key_clamps_at_zero() {
    let mut app = App {
        mode: Mode::AddingModule {
            available: vec!["a".to_string(), "b".to_string()],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::AddingModule {
            available: vec!["a".to_string(), "b".to_string()],
            selected: 0,
            scroll_offset: 0,
        }
    );
}

#[test]
fn adding_module_down_key_scrolls_using_overlay_viewport_not_column() {
    let mut app = App {
        mode: Mode::AddingModule {
            available: vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string(),
            ],
            selected: 0,
            scroll_offset: 0,
        },
        modules_viewport_height: 8,
        overlay_viewport_height: 2,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::AddingModule {
            available: vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string(),
            ],
            selected: 2,
            scroll_offset: 1,
        }
    );
}

#[test]
fn adding_module_enter_on_empty_list_is_a_noop() {
    let mut app = App {
        mode: Mode::AddingModule {
            available: vec![],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::AddingModule {
            available: vec![],
            selected: 0,
            scroll_offset: 0,
        }
    );
}

#[test]
fn esc_cancels_adding_module_without_log_message() {
    install_test_log();
    let mut app = App {
        mode: Mode::AddingModule {
            available: vec!["a".to_string()],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().is_empty());
}

#[test]
fn ctrl_c_hard_quits_while_adding_module() {
    let mut app = App {
        mode: Mode::AddingModule {
            available: vec![],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn enter_on_adding_module_transitions_to_naming_module_instance_with_chosen_kind() {
    let mut app = App {
        mode: Mode::AddingModule {
            available: vec!["cpu".to_string(), "disk".to_string()],
            selected: 1,
            scroll_offset: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: String::new(),
            cursor: 0,
        }
    );
}

#[test]
fn naming_module_instance_left_right_backspace_edit_buffer_like_separator_editing() {
    let mut app = App {
        mode: Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "ac".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "abc".to_string(),
            cursor: 2,
        }
    );
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "abc".to_string(),
            cursor: 0,
        }
    );
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "abc".to_string(),
            cursor: 0,
        }
    );
    app.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "abc".to_string(),
            cursor: 1,
        }
    );
    app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "bc".to_string(),
            cursor: 0,
        }
    );
}

#[test]
fn commit_add_module_with_empty_buffer_inserts_bare_kind() {
    install_test_log();
    let path = unique_temp_path("add-empty-buffer");
    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();
    let mut app = App {
        mode: Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: String::new(),
            cursor: 0,
        },
        config_path: Some(path.clone()),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "disk".to_string()])
    );
    assert_eq!(app.selected_index, Some(1));
    assert_eq!(action_log(), vec!["Added disk"]);
}

#[test]
fn commit_add_module_with_buffer_inserts_kind_hash_instance() {
    install_test_log();
    let path = unique_temp_path("add-with-buffer");
    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();
    let mut app = App {
        mode: Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "root".to_string(),
            cursor: 4,
        },
        config_path: Some(path.clone()),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "disk#root".to_string()])
    );
    assert_eq!(app.selected_index, Some(1));
    assert_eq!(action_log(), vec!["Added disk#root"]);
}

#[test]
fn commit_add_module_failure_logs_error_and_returns_to_normal_mode() {
    install_test_log();
    let path = unique_temp_path("add-write-fails");
    let mut app = App {
        mode: Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: String::new(),
            cursor: 0,
        },
        config_path: Some(path),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(action_log().len(), 1);
    assert!(
        action_log()[0].starts_with("Failed to add module:"),
        "unexpected message: {}",
        action_log()[0]
    );
    assert_eq!(app.modules, Some(vec!["cpu".to_string()]));
}

#[test]
fn esc_cancels_naming_module_instance_without_log_message() {
    install_test_log();
    let mut app = App {
        mode: Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "abc".to_string(),
            cursor: 3,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().is_empty());
}

#[test]
fn begin_remove_module_with_no_selection_pushes_message_and_stays_normal() {
    install_test_log();
    let mut app = App {
        modules: None,
        selected_index: None,
        ..App::default()
    };
    app.begin_remove_module();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(action_log(), vec!["no module selected to remove"]);
}

#[test]
fn begin_remove_module_arms_confirming_remove_without_log_message() {
    install_test_log();
    let mut app = App {
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        ..App::default()
    };
    app.begin_remove_module();
    assert_eq!(
        app.mode,
        Mode::ConfirmingRemove {
            index: 1,
            name: "disk".to_string(),
        }
    );
    assert!(action_log().is_empty());
}

#[test]
fn confirming_remove_any_other_key_cancels_silently() {
    install_test_log();
    let mut app = App {
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        mode: Mode::ConfirmingRemove {
            index: 1,
            name: "disk".to_string(),
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(action_log().is_empty());
    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "disk".to_string()])
    );
}

#[test]
fn confirming_remove_second_d_removes_and_keeps_selection_on_shifted_entry() {
    install_test_log();
    let path = unique_temp_path("remove-shift");
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
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "battery".to_string()])
    );
    assert_eq!(app.selected_index, Some(1));
    assert_eq!(action_log(), vec!["Removed disk"]);
}

#[test]
fn confirming_remove_on_the_only_remaining_entry_clears_selection() {
    install_test_log();
    let path = unique_temp_path("remove-only-entry");
    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();
    let mut app = App {
        config_path: Some(path.clone()),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        mode: Mode::ConfirmingRemove {
            index: 0,
            name: "cpu".to_string(),
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::NONE));
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.modules, Some(vec![]));
    assert_eq!(app.selected_index, None);
    assert_eq!(action_log(), vec!["Removed cpu"]);
}

#[test]
fn confirming_remove_failure_pushes_message_and_leaves_modules_unchanged() {
    install_test_log();
    let path = unique_temp_path("remove-write-fails");
    let mut app = App {
        config_path: Some(path),
        modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
        selected_index: Some(1),
        mode: Mode::ConfirmingRemove {
            index: 1,
            name: "disk".to_string(),
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::NONE));

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(action_log().len(), 1);
    assert!(
        action_log()[0].starts_with("Failed to remove module:"),
        "unexpected message: {}",
        action_log()[0]
    );
    assert_eq!(
        app.modules,
        Some(vec!["cpu".to_string(), "disk".to_string()])
    );
    assert_eq!(app.selected_index, Some(1));
}

#[test]
fn enter_ignored_when_no_module_selected() {
    let mut app = App {
        modules: Some(vec![]),
        selected_index: None,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Modules);
}

#[test]
fn esc_and_left_return_focus_to_modules() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Modules);
    app.panel_focus = PanelFocus::Params;
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Modules);
}

#[test]
fn x_opens_extensions_overlay_and_esc_returns_to_normal() {
    install_test_log();
    let dir = unique_temp_path("extensions-browse");
    std::fs::create_dir(&dir).unwrap();
    let mut app = App {
        extensions_dir: Some(dir.clone()),
        panel_focus: PanelFocus::Modules,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::BrowsingExtensions {
            selected: 0,
            scroll_offset: 0,
        }
    );
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    let _ = std::fs::remove_dir_all(&dir);
}
