use super::*;
use crate::tui::app::InstallTarget;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn write_tar_gz(staging: &Path, archive: &Path) {
    let file = std::fs::File::create(archive).unwrap();
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.append_dir_all(".", staging).unwrap();
    builder.finish().unwrap();
}

fn pack_minimal_extension_archive(name: &str) -> std::path::PathBuf {
    let staging = unique_temp_path(&format!("ext-pack-{name}")).with_extension("");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(
        staging.join("manifest.toml"),
        format!(
            "name = \"{name}\"\nversion = \"0.1.0\"\nauthor = \"test\"\nextensions-api = {{ major = 0, minor = 1 }}\n"
        ),
    )
    .unwrap();
    let binary = staging.join("extension");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let archive = Path::new(&format!("{}.tar.gz", staging.display())).to_path_buf();
    write_tar_gz(&staging, &archive);
    archive
}

fn pack_minimal_module_archive(name: &str) -> std::path::PathBuf {
    let staging = unique_temp_path(&format!("mod-pack-{name}")).with_extension("");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(
        staging.join("manifest.toml"),
        format!(
            "name = \"{name}\"\ndisplay_name = \"{name}\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = {{ major = 0, minor = 1 }}\n"
        ),
    )
    .unwrap();
    std::fs::write(staging.join("module.wasm"), b"\0asm").unwrap();
    let archive = Path::new(&format!("{}.tar.gz", staging.display())).to_path_buf();
    write_tar_gz(&staging, &archive);
    archive
}

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
    let archive = pack_minimal_extension_archive("probe");
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
fn tui_install_module_from_archive_logs_outcome_and_discovers_kind() {
    install_test_log();
    clear_action_log();
    let base = unique_temp_path("tui-install-mod").with_extension("");
    let modules_dir = base.join("modules");
    std::fs::create_dir_all(&modules_dir).unwrap();
    let archive = pack_minimal_module_archive("widget");
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
