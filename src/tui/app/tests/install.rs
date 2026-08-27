use super::*;
use crate::install::test_fixtures::{pack_minimal_extension_archive, pack_minimal_module_archive};
use crate::tui::app::InstallTarget;

#[test]
fn i_opens_install_kind_picker_and_esc_cancels() {
    let mut app = App {
        panel_focus: PanelFocus::Modules,
        ..App::default()
    };
    app.handle_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::ChoosingInstallKind { selected: 0 });
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn install_kind_picker_enter_opens_source_editor_and_esc_cancels() {
    let mut app = App {
        mode: Mode::ChoosingInstallKind { selected: 0 },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EnteringInstallSource {
            target: InstallTarget::Module,
            buffer: String::new(),
            cursor: 0,
        }
    );
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn install_kind_picker_down_selects_extension() {
    let mut app = App {
        mode: Mode::ChoosingInstallKind { selected: 0 },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EnteringInstallSource {
            target: InstallTarget::Extension,
            buffer: String::new(),
            cursor: 0,
        }
    );
}

#[test]
fn tui_install_extension_from_archive_updates_list_and_logs_outcome() {
    install_test_log();
    clear_action_log();
    let base = unique_temp_path("tui-install-ext").with_extension("");
    let extensions_dir = base.join("extensions");
    std::fs::create_dir_all(&extensions_dir).unwrap();
    let archive = pack_minimal_extension_archive("probe").unwrap();
    let mut app = App {
        extensions_dir: Some(extensions_dir.clone()),
        mode: Mode::EnteringInstallSource {
            target: InstallTarget::Extension,
            buffer: archive.display().to_string(),
            cursor: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        crate::install::list_extensions_in(&extensions_dir).unwrap(),
        vec!["probe".to_string()]
    );
    assert_eq!(app.installed_extensions, vec!["probe".to_string()]);
    assert!(
        action_log()
            .iter()
            .any(|m| m.contains("installed extension `probe`")),
        "unexpected log: {:?}",
        action_log()
    );
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir_all(archive.with_extension("").with_extension(""));
}

#[test]
fn tui_extension_reinstall_logs_skip_and_stays_normal() {
    install_test_log();
    clear_action_log();
    let base = unique_temp_path("tui-install-ext-skip").with_extension("");
    let extensions_dir = base.join("extensions");
    std::fs::create_dir_all(&extensions_dir).unwrap();
    let archive = pack_minimal_extension_archive("probe").unwrap();
    let source = archive.display().to_string();
    let mut app = App {
        extensions_dir: Some(extensions_dir.clone()),
        mode: Mode::EnteringInstallSource {
            target: InstallTarget::Extension,
            buffer: source.clone(),
            cursor: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(crate::manifest::extension_dir(&extensions_dir, "probe").exists());

    app.mode = Mode::EnteringInstallSource {
        target: InstallTarget::Extension,
        buffer: source.clone(),
        cursor: 0,
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert!(
        action_log()
            .iter()
            .any(|m| m.contains("extension `probe` already installed")),
        "unexpected log: {:?}",
        action_log()
    );
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir_all(archive.with_extension("").with_extension(""));
}

#[test]
fn tui_install_module_from_archive_logs_outcome_and_discovers_kind() {
    install_test_log();
    clear_action_log();
    let base = unique_temp_path("tui-install-mod").with_extension("");
    let modules_dir = base.join("modules");
    std::fs::create_dir_all(&modules_dir).unwrap();
    let archive = pack_minimal_module_archive("widget").unwrap();
    let mut app = App {
        modules_dir: Some(modules_dir.clone()),
        mode: Mode::EnteringInstallSource {
            target: InstallTarget::Module,
            buffer: archive.display().to_string(),
            cursor: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        crate::config::discover_module_kinds(&modules_dir).unwrap(),
        vec!["widget".to_string()]
    );
    assert!(
        action_log()
            .iter()
            .any(|m| m.contains("installed module `widget`")),
        "unexpected log: {:?}",
        action_log()
    );
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn tui_remote_install_prompts_for_sha256() {
    let mut app = App {
        extensions_dir: Some(std::env::temp_dir().join("smstatus-unused-ext")),
        mode: Mode::EnteringInstallSource {
            target: InstallTarget::Extension,
            buffer: "https://example.com/probe.tar.gz".to_string(),
            cursor: 0,
        },
        ..App::default()
    };
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.mode,
        Mode::EnteringInstallSha256 {
            target: InstallTarget::Extension,
            source: "https://example.com/probe.tar.gz".to_string(),
            buffer: String::new(),
            cursor: 0,
        }
    );
}
