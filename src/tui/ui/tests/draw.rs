use super::*;

#[test]
fn draw_renders_stopped_status_in_outer_title() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_running_status_in_outer_title() {
    let app = App {
        daemon_status: Some(DaemonStatus::Running { pid: 12345 }),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Running { pid: 12345 }),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_running_pid_unknown_status_in_outer_title() {
    let app = App {
        daemon_status: Some(DaemonStatus::RunningPidUnknown),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::RunningPidUnknown),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_unknown_status_in_outer_title() {
    let app = App {
        daemon_status: None,
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            None,
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_separator_value() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        separator: Some(" | ".to_string()),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: \" | \"",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_empty_separator_as_quoted_empty_string() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        separator: Some(String::new()),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: \"\"",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_editing_prompt_with_buffer_contents() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::EditingSeparator {
            buffer: "::".to_string(),
            cursor: 2,
        },
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "New separator: \"::\"",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            "Save: Enter | Cancel: Esc"
        )
    );
}

#[test]
fn draw_renders_editing_prompt_with_empty_buffer() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::EditingSeparator {
            buffer: String::new(),
            cursor: 0,
        },
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "New separator: \"\"",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            "Save: Enter | Cancel: Esc"
        )
    );
}

#[test]
fn draw_renders_empty_action_log_as_blank_rows() {
    install_test_log();
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_single_action_message_padded_to_capacity() {
    seed_log_lines(&["Starting smstatus..."]);
    let mut app = App {
        daemon_status: Some(DaemonStatus::Running { pid: 42 }),
        ..App::default()
    };
    assert_eq!(
        render_with_log(&mut app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Running { pid: 42 }),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs 1-1/1",
            &["Starting smstatus..."],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_action_log_at_capacity() {
    seed_log_lines(&[
        "Starting smstatus...",
        "smstatus is already running",
        "Sent stop signal to smstatus (pid 42)",
    ]);
    let mut app = App {
        daemon_status: Some(DaemonStatus::Stopped),
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
            "logs 1-3/3",
            &[
                "Starting smstatus...",
                "smstatus is already running",
                "Sent stop signal to smstatus (pid 42)",
            ],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_empty_module_list_title_with_zero_rows() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec![]),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules (none configured)",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_degraded_title_when_viewport_height_is_zero_with_modules_present() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec![
            "cpu".to_string(),
            "disk#root".to_string(),
            "battery".to_string(),
        ]),
        ..App::default()
    };
    let height = 10;
    assert_eq!(
        render(&app, 70, height),
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 3 configured",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_modules_that_fully_fit_in_the_viewport() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec![
            "cpu".to_string(),
            "disk#root".to_string(),
            "battery".to_string(),
        ]),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 3;
    assert_eq!(
        render(&app, 70, height),
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 1-3/3",
            &["cpu", "disk#root", "battery"],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_scrolled_slice_of_modules_when_more_than_fit() {
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
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 2;
    assert_eq!(
        render(&app, 70, height),
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 3-6/6",
            &["m2", "m3", "m4", "m5"],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_instance_suffixed_module_entry_verbatim() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["disk#root".to_string()]),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 1;
    assert_eq!(
        render(&app, 70, height),
        expected(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules 1-1/1",
            &["disk#root"],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_missing_params_section_message() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        module_params: Some(params_missing("cpu")),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 1;
    assert_eq!(
        render(&app, 70, height),
        with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["cpu"],
                "config cpu",
                &["(no [cpu] section)"],
                "logs",
                &[],
                NORMAL_HINT_MODULES
            ),
            5,
            70,
        )
    );
}

#[test]
fn draw_renders_empty_params_section_message() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        module_params: Some(params_empty()),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 1;
    assert_eq!(
        render(&app, 70, height),
        with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["cpu"],
                "config cpu",
                &["(empty)"],
                "logs",
                &[],
                NORMAL_HINT_MODULES
            ),
            5,
            70,
        )
    );
}

#[test]
fn draw_renders_string_and_non_string_param_entries() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["disk#root".to_string()]),
        selected_index: Some(0),
        module_params: Some(params_entries(vec![
            ParamEntry {
                key: "path".to_string(),
                value: ModuleParamValue::String("/".to_string()),
                origin: ParamOrigin::Explicit,
            },
            ParamEntry {
                key: "interval".to_string(),
                value: ModuleParamValue::NonString,
                origin: ParamOrigin::Explicit,
            },
        ])),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 2;
    assert_eq!(
        render(&app, 70, height),
        with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["disk#root"],
                "config disk#root",
                &["path = \"/\"", "interval = <non-string>"],
                "logs",
                &[],
                NORMAL_HINT_MODULES
            ),
            5,
            70,
        )
    );
}

