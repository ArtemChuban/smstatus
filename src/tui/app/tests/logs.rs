use super::*;
use crate::tui::app::{LOGS_CHUNK_LINES, PanelFocus};

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
    assert_eq!(app.logs_total, 5);

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

    let mut app = App {
        logs_viewport_height: 3,
        ..App::default()
    };
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
    assert!(idx < app.logs_scroll_offset + app.logs_viewport_height);
}

#[test]
fn refresh_keeps_selected_line_when_new_log_appended() {
    install_test_log();
    let path = crate::logging::current_log_path().expect("test log path");
    std::fs::write(&path, "a\nb\nc\nd\n").unwrap();

    let mut app = App {
        logs_viewport_height: 3,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert!(!app.logs_follow);
    assert_eq!(app.logs_selected_index, Some(1));
    let selected = app.log_history[1].clone();

    std::fs::write(&path, "a\nb\nc\nd\ne\n").unwrap();
    app.refresh_log_history();
    assert_eq!(app.logs_total, 5);
    assert_eq!(app.logs_selected_index, Some(1));
    assert_eq!(app.log_history[1], selected);
}

#[test]
fn refresh_keeps_selected_line_when_history_drops_oldest() {
    install_test_log();
    let path = crate::logging::current_log_path().expect("test log path");
    std::fs::write(&path, "line0\nline1\nline2\nline3\nline4\n").unwrap();

    let mut app = App {
        logs_viewport_height: 3,
        logs_follow: false,
        panel_focus: PanelFocus::Logs,
        ..App::default()
    };
    app.refresh_log_history();
    app.logs_follow = false;
    app.logs_selected_index = Some(1);
    app.logs_scroll_offset = 0;
    let selected = app.log_history[1].clone();
    assert_eq!(selected, "line1");

    std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();
    app.refresh_log_history();
    assert_eq!(app.logs_total, 5);
    assert_eq!(app.logs_selected_index, Some(0));
    let rel = app.logs_selected_index.unwrap() - app.logs_loaded_from;
    assert_eq!(app.log_history[rel], selected);
    assert_eq!(app.logs_scroll_offset, 0);
}

#[test]
fn scrolling_up_past_chunk_loads_older_lines_with_full_total() {
    install_test_log();
    let path = crate::logging::current_log_path().expect("test log path");
    let mut content = String::new();
    let total = LOGS_CHUNK_LINES + 50;
    for i in 0..total {
        content.push_str(&format!("line{i}\n"));
    }
    std::fs::write(&path, content).unwrap();

    let mut app = App {
        logs_viewport_height: 3,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.logs_total, total);
    assert!(app.log_history.len() <= LOGS_CHUNK_LINES.max(6));
    assert!(app.logs_loaded_from > 0);

    // Walk up past the initially loaded window.
    for _ in 0..(app.logs_selected_index.unwrap() + 1) {
        app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    }
    assert_eq!(app.logs_selected_index, Some(0));
    assert_eq!(app.logs_loaded_from, 0);
    assert_eq!(app.logs_total, total);
    assert!(app.log_history.len() >= total.min(LOGS_CHUNK_LINES + 50));
}
