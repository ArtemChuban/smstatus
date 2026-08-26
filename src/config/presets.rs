//! Preset storage and program-wide config helpers.
//!
//! Root `config.toml` holds the active preset pointer; bar layout lives in
//! `presets/<name>.toml`. See `create_default_preset` for initial layout (#64).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::extension::is_safe_extension_name;

use super::io::atomic_write;

pub(crate) const PRESETS_SUBDIR: &str = "presets";
pub(crate) const PROGRAM_CONFIG_FILE: &str = "config.toml";
pub(crate) const DEFAULT_PRESET_NAME: &str = "default";

pub(crate) fn program_config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(PROGRAM_CONFIG_FILE)
}

pub(crate) fn presets_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(PRESETS_SUBDIR)
}

pub(crate) fn preset_file(config_dir: &Path, name: &str) -> Result<PathBuf> {
    if !is_safe_extension_name(name) {
        return Err(format!("invalid preset name `{name}`").into());
    }
    Ok(presets_dir(config_dir).join(format!("{name}.toml")))
}

pub(crate) fn read_active_name(config_dir: &Path) -> Result<String> {
    let path = program_config_path(config_dir);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc = content
        .parse::<toml::Table>()
        .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    let presets = doc
        .get("presets")
        .and_then(|v| v.as_table())
        .ok_or_else(|| format!("{}: missing `[presets]` section", path.display()))?;
    let active = presets
        .get("active")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{}: missing `[presets].active`", path.display()))?;
    let trimmed = active.trim();
    if trimmed.is_empty() {
        return Err(format!("{}: `[presets].active` must not be empty", path.display()).into());
    }
    Ok(trimmed.to_string())
}

pub(crate) fn write_active_name(config_dir: &Path, name: &str) -> Result<()> {
    let preset_path = preset_file(config_dir, name)?;
    if !preset_path.is_file() {
        return Err(format!("preset file does not exist: {}", preset_path.display()).into());
    }

    let path = program_config_path(config_dir);
    let content = if path.is_file() {
        std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?
    } else {
        String::new()
    };

    let mut doc = if content.is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("cannot parse {}: {e}", path.display()))?
    };

    let presets = doc
        .entry("presets")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    if let Some(table) = presets.as_table_mut() {
        table.insert("active", toml_edit::value(name));
    } else {
        return Err(format!("{}: `[presets]` is not a table", path.display()).into());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    atomic_write(&path, &doc.to_string())
}

pub(crate) fn list_preset_names(config_dir: &Path) -> Result<Vec<String>> {
    let dir = presets_dir(config_dir);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in
        std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("cannot read {} entry: {e}", dir.display()))?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.ends_with(".toml") {
            names.push(name.trim_end_matches(".toml").to_string());
        }
    }
    names.sort_unstable();
    Ok(names)
}

pub(crate) fn active_config_path(config_dir: &Path) -> Result<PathBuf> {
    let name = read_active_name(config_dir)?;
    let path = preset_file(config_dir, &name)?;
    if !path.is_file() {
        return Err(format!("preset file does not exist: {}", path.display()).into());
    }
    Ok(path)
}

/// Writes root `config.toml` and a minimal `presets/default.toml` skeleton.
/// Intended for future `smstatus init` (#64); errors if either file exists.
pub(crate) fn create_default_preset(config_dir: &Path) -> Result<()> {
    let program_path = program_config_path(config_dir);
    if program_path.exists() {
        return Err(format!("{} already exists", program_path.display()).into());
    }

    let default_preset = preset_file(config_dir, DEFAULT_PRESET_NAME)?;
    if default_preset.exists() {
        return Err(format!("{} already exists", default_preset.display()).into());
    }

    std::fs::create_dir_all(presets_dir(config_dir))
        .map_err(|e| format!("cannot create {}: {e}", presets_dir(config_dir).display()))?;

    let preset_content = "# bar layout preset\nseparator = \" | \"\nmodules = []\n";
    atomic_write(&default_preset, preset_content)?;

    let program_content = "[presets]\nactive = \"default\"\n";
    if let Some(parent) = program_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    atomic_write(&program_path, program_content)?;

    Ok(())
}

pub(crate) fn copy_preset(config_dir: &Path, from: &str, to: &str) -> Result<()> {
    let src = preset_file(config_dir, from)?;
    let dest = preset_file(config_dir, to)?;
    let bytes = std::fs::read(&src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    atomic_write(
        &dest,
        std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
    )
}

pub(crate) fn remove_preset_file(config_dir: &Path, name: &str) -> Result<()> {
    let path = preset_file(config_dir, name)?;
    if path.is_file() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    pub fn unique_config_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "smstatus-presets-test-{label}-{}-{nanos}-{counter}",
            std::process::id()
        ))
    }

    pub fn write_preset(config_dir: &Path, name: &str, content: &str) {
        let path = preset_file(config_dir, name).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }

    pub fn write_program_config(config_dir: &Path, content: &str) {
        let path = program_config_path(config_dir);
        std::fs::create_dir_all(config_dir).unwrap();
        std::fs::write(&path, content).unwrap();
    }
}
