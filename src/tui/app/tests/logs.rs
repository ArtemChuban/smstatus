use super::*;
use crate::tui::app::{LOGS_PANEL_LINES, PanelFocus};

#[test]
fn tab_focuses_logs_and_sets_follow() {
    let mut app = App::default();
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Logs);
    assert!(app.logs_follow);
}

#[test]
fn tab_from_params_focuses_logs() {
    let mut app = App {
        panel_focus: PanelFocus::Params,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Logs);
    assert!(app.logs_follow);
}

#[test]
fn esc_and_left_from_logs_return_to_modules() {
    let mut app = App {
        panel_focus: PanelFocus::Logs,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Modules);

    app.panel_focus = PanelFocus::Logs;
    app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Modules);
}

#[test]
fn tab_from_logs_returns_to_modules() {
    let mut app = App {
        panel_focus: PanelFocus::Logs,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Modules);
}

#[test]
fn up_from_newest_clears_follow_and_down_restores_it() {
    install_test_log();
    let path = crate::logging::current_log_path().expect("test log path");
    std::fs::write(&path, "line0\nline1\nline2\nline3\nline4\n").unwrap();

    let mut app = App::default();
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert!(app.logs_follow);
    assert_eq!(app.logs_selected_index, Some(4));

    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert!(!app.logs_follow);
    assert_eq!(app.logs_selected_index, Some(3));

    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert!(app.logs_follow);
    assert_eq!(app.logs_selected_index, Some(4));
}

#[test]
fn tab_away_and_back_restores_follow() {
    install_test_log();
    let path = crate::logging::current_log_path().expect("test log path");
    std::fs::write(&path, "a\nb\nc\nd\n").unwrap();

    let mut app = App::default();
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert!(!app.logs_follow);

    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Modules);
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.panel_focus, PanelFocus::Logs);
    assert!(app.logs_follow);
    assert_eq!(app.logs_selected_index, Some(3));
}

#[test]
fn scroll_offset_clamps_selected_line_into_viewport() {
    install_test_log();
    let path = crate::logging::current_log_path().expect("test log path");
    std::fs::write(&path, "l0\nl1\nl2\nl3\nl4\nl5\n").unwrap();

    let mut app = App::default();
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.logs_selected_index, Some(5));
    assert_eq!(app.logs_scroll_offset, 3);

    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.logs_selected_index, Some(2));
    assert_eq!(app.logs_scroll_offset, 2);
    let idx = app.logs_selected_index.unwrap();
    assert!(idx >= app.logs_scroll_offset);
    assert!(idx < app.logs_scroll_offset + LOGS_PANEL_LINES);
}
