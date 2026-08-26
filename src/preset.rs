use std::path::Path;

use crate::config::{
    copy_preset, list_preset_names, preset_file, read_active_name, remove_preset_file,
    write_active_name,
};
use crate::control;
use crate::error::Result;
use crate::reload::ReloadRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsePresetOutcome {
    Switched,
    ReloadDelivered,
    ReloadNotRunning,
}

pub(crate) fn list_presets_in(config_dir: &Path) -> Result<Vec<String>> {
    let names = list_preset_names(config_dir)?;
    let active = read_active_name(config_dir).ok();
    Ok(names
        .into_iter()
        .map(|name| {
            if active.as_deref() == Some(name.as_str()) {
                format!("{name}\t(active)")
            } else {
                name
            }
        })
        .collect())
}

pub(crate) fn list_presets() -> Result<Vec<String>> {
    list_presets_in(&crate::config::default_config_dir()?)
}

pub(crate) fn save_preset_in(config_dir: &Path, name: &str) -> Result<()> {
    let dest = preset_file(config_dir, name)?;
    if dest.is_file() {
        return Err(format!("preset `{name}` already exists").into());
    }
    let active = read_active_name(config_dir)?;
    copy_preset(config_dir, &active, name)
}

pub(crate) fn save_preset(name: &str) -> Result<()> {
    save_preset_in(&crate::config::default_config_dir()?, name)
}

pub(crate) fn use_preset_in(
    config_dir: &Path,
    name: &str,
    reload: bool,
) -> Result<UsePresetOutcome> {
    let path = preset_file(config_dir, name)?;
    if !path.is_file() {
        return Err(format!("preset `{name}` does not exist").into());
    }
    write_active_name(config_dir, name)?;
    if !reload {
        return Ok(UsePresetOutcome::Switched);
    }
    match control::notify_running(ReloadRequest::config()) {
        Ok(control::NotifyOutcome::Delivered) => Ok(UsePresetOutcome::ReloadDelivered),
        Ok(control::NotifyOutcome::NotRunning) => Ok(UsePresetOutcome::ReloadNotRunning),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn use_preset(name: &str, reload: bool) -> Result<UsePresetOutcome> {
    use_preset_in(&crate::config::default_config_dir()?, name, reload)
}

pub(crate) fn remove_preset_in(config_dir: &Path, name: &str) -> Result<()> {
    let active = read_active_name(config_dir)?;
    if active == name {
        return Err(format!("cannot remove active preset `{name}`").into());
    }
    let names = list_preset_names(config_dir)?;
    if names.len() <= 1 {
        return Err("cannot remove the last preset".into());
    }
    if !names.iter().any(|n| n == name) {
        return Err(format!("preset `{name}` does not exist").into());
    }
    remove_preset_file(config_dir, name)
}

pub(crate) fn remove_preset(name: &str) -> Result<()> {
    remove_preset_in(&crate::config::default_config_dir()?, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        active_config_path, create_default_preset, program_config_path, test_fixtures,
    };

    #[test]
    fn list_presets_marks_active() {
        let dir = test_fixtures::unique_config_dir("list-active");
        create_default_preset(&dir).unwrap();
        test_fixtures::write_preset(&dir, "work", "modules = []\n");

        let lines = list_presets_in(&dir).unwrap();
        assert_eq!(
            lines,
            vec!["default\t(active)".to_string(), "work".to_string()]
        );

        write_active_name(&dir, "work").unwrap();
        let lines = list_presets_in(&dir).unwrap();
        assert_eq!(
            lines,
            vec!["default".to_string(), "work\t(active)".to_string()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_preset_copies_active() {
        let dir = test_fixtures::unique_config_dir("save");
        create_default_preset(&dir).unwrap();
        std::fs::write(
            active_config_path(&dir).unwrap(),
            "modules = [\"cpu\"]\nseparator = \" :: \"\n",
        )
        .unwrap();

        save_preset_in(&dir, "work").unwrap();
        let work = std::fs::read_to_string(preset_file(&dir, "work").unwrap()).unwrap();
        assert!(work.contains("cpu"));
        assert!(work.contains(" :: "));

        let err = save_preset_in(&dir, "work").unwrap_err();
        assert!(err.to_string().contains("already exists"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn use_preset_switches_active_pointer() {
        let dir = test_fixtures::unique_config_dir("use");
        create_default_preset(&dir).unwrap();
        test_fixtures::write_preset(&dir, "work", "modules = []\n");

        use_preset_in(&dir, "work", false).unwrap();
        assert_eq!(read_active_name(&dir).unwrap(), "work");
        assert_eq!(
            active_config_path(&dir).unwrap(),
            preset_file(&dir, "work").unwrap()
        );

        let err = use_preset_in(&dir, "missing", false).unwrap_err();
        assert!(err.to_string().contains("does not exist"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn use_preset_reload_succeeds_when_daemon_not_running() {
        let dir = test_fixtures::unique_config_dir("use-reload-not-running");
        create_default_preset(&dir).unwrap();
        test_fixtures::write_preset(&dir, "work", "modules = []\n");

        assert_eq!(
            use_preset_in(&dir, "work", true).unwrap(),
            UsePresetOutcome::ReloadNotRunning
        );
        assert_eq!(read_active_name(&dir).unwrap(), "work");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_preset_rejects_active_and_last() {
        let dir = test_fixtures::unique_config_dir("remove");
        create_default_preset(&dir).unwrap();
        test_fixtures::write_preset(&dir, "work", "modules = []\n");

        let err = remove_preset_in(&dir, "default").unwrap_err();
        assert!(err.to_string().contains("active preset"));

        write_active_name(&dir, "work").unwrap();
        remove_preset_in(&dir, "default").unwrap();
        assert!(!preset_file(&dir, "default").unwrap().is_file());

        std::fs::remove_file(preset_file(&dir, "work").unwrap()).unwrap();
        let err = remove_preset_in(&dir, "default").unwrap_err();
        assert!(err.to_string().contains("last preset"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_preset_errors_when_missing() {
        let dir = test_fixtures::unique_config_dir("remove-missing");
        create_default_preset(&dir).unwrap();
        test_fixtures::write_preset(&dir, "work", "modules = []\n");

        let err = remove_preset_in(&dir, "ghost").unwrap_err();
        assert!(err.to_string().contains("does not exist"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_active_name_updates_program_config() {
        let dir = test_fixtures::unique_config_dir("program-config");
        create_default_preset(&dir).unwrap();
        test_fixtures::write_preset(&dir, "work", "modules = []\n");
        use_preset_in(&dir, "work", false).unwrap();

        let content = std::fs::read_to_string(program_config_path(&dir)).unwrap();
        assert!(content.contains("active = \"work\""));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
