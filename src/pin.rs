use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::error::Result;
use crate::install::{self, InstallOptions, format_extension_outcome, format_module_outcome};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PinApplyOptions {
    pub allow_insecure_http: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PinApplyOutput {
    pub outcomes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct PinFile {
    #[serde(default)]
    pub module: Vec<PinEntry>,
    #[serde(default)]
    pub extension: Vec<PinEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PinEntry {
    pub source: Option<String>,
    pub sha256: Option<String>,
    pub force: Option<bool>,
    pub name: Option<String>,
    pub version: Option<String>,
}

pub(crate) fn parse_pin_file(text: &str) -> Result<PinFile> {
    toml::from_str(text).map_err(|e| format!("failed to parse pin file: {e}").into())
}

fn resolve_pin_source(entry: &PinEntry) -> Result<String> {
    entry
        .source
        .clone()
        .ok_or_else(|| "pin entry missing required field `source`".into())
}

fn install_options_for_entry(
    entry: &PinEntry,
    global: &PinApplyOptions,
    force: bool,
) -> InstallOptions {
    InstallOptions {
        allow_insecure_http: global.allow_insecure_http,
        expected_sha256: entry.sha256.clone(),
        force,
        expected_name: entry.name.clone(),
        expected_version: entry.version.clone(),
    }
}

fn apply_module_entry(
    modules_dir: &Path,
    entry: &PinEntry,
    global: &PinApplyOptions,
) -> Result<(String, Vec<String>)> {
    let source = resolve_pin_source(entry)?;
    let options = install_options_for_entry(entry, global, false);
    let output = install::install_module_into(modules_dir, &source, &options)?;
    Ok((format_module_outcome(&output.value), output.warnings))
}

fn apply_extension_entry(
    extensions_dir: &Path,
    entry: &PinEntry,
    global: &PinApplyOptions,
) -> Result<(String, Vec<String>)> {
    let source = resolve_pin_source(entry)?;
    let options = install_options_for_entry(entry, global, entry.force.unwrap_or(false));
    let output = install::install_extension_into(extensions_dir, &source, &options)?;
    Ok((format_extension_outcome(&output.value), output.warnings))
}

pub(crate) fn apply_pin_file_in(
    config_dir: &Path,
    path: &Path,
    global: &PinApplyOptions,
) -> Result<PinApplyOutput> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("failed to read pin file `{}`: {e}", path.display()))?;
    let pin_file = parse_pin_file(&text)?;

    let modules_dir = config_dir.join("modules");
    let extensions_dir = config_dir.join("extensions");
    fs::create_dir_all(&modules_dir)
        .map_err(|e| format!("failed to create `{}`: {e}", modules_dir.display()))?;
    fs::create_dir_all(&extensions_dir)
        .map_err(|e| format!("failed to create `{}`: {e}", extensions_dir.display()))?;

    let mut outcomes = Vec::new();
    let mut warnings = Vec::new();
    for entry in &pin_file.module {
        let (outcome, entry_warnings) = apply_module_entry(&modules_dir, entry, global)?;
        warnings.extend(entry_warnings);
        outcomes.push(outcome);
    }
    for entry in &pin_file.extension {
        let (outcome, entry_warnings) = apply_extension_entry(&extensions_dir, entry, global)?;
        warnings.extend(entry_warnings);
        outcomes.push(outcome);
    }
    Ok(PinApplyOutput { outcomes, warnings })
}

pub(crate) fn apply_pin_file(path: &Path, global: &PinApplyOptions) -> Result<PinApplyOutput> {
    apply_pin_file_in(&crate::config::default_config_dir()?, path, global)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_fixtures;
    use crate::install::test_fixtures::{
        pack_minimal_extension_archive, pack_minimal_module_archive,
    };
    use crate::manifest;

    fn pin_file_for(archive: &Path, kind: &str, version: Option<&str>) -> String {
        let source = archive.display().to_string();
        let version_line = version
            .map(|v| format!("\nversion = \"{v}\""))
            .unwrap_or_default();
        format!("[[{kind}]]\nsource = \"{source}\"{version_line}\n")
    }

    fn pin_file_module_and_extension(module_archive: &Path, extension_archive: &Path) -> String {
        format!(
            "[[module]]\nsource = \"{}\"\n\n[[extension]]\nsource = \"{}\"\n",
            module_archive.display(),
            extension_archive.display()
        )
    }

    #[test]
    fn parse_pin_file_accepts_valid_toml() {
        let parsed = parse_pin_file(
            "[[module]]\nsource = \"/tmp/mod.tar.gz\"\nsha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
        )
        .unwrap();
        assert_eq!(parsed.module.len(), 1);
        assert_eq!(parsed.extension.len(), 0);
        assert_eq!(parsed.module[0].source.as_deref(), Some("/tmp/mod.tar.gz"));
    }

    #[test]
    fn parse_pin_file_accepts_empty_tables() {
        let parsed = parse_pin_file("module = []\nextension = []\n").unwrap();
        assert!(parsed.module.is_empty());
        assert!(parsed.extension.is_empty());
    }

    #[test]
    fn parse_pin_file_rejects_invalid_toml() {
        let err = parse_pin_file("not valid toml [[[").unwrap_err();
        assert!(err.to_string().contains("failed to parse pin file"));
    }

    #[test]
    fn parse_pin_file_accepts_empty_file() {
        let parsed = parse_pin_file("").unwrap();
        assert!(parsed.module.is_empty());
        assert!(parsed.extension.is_empty());
    }

    #[test]
    fn apply_pin_file_in_rejects_missing_source() {
        let config_dir = test_fixtures::unique_config_dir("pin-missing-source");
        fs::create_dir_all(&config_dir).unwrap();
        let pin_path = config_dir.join("pins.toml");
        fs::write(&pin_path, "[[module]]\n").unwrap();
        let err =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap_err();
        assert!(err.to_string().contains("missing required field `source`"));
    }

    #[test]
    fn apply_pin_file_in_rejects_name_without_source() {
        let config_dir = test_fixtures::unique_config_dir("pin-name-no-source");
        fs::create_dir_all(&config_dir).unwrap();
        let pin_path = config_dir.join("pins.toml");
        fs::write(
            &pin_path,
            "[[module]]\nname = \"battery\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let err =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap_err();
        assert!(err.to_string().contains("missing required field `source`"));
    }

    #[test]
    fn apply_pin_file_in_installs_then_skips() {
        let config_dir = test_fixtures::unique_config_dir("pin-apply-skip");
        fs::create_dir_all(&config_dir).unwrap();
        let module_archive = pack_minimal_module_archive("widget").unwrap();
        let extension_archive = pack_minimal_extension_archive("probe").unwrap();
        let pin_path = config_dir.join("pins.toml");
        fs::write(
            &pin_path,
            pin_file_module_and_extension(&module_archive, &extension_archive),
        )
        .unwrap();

        let first = apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap();
        assert_eq!(first.outcomes.len(), 2);
        assert!(first.outcomes[0].contains("installed module `widget`"));
        assert!(first.outcomes[1].contains("installed extension `probe`"));
        assert!(
            first.warnings.iter().any(|w| w.contains("native code")),
            "expected extension native-code warning, got {:?}",
            first.warnings
        );
        assert!(
            first
                .warnings
                .iter()
                .any(|w| w.contains("without SHA-256 verification")),
            "expected local-archive warning, got {:?}",
            first.warnings
        );

        let second =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap();
        assert!(second.outcomes[0].contains("module `widget` already installed"));
        assert!(second.outcomes[1].contains("extension `probe` already installed"));

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_file(&module_archive);
        let _ = fs::remove_file(&extension_archive);
    }

    #[test]
    fn apply_pin_file_in_rejects_wrong_version_before_install() {
        let config_dir = test_fixtures::unique_config_dir("pin-wrong-version");
        fs::create_dir_all(&config_dir).unwrap();
        let module_archive = pack_minimal_module_archive("widget").unwrap();
        let pin_path = config_dir.join("pins.toml");
        fs::write(
            &pin_path,
            pin_file_for(&module_archive, "module", Some("9.9.9")),
        )
        .unwrap();

        let err =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap_err();
        assert!(err.to_string().contains("does not match manifest version"));
        assert!(!manifest::module_dir(&config_dir.join("modules"), "widget").exists());

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_file(&module_archive);
    }

    #[test]
    fn apply_pin_file_in_rejects_wrong_name_before_install() {
        let config_dir = test_fixtures::unique_config_dir("pin-wrong-name");
        fs::create_dir_all(&config_dir).unwrap();
        let module_archive = pack_minimal_module_archive("widget").unwrap();
        let pin_path = config_dir.join("pins.toml");
        fs::write(
            &pin_path,
            format!(
                "[[module]]\nsource = \"{}\"\nname = \"other\"\n",
                module_archive.display()
            ),
        )
        .unwrap();

        let err =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap_err();
        assert!(err.to_string().contains("does not match manifest name"));
        assert!(!manifest::module_dir(&config_dir.join("modules"), "widget").exists());

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_file(&module_archive);
    }

    #[test]
    fn apply_pin_file_in_rejects_module_archive_in_extension_table() {
        let config_dir = test_fixtures::unique_config_dir("pin-wrong-kind");
        fs::create_dir_all(&config_dir).unwrap();
        let module_archive = pack_minimal_module_archive("widget").unwrap();
        let pin_path = config_dir.join("pins.toml");
        fs::write(
            &pin_path,
            format!("[[extension]]\nsource = \"{}\"\n", module_archive.display()),
        )
        .unwrap();

        let err =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap_err();
        assert!(
            err.to_string().contains("failed to parse")
                || err.to_string().contains("missing")
                || err.to_string().contains("extension archive")
        );
        assert!(!manifest::extension_dir(&config_dir.join("extensions"), "widget").exists());

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_file(&module_archive);
    }

    #[test]
    fn apply_pin_file_in_force_replaces_same_version_extension() {
        let config_dir = test_fixtures::unique_config_dir("pin-ext-force");
        fs::create_dir_all(&config_dir).unwrap();
        let extension_archive = pack_minimal_extension_archive("probe").unwrap();
        let pin_path = config_dir.join("pins.toml");
        fs::write(
            &pin_path,
            format!(
                "[[extension]]\nsource = \"{}\"\nforce = true\n",
                extension_archive.display()
            ),
        )
        .unwrap();

        let first = apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap();
        assert!(first.outcomes[0].contains("installed extension `probe`"));

        let second =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap();
        assert!(second.outcomes[0].contains("replaced extension `probe`"));

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_file(&extension_archive);
    }

    #[test]
    fn apply_pin_file_in_accepts_sha256_sidecar_for_local_source() {
        let config_dir = test_fixtures::unique_config_dir("pin-sha256-sidecar");
        fs::create_dir_all(&config_dir).unwrap();
        let module_archive = pack_minimal_module_archive("widget").unwrap();
        let hash = {
            use sha2::{Digest, Sha256};
            let mut file = fs::File::open(&module_archive).unwrap();
            let mut hasher = Sha256::new();
            std::io::copy(&mut file, &mut hasher).unwrap();
            format!("{:x}", hasher.finalize())
        };
        fs::write(
            format!("{}.sha256", module_archive.display()),
            format!("{hash}\n"),
        )
        .unwrap();

        let pin_path = config_dir.join("pins.toml");
        fs::write(
            &pin_path,
            format!(
                "[[module]]\nsource = \"{}\"\nsha256 = \"{hash}\"\n",
                module_archive.display()
            ),
        )
        .unwrap();

        let output =
            apply_pin_file_in(&config_dir, &pin_path, &PinApplyOptions::default()).unwrap();
        assert!(output.outcomes[0].contains("installed module `widget`"));
        assert!(manifest::module_dir(&config_dir.join("modules"), "widget").exists());

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_file(&module_archive);
        let _ = fs::remove_file(format!("{}.sha256", module_archive.display()));
    }
}
