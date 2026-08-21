use super::*;

#[test]
fn cursor_hidden_in_normal_mode() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(!terminal.backend().cursor_visible());
}

#[test]
fn cursor_visible_and_positioned_at_start_of_empty_buffer() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::EditingSeparator {
            buffer: String::new(),
            cursor: 0,
        },
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(terminal.backend().cursor_visible());
    assert_eq!(terminal.backend().cursor_position(), Position::new(18, 2));
}

#[test]
fn cursor_visible_and_positioned_mid_buffer() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::EditingSeparator {
            buffer: "ab".to_string(),
            cursor: 1,
        },
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(terminal.backend().cursor_visible());
    assert_eq!(terminal.backend().cursor_position(), Position::new(19, 2));
}

#[test]
fn cursor_visible_and_positioned_at_end_of_buffer() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::EditingSeparator {
            buffer: "ab".to_string(),
            cursor: 2,
        },
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(terminal.backend().cursor_visible());
    assert_eq!(terminal.backend().cursor_position(), Position::new(20, 2));
}

#[test]
fn cursor_column_accounts_for_debug_escaped_characters() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::EditingSeparator {
            buffer: "a\"b".to_string(),
            cursor: 2,
        },
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(terminal.backend().cursor_visible());
    assert_eq!(terminal.backend().cursor_position(), Position::new(21, 2));
}

#[test]
fn cursor_hidden_in_help_mode() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::Help,
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(!terminal.backend().cursor_visible());
}

#[test]
fn cursor_hidden_in_adding_module_mode() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::AddingModule {
            available: vec!["cpu".to_string()],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(!terminal.backend().cursor_visible());
}

#[test]
fn cursor_hidden_in_confirming_remove_mode() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        mode: Mode::ConfirmingRemove {
            index: 0,
            name: "cpu".to_string(),
        },
        ..App::default()
    };
    let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
    assert!(!terminal.backend().cursor_visible());
}
