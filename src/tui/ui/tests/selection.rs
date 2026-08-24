use super::*;

#[test]
fn selected_module_row_is_rendered_with_reversed_style() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec![
            "cpu".to_string(),
            "disk".to_string(),
            "battery".to_string(),
        ]),
        selected_index: Some(1),
        module_params: Some(params_missing("disk")),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 3;
    let buffer = render(&app, 70, height);
    let expected_buf = with_reversed_modules_row(
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 1-3/3",
            &["cpu", "disk", "battery"],
            "config disk",
            &["(no [disk] section)"],
            "logs",
            &[],
            NORMAL_HINT_MODULES,
        ),
        6,
        70,
    );
    assert_eq!(buffer, expected_buf);
}

#[test]
fn selected_param_row_reversed_only_when_params_focused() {
    let height = BASELINE_HEIGHT + 2;

    let modules_focus = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Modules,
        module_params: Some(params_entries(vec![
            ParamEntry {
                key: "a".to_string(),
                value: ModuleParamValue::String("1".to_string()),
                origin: ParamOrigin::Explicit,
            },
            ParamEntry {
                key: "b".to_string(),
                value: ModuleParamValue::String("2".to_string()),
                origin: ParamOrigin::Explicit,
            },
        ])),
        ..App::default()
    };
    let buf = render(&modules_focus, 70, height);
    let exp = with_reversed_modules_row(
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 1-1/1",
            &["cpu"],
            "config cpu",
            &["a = \"1\"", "b = \"2\""],
            "logs",
            &[],
            NORMAL_HINT_MODULES,
        ),
        5,
        70,
    );
    assert_eq!(buf, exp);

    let params_focus = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        panel_focus: PanelFocus::Params,
        module_params: Some(params_entries(vec![
            ParamEntry {
                key: "a".to_string(),
                value: ModuleParamValue::String("1".to_string()),
                origin: ParamOrigin::Explicit,
            },
            ParamEntry {
                key: "b".to_string(),
                value: ModuleParamValue::String("2".to_string()),
                origin: ParamOrigin::Explicit,
            },
        ])),
        ..App::default()
    };
    let buf = render(&params_focus, 70, height);
    let exp = with_reversed_params_row(
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 1-1/1",
            &["cpu"],
            "config cpu",
            &["a = \"1\"", "b = \"2\""],
            "logs",
            &[],
            NORMAL_HINT_PARAMS,
        ),
        5,
        70,
    );
    assert_eq!(buf, exp);
}

#[test]
fn selected_module_row_style_follows_selection_when_scrolled() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec![
            "m0".to_string(),
            "m1".to_string(),
            "m2".to_string(),
            "m3".to_string(),
            "m4".to_string(),
            "m5".to_string(),
        ]),
        module_scroll_offset: 2,
        selected_index: Some(3),
        module_params: Some(params_missing("m3")),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 2;
    let buffer = render(&app, 70, height);
    let expected_buf = with_reversed_modules_row(
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 3-6/6",
            &["m2", "m3", "m4", "m5"],
            "config m3",
            &["(no [m3] section)"],
            "logs",
            &[],
            NORMAL_HINT_MODULES,
        ),
        6,
        70,
    );
    assert_eq!(buffer, expected_buf);
}

#[test]
fn no_module_row_is_reversed_styled_when_selection_is_none() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec![
            "cpu".to_string(),
            "disk".to_string(),
            "battery".to_string(),
        ]),
        selected_index: None,
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 3;
    let buffer = render(&app, 70, height);
    for y in 5..=7u16 {
        for x in 0..70 {
            assert!(
                !buffer[(x, y)].modifier.contains(Modifier::REVERSED),
                "expected no reversed styling at ({x},{y}) when nothing is selected"
            );
        }
    }
}

#[test]
fn draw_logs_scrolled_up_shows_older_seeded_lines() {
    seed_log_lines(&["old0", "old1", "old2", "new3", "new4"]);
    let mut app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        panel_focus: PanelFocus::Logs,
        logs_follow: false,
        logs_scroll_offset: 0,
        logs_selected_index: Some(0),
        ..App::default()
    };
    let buffer = render_with_log(&mut app, 70, BASELINE_HEIGHT);
    let expected_buf = with_reversed_logs_row(
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs 1-3/5",
            &["old0", "old1", "old2"],
            NORMAL_HINT_LOGS,
        ),
        10,
        70,
    );
    assert_eq!(buffer, expected_buf);
}

#[test]
fn draw_logs_follow_still_shows_newest_tail_when_unfocused() {
    seed_log_lines(&["old0", "old1", "old2", "new3", "new4"]);
    let mut app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        logs_follow: true,
        ..App::default()
    };
    assert_eq!(
        render_with_log(&mut app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs 3-5/5",
            &["old2", "new3", "new4"],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn logs_title_includes_file_total_when_filtered() {
    seed_log_lines(&[
        "2026-08-24T08:41:00.000Z ERROR e1",
        "2026-08-24T08:41:00.000Z INFO i1",
        "2026-08-24T08:41:00.000Z WARN w1",
    ]);
    let mut app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        panel_focus: PanelFocus::Logs,
        logs_follow: true,
        logs_show_info: false,
        ..App::default()
    };
    let _ = render_with_log(&mut app, 70, BASELINE_HEIGHT);
    assert_eq!(app.logs_total, 2);
    assert_eq!(app.logs_file_total, 3);
    let title = logs_title(&app, app.logs_viewport_height.max(1));
    assert!(
        title.contains("of 3"),
        "expected file total in title, got {title}"
    );
}

#[test]
fn logs_title_shows_file_total_when_filter_hides_all_lines() {
    seed_log_lines(&[
        "2026-08-24T08:41:00.000Z ERROR e1",
        "2026-08-24T08:41:00.000Z INFO i1",
        "2026-08-24T08:41:00.000Z WARN w1",
    ]);
    let mut app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        panel_focus: PanelFocus::Logs,
        logs_follow: true,
        logs_show_error: false,
        logs_show_warn: false,
        logs_show_info: false,
        ..App::default()
    };
    let _ = render_with_log(&mut app, 70, BASELINE_HEIGHT);
    assert_eq!(app.logs_total, 0);
    assert_eq!(app.logs_file_total, 3);
    let title = logs_title(&app, app.logs_viewport_height.max(1));
    assert!(
        title.contains("0 of 3"),
        "expected empty filtered title to keep file total, got {title}"
    );
}
