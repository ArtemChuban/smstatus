use super::*;

#[test]
fn q_key_requests_quit() {
    let mut app = App::default();
    app.handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(app.should_quit);
}

#[test]
fn ctrl_c_requests_quit() {
    let mut app = App::default();
    app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn ctrl_d_requests_quit() {
    let mut app = App::default();
    app.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn unrelated_key_does_not_request_quit() {
    let mut app = App::default();
    app.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(!app.should_quit);
}

#[test]
fn plain_c_without_control_does_not_request_quit() {
    let mut app = App::default();
    app.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));
    assert!(!app.should_quit);
}

#[test]
fn push_action_message_caps_at_capacity_keeping_most_recent() {
    let mut app = App::default();
    app.push_action_message("one".to_string());
    app.push_action_message("two".to_string());
    app.push_action_message("three".to_string());
    app.push_action_message("four".to_string());
    assert_eq!(app.action_log, vec!["two", "three", "four"]);
}

#[test]
fn begin_edit_separator_prefills_buffer_with_current_separator() {
    let mut app = App {
        config_path: Some(unique_temp_path("prefill")),
        separator: Some(" | ".to_string()),
        ..App::default()
    };
    app.begin_edit_separator();
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: " | ".to_string(),
            cursor: 3,
        }
    );
}

#[test]
fn begin_edit_separator_without_config_path_pushes_message_and_stays_normal() {
    let mut app = App {
        config_path: None,
        ..App::default()
    };
    app.begin_edit_separator();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.action_log,
        vec!["cannot edit separator: config path unknown"]
    );
}

#[test]
fn typing_plain_char_while_editing_appends_to_buffer() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "a".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "ab".to_string(),
            cursor: 2,
        }
    );
}

#[test]
fn ctrl_modified_char_while_editing_is_ignored() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "a".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "a".to_string(),
            cursor: 1,
        }
    );
}

#[test]
fn backspace_removes_last_char_while_editing() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "ab".to_string(),
            cursor: 2,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "a".to_string(),
            cursor: 1,
        }
    );
}

#[test]
fn backspace_on_empty_buffer_while_editing_is_noop() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: String::new(),
            cursor: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: String::new(),
            cursor: 0,
        }
    );
}

#[test]
fn backspace_at_mid_buffer_cursor_removes_char_before_cursor_only() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "bc".to_string(),
            cursor: 0,
        }
    );
}

#[test]
fn typing_char_at_mid_buffer_cursor_inserts_at_cursor_not_at_end() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "ac".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 2,
        }
    );
}

#[test]
fn left_key_moves_cursor_left_and_saturates_at_zero() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 0,
        }
    );
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 0,
        }
    );
}

#[test]
fn right_key_moves_cursor_right_and_saturates_at_buffer_length() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 2,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 3,
        }
    );
    app.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 3,
        }
    );
}

#[test]
fn multi_byte_utf8_buffer_uses_char_indices_not_byte_indices() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "é".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "éx".to_string(),
            cursor: 2,
        }
    );
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "éx".to_string(),
            cursor: 1,
        }
    );
    app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "x".to_string(),
            cursor: 0,
        }
    );
}

#[test]
fn q_while_editing_appends_literal_char_rather_than_quitting() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: String::new(),
            cursor: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(!app.should_quit);
    assert_eq!(
        app.mode,
        Mode::EditingSeparator {
            buffer: "q".to_string(),
            cursor: 1,
        }
    );
}

#[test]
fn ctrl_c_while_editing_still_quits() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 3,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn esc_while_editing_returns_to_normal_without_logging() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: "abc".to_string(),
            cursor: 3,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.action_log.is_empty());
}

#[test]
fn enter_while_editing_writes_value_updates_separator_and_logs_success() {
    let path = unique_temp_path("commit");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: " :: ".to_string(),
            cursor: 4,
        },
        config_path: Some(path.clone()),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.separator, Some(" :: ".to_string()));
    assert_eq!(app.action_log, vec!["Separator updated"]);
}

#[test]
fn enter_with_empty_buffer_writes_empty_separator_successfully() {
    let path = unique_temp_path("commit-empty");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: String::new(),
            cursor: 0,
        },
        config_path: Some(path.clone()),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.separator, Some(String::new()));
    assert_eq!(app.action_log, vec!["Separator updated"]);
    assert!(content.contains("separator = \"\""));
}
