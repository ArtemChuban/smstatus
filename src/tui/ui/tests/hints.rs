use super::*;

#[test]
fn hint_line_normal_mode_modules_focus() {
    let app = App::default();
    assert_eq!(hint_line(&app).as_ref(), NORMAL_HINT_MODULES);
}

#[test]
fn hint_line_normal_mode_params_focus() {
    let app = App {
        panel_focus: PanelFocus::Params,
        ..App::default()
    };
    assert_eq!(hint_line(&app).as_ref(), NORMAL_HINT_PARAMS);
}

#[test]
fn hint_line_editing_separator_mode() {
    let app = App {
        mode: Mode::EditingSeparator {
            buffer: String::new(),
            cursor: 0,
        },
        ..App::default()
    };
    assert_eq!(hint_line(&app).as_ref(), "Save: Enter | Cancel: Esc");
}

#[test]
fn hint_line_adding_module_mode() {
    let app = App {
        mode: Mode::AddingModule {
            available: vec![],
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    assert_eq!(
        hint_line(&app).as_ref(),
        "Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc"
    );
}

#[test]
fn hint_line_naming_module_instance_mode() {
    let app = App {
        mode: Mode::NamingModuleInstance {
            kind: "cpu".to_string(),
            buffer: String::new(),
            cursor: 0,
        },
        ..App::default()
    };
    assert_eq!(hint_line(&app).as_ref(), "Confirm: Enter | Cancel: Esc");
}

#[test]
fn hint_line_confirming_remove_mode_includes_module_name() {
    let app = App {
        mode: Mode::ConfirmingRemove {
            index: 0,
            name: "cpu".to_string(),
        },
        ..App::default()
    };
    assert_eq!(
        hint_line(&app).as_ref(),
        "Remove cpu? Confirm: d | Cancel: any key"
    );
}

#[test]
fn hint_line_help_mode() {
    let app = App {
        mode: Mode::Help,
        ..App::default()
    };
    assert_eq!(hint_line(&app).as_ref(), HELP_HINT);
}

#[test]
fn help_lines_modules_focus_lists_local_module_bindings() {
    let app = App {
        panel_focus: PanelFocus::Modules,
        ..App::default()
    };
    let lines = help_lines(&app);
    assert_eq!(lines[0], "--- Local ---");
    assert!(lines.iter().any(|l| l.contains("Add module: a")));
    assert!(lines.iter().any(|l| l.contains("Browse extensions: x")));
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Install module/extension: i"))
    );
    assert!(lines.iter().any(|l| l.contains("Focus params")));
    assert!(lines.iter().any(|l| l.contains("Focus logs: Tab")));
    assert!(lines.iter().any(|l| l == "--- Global ---"));
    assert!(lines.iter().any(|l| l == "Quit: q"));
}

#[test]
fn help_lines_params_focus_lists_edit_add_del_rename() {
    let app = App {
        panel_focus: PanelFocus::Params,
        ..App::default()
    };
    let lines = help_lines(&app);
    assert!(lines.iter().any(|l| l.contains("Select param")));
    assert!(lines.iter().any(|l| l.contains("Edit value")));
    assert!(lines.iter().any(|l| l.contains("Add param")));
    assert!(lines.iter().any(|l| l.contains("Remove param")));
    assert!(lines.iter().any(|l| l.contains("Rename key")));
    assert!(lines.iter().any(|l| l.contains("Focus logs: Tab")));
    assert!(lines.iter().any(|l| l.contains("Back to modules")));
    assert!(!lines.iter().any(|l| l.contains("Add module")));
    assert!(!lines.iter().any(|l| l.contains("Edit separator")));
}

#[test]
fn hint_line_normal_mode_logs_focus() {
    let app = App {
        panel_focus: PanelFocus::Logs,
        ..App::default()
    };
    assert_eq!(hint_line(&app).as_ref(), NORMAL_HINT_LOGS);
}

#[test]
fn help_lines_logs_focus_lists_scroll_and_back() {
    let app = App {
        panel_focus: PanelFocus::Logs,
        ..App::default()
    };
    let lines = help_lines(&app);
    assert!(lines.iter().any(|l| l.contains("Scroll logs")));
    assert!(lines.iter().any(|l| l.contains("Toggle ERROR/WARN/INFO")));
    assert!(lines.iter().any(|l| l.contains("Back to modules")));
    assert!(!lines.iter().any(|l| l.contains("Add module")));
}

#[test]
fn hint_line_browsing_extensions_mode() {
    let app = App {
        mode: Mode::BrowsingExtensions {
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    assert_eq!(
        hint_line(&app).as_ref(),
        "Select: \u{2191}/\u{2193} | Install: i | Close: Esc"
    );
}

#[test]
fn help_lines_browsing_extensions_lists_close() {
    let app = App {
        mode: Mode::BrowsingExtensions {
            selected: 0,
            scroll_offset: 0,
        },
        ..App::default()
    };
    let lines = help_lines(&app);
    assert!(lines.iter().any(|l| l.contains("Select extension")));
    assert!(lines.iter().any(|l| l.contains("Install: i")));
    assert!(lines.iter().any(|l| l.contains("Close: Esc")));
}

#[test]
fn hint_line_choosing_install_kind_mode() {
    let app = App {
        mode: Mode::ChoosingInstallKind { selected: 0 },
        ..App::default()
    };
    assert_eq!(
        hint_line(&app).as_ref(),
        "Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc"
    );
}

#[test]
fn hint_line_entering_install_source_mode() {
    let app = App {
        mode: Mode::EnteringInstallSource {
            target: InstallTarget::Module,
            buffer: String::new(),
            cursor: 0,
        },
        ..App::default()
    };
    assert_eq!(hint_line(&app).as_ref(), "Confirm: Enter | Cancel: Esc");
}
