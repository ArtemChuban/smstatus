use super::*;

#[test]
fn enter_without_config_path_logs_failure_and_returns_to_normal() {
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: " | ".to_string(),
            cursor: 3,
        },
        config_path: None,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.action_log,
        vec!["cannot save separator: config path unknown"]
    );
}

#[test]
fn enter_when_write_fails_logs_failure_and_returns_to_normal() {
    let path = unique_temp_path("nonexistent");
    let mut app = App {
        mode: Mode::EditingSeparator {
            buffer: " | ".to_string(),
            cursor: 3,
        },
        config_path: Some(path),
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.action_log.len(), 1);
    assert!(
        app.action_log[0].starts_with("Failed to update separator:"),
        "unexpected message: {}",
        app.action_log[0]
    );
}