#[test]
fn draw_at_very_short_height_renders_only_what_fits_without_corrupting_borders() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, 4),
        expected(
            70,
            4,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_at_height_with_only_settings_and_hint_present() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, 8),
        expected(
            70,
            8,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "modules unknown",
            &[],
            "config",
            &[],
            "logs",
            &[],
            NORMAL_HINT_MODULES
        )
    );
}

#[test]
fn draw_renders_add_picker_title_list_and_hint() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::AddingModule {
            available: vec!["battery".to_string(), "cpu".to_string(), "disk".to_string()],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 5;
    let expected_buf = expected_overlay(
        70,
        height,
        Some(DaemonStatus::Stopped),
        "separator: unknown",
        "add module 1-3/3",
        &["battery", "cpu", "disk"],
        "logs",
        &[],
        "Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc",
    );
    let overlay = overlay_area_for_frame(70, height);
    let inner_y = Block::default().borders(Borders::ALL).inner(overlay).y;
    let expected_buf = with_reversed_overlay_row(expected_buf, inner_y, 70, height);
    assert_eq!(render(&app, 70, height), expected_buf);
}

#[test]
fn draw_renders_add_picker_with_empty_available_list() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::AddingModule {
            available: vec![],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    assert_eq!(
        render(&app, 70, BASELINE_HEIGHT),
        expected_overlay(
            70,
            BASELINE_HEIGHT,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "add module 0 available",
            &[],
            "logs",
            &[],
            "Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc"
        )
    );
}

#[test]
fn draw_renders_naming_instance_prompt_and_cursor() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::NamingModuleInstance {
            kind: "disk".to_string(),
            buffer: "root".to_string(),
            cursor: 4,
        },
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 1;
    let terminal = render_terminal(&app, 70, height);
    assert_eq!(
        terminal.backend().buffer().clone(),
        expected_overlay(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "name instance of disk",
            &["instance name for disk: \"root\""],
            "logs",
            &[],
            "Confirm: Enter | Cancel: Esc"
        )
    );
    assert!(terminal.backend().cursor_visible());
    let overlay = overlay_area_for_frame(70, height);
    let inner = Block::default().borders(Borders::ALL).inner(overlay);
    let prefix = instance_name_prefix("disk");
    let col = inner.x + text_edit_cursor_column(&prefix, "root", 4);
    assert_eq!(
        terminal.backend().cursor_position(),
        Position::new(col, inner.y)
    );
}

#[test]
fn draw_renders_compact_param_value_overlay_without_prefix() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::EditingParamValue {
            section: "cpu".to_string(),
            key: "format".to_string(),
            buffer: "hi".to_string(),
            cursor: 2,
            expect: ParamWriteExpect::ExistingString("old".to_string()),
        },
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 4;
    let terminal = render_terminal(&app, 70, height);
    assert_eq!(
        terminal.backend().buffer().clone(),
        expected_param_text_overlay(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "value for format",
            "\"hi\"",
            "logs",
            &[],
            "Confirm: Enter | Cancel: Esc"
        )
    );
    let overlay = param_text_overlay_area_for_frame(70, height);
    assert_eq!(overlay.height, PARAM_TEXT_OVERLAY_HEIGHT);
    assert!(terminal.backend().cursor_visible());
    let inner = Block::default().borders(Borders::ALL).inner(overlay);
    let col = inner.x + text_edit_cursor_column("", "hi", 2);
    assert_eq!(
        terminal.backend().cursor_position(),
        Position::new(col, inner.y)
    );
}

#[test]
fn draw_renders_confirming_remove_hint_with_unchanged_list_and_title() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec![
            "cpu".to_string(),
            "disk".to_string(),
            "battery".to_string(),
        ]),
        selected_index: Some(1),
        module_params: Some(params_missing("disk")),
        mode: Mode::ConfirmingRemove {
            index: 1,
            name: "disk".to_string(),
        },
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 3;
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
            "Remove disk? Confirm: d | Cancel: any key",
        ),
        6,
        70,
    );
    assert_eq!(render(&app, 70, height), expected_buf);
}

