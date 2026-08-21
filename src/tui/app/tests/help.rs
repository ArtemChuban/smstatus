use super::*;

#[test]
fn question_mark_enters_help_mode_and_esc_or_question_mark_exits() {
    let mut app = App::default();
    app.handle_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Help);
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);

    app.handle_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Help);
    app.handle_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn question_mark_preserves_panel_focus() {
    let mut app = App {
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Help);
    assert_eq!(app.panel_focus, PanelFocus::Params);
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.panel_focus, PanelFocus::Params);
}

#[test]
fn help_scroll_down_is_noop_when_lines_fit_in_viewport() {
    let mut app = App {
        overlay_viewport_height: 64,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Help);
    assert_eq!(app.help_scroll_offset, 0);
    let line_count = super::super::super::ui::help_lines(&app).len();
    assert!(
        line_count <= app.overlay_viewport_height,
        "precondition: help lines ({line_count}) must fit in overlay viewport"
    );

    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        app.help_scroll_offset, 0,
        "Down must be a no-op when every help line already fits"
    );
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.help_scroll_offset, 0);
}
