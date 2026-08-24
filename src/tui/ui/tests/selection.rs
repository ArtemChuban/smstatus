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
            "modules 3-4/6",
            &["m2", "m3"],
            "config m3",
            &["(no [m3] section)"],
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