#[test]
fn draw_renders_help_title_content_and_hint() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::Help,
        ..App::default()
    };
    let lines = help_lines(&app);
    let height = BASELINE_HEIGHT + lines.len() as u16 + 2;
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    assert_eq!(
        render(&app, 70, height),
        expected_overlay(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "help",
            &line_refs,
            "logs",
            &[],
            HELP_HINT
        )
    );
}

fn sample_cpu_metadata() -> Metadata {
    Metadata {
        display_name: "CPU".to_string(),
        version: "0.1.0".to_string(),
        author: "ArtemChuban".to_string(),
    }
}

fn sample_disk_metadata() -> Metadata {
    Metadata {
        display_name: "Disk".to_string(),
        version: "0.1.0".to_string(),
        author: "ArtemChuban".to_string(),
    }
}

#[test]
fn draw_renders_metadata_display_name_for_undecorated_module() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        metadata_by_kind: [("cpu".to_string(), sample_cpu_metadata())]
            .into_iter()
            .collect(),
        module_params: Some(params_empty()),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 1;
    assert_eq!(
        render(&app, 70, height),
        with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["CPU 0.1.0 by ArtemChuban"],
                "config CPU",
                &["(empty)"],
                "logs",
                &[],
                NORMAL_HINT_MODULES
            ),
            5,
            70,
        )
    );
}

#[test]
fn draw_renders_metadata_with_entry_for_instance_module() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["disk#root".to_string()]),
        selected_index: Some(0),
        metadata_by_kind: [("disk".to_string(), sample_disk_metadata())]
            .into_iter()
            .collect(),
        module_params: Some(params_empty()),
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 1;
    assert_eq!(
        render(&app, 70, height),
        with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["Disk (disk#root) 0.1.0 by ArtemChuban"],
                "config Disk (disk#root)",
                &["(empty)"],
                "logs",
                &[],
                NORMAL_HINT_MODULES
            ),
            5,
            70,
        )
    );
}

#[test]
fn draw_renders_extensions_overlay_title_list_and_hint() {
    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        mode: Mode::BrowsingExtensions {
            selected: 0,
            scroll_offset: 0,
        },
        installed_extensions: vec!["echo".to_string(), "notify".to_string()],
        extension_overlay_labels: vec!["echo".to_string(), "notify".to_string()],
        ..App::default()
    };
    let height = BASELINE_HEIGHT + 5;
    let expected_buf = expected_overlay(
        70,
        height,
        Some(DaemonStatus::Stopped),
        "separator: unknown",
        "extensions 1-2/2",
        &["echo", "notify"],
        "logs",
        &[],
        "Select: \u{2191}/\u{2193} | Close: Esc",
    );
    let overlay = overlay_area_for_frame(70, height);
    let inner_y = Block::default().borders(Borders::ALL).inner(overlay).y;
    let expected_buf = with_reversed_overlay_row(expected_buf, inner_y, 70, height);
    assert_eq!(render(&app, 70, height), expected_buf);
}

#[test]
fn draw_params_column_pins_required_extensions_header() {
    use crate::manifest::{ApiVersionReq, RequiredExtension};
    use std::collections::HashMap;

    let app = App {
        daemon_status: Some(DaemonStatus::Stopped),
        modules: Some(vec!["cpu".to_string()]),
        selected_index: Some(0),
        module_params: Some(params_entries(vec![ParamEntry {
            key: "a".to_string(),
            value: ModuleParamValue::String("1".to_string()),
            origin: ParamOrigin::Explicit,
        }])),
        required_extensions_by_kind: HashMap::from([(
            "cpu".to_string(),
            vec![RequiredExtension {
                name: "echo".to_string(),
                version: ApiVersionReq { major: 1, minor: 0 },
            }],
        )]),
        requirement_lines_by_kind: HashMap::from([(
            "cpu".to_string(),
            vec!["echo missing".to_string()],
        )]),
        ..App::default()
    };
    let lines = visible_params_lines(&app, 10, 40);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].to_string(), "echo missing");
    assert_eq!(lines[1].to_string(), "a = \"1\"");
    let buffer = render(&app, 70, BASELINE_HEIGHT + 2);
    let row: String = (0..70)
        .map(|x| buffer[(x, 5)].symbol().to_string())
        .collect();
    assert!(
        row.contains("echo missing"),
        "params column should show requirement status, got {row:?}"
    );
}
