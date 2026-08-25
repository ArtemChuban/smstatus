use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::bindings::Metadata;
use crate::error::Result;
use crate::extension::is_safe_extension_name;
use crate::meta::MetadataProbe;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceKind {
    Local,
    Url,
}

/// Local path or downloaded temp file. URL temps are removed on drop.
#[derive(Debug)]
pub(crate) struct ResolvedSource {
    path: PathBuf,
    kind: SourceKind,
    label: PathBuf,
}

impl ResolvedSource {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn label(&self) -> &Path {
        &self.label
    }
}

impl Drop for ResolvedSource {
    fn drop(&mut self) {
        if self.kind == SourceKind::Url {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleInstallAction {
    Fresh,
    Skip,
    Replace,
}

#[derive(Debug)]
pub(crate) enum ModuleInstallOutcome {
    Fresh { kind: String, metadata: Metadata },
    Skip { kind: String, metadata: Metadata },
    Replace { kind: String, metadata: Metadata },
}

pub(crate) fn decide_module_install(
    installed: Option<&Metadata>,
    candidate: &Metadata,
) -> ModuleInstallAction {
    match installed {
        None => ModuleInstallAction::Fresh,
        Some(existing) if existing.version == candidate.version => ModuleInstallAction::Skip,
        Some(_) => ModuleInstallAction::Replace,
    }
}

pub(crate) fn artifact_stem(path: &Path) -> Result<String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("cannot determine artifact name from `{}`", path.display()))?;
    if !is_safe_extension_name(stem) {
        return Err(format!("invalid artifact name `{stem}`").into());
    }
    Ok(stem.to_string())
}

fn has_wasm_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("wasm")
}

fn url_basename(source: &str) -> &str {
    let without_query = source.split('?').next().unwrap_or(source);
    without_query.rsplit('/').next().unwrap_or(without_query)
}

pub(crate) fn resolve_source(source: &str) -> Result<ResolvedSource> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let basename = url_basename(source);
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(DOWNLOAD_TIMEOUT))
                .build(),
        );
        let bytes = {
            let mut response = agent
                .get(source)
                .call()
                .map_err(|e| format!("failed to download `{source}`: {e}"))?;
            response
                .body_mut()
                .read_to_vec()
                .map_err(|e| format!("failed to read download body for `{source}`: {e}"))?
        };
        let label = if basename.is_empty() {
            PathBuf::from(format!("smstatus-download-{}.bin", std::process::id()))
        } else {
            PathBuf::from(basename)
        };
        let file_name = if basename.is_empty() {
            format!("smstatus-download-{}.bin", std::process::id())
        } else {
            format!("smstatus-download-{}-{basename}", std::process::id())
        };
        let temp_path = std::env::temp_dir().join(file_name);
        fs::write(&temp_path, bytes).map_err(|e| {
            format!(
                "failed to write temp download `{}`: {e}",
                temp_path.display()
            )
        })?;
        Ok(ResolvedSource {
            path: temp_path,
            kind: SourceKind::Url,
            label,
        })
    } else {
        let path = PathBuf::from(source);
        if !path.exists() {
            return Err(format!("source path `{}` does not exist", path.display()).into());
        }
        Ok(ResolvedSource {
            label: path.clone(),
            path,
            kind: SourceKind::Local,
        })
    }
}

fn place_module_bytes(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create modules dir `{}`: {e}", parent.display()))?;
    }
    fs::copy(src, dest).map_err(|e| {
        format!(
            "failed to install module from `{}` to `{}`: {e}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn format_meta(metadata: &Metadata) -> String {
    format!(
        "{} {} by {}",
        metadata.display_name, metadata.version, metadata.author
    )
}

pub(crate) fn format_module_outcome(outcome: &ModuleInstallOutcome) -> String {
    match outcome {
        ModuleInstallOutcome::Fresh { kind, metadata } => {
            format!("installed module `{kind}` ({})", format_meta(metadata))
        }
        ModuleInstallOutcome::Skip { kind, metadata } => {
            format!(
                "module `{kind}` already installed ({})",
                format_meta(metadata)
            )
        }
        ModuleInstallOutcome::Replace { kind, metadata } => {
            format!("replaced module `{kind}` ({})", format_meta(metadata))
        }
    }
}

pub(crate) fn install_module_into(
    modules_dir: &Path,
    source: &str,
) -> Result<ModuleInstallOutcome> {
    let resolved = resolve_source(source)?;
    let check_path = resolved.label();
    if !has_wasm_extension(check_path) {
        return Err(format!(
            "module source must be a `.wasm` file, got `{}`",
            check_path.display()
        )
        .into());
    }
    let kind = artifact_stem(check_path)?;
    let dest = modules_dir.join(format!("{kind}.wasm"));

    let probe = MetadataProbe::new()?;
    let candidate = probe.read_path(resolved.path(), false)?;

    if dest.exists() {
        match probe.read(modules_dir, &kind) {
            Ok(installed) => match decide_module_install(Some(&installed), &candidate) {
                ModuleInstallAction::Skip => {
                    return Ok(ModuleInstallOutcome::Skip {
                        kind,
                        metadata: installed,
                    });
                }
                ModuleInstallAction::Replace => {
                    place_module_bytes(resolved.path(), &dest)?;
                    return Ok(ModuleInstallOutcome::Replace {
                        kind,
                        metadata: candidate,
                    });
                }
                ModuleInstallAction::Fresh => {
                    unreachable!("dest exists implies installed is Some")
                }
            },
            Err(_) => {
                place_module_bytes(resolved.path(), &dest)?;
                return Ok(ModuleInstallOutcome::Replace {
                    kind,
                    metadata: candidate,
                });
            }
        }
    }

    place_module_bytes(resolved.path(), &dest)?;
    Ok(ModuleInstallOutcome::Fresh {
        kind,
        metadata: candidate,
    })
}

pub(crate) fn install_module(source: &str) -> Result<ModuleInstallOutcome> {
    let modules_dir = crate::config::default_config_dir()?.join("modules");
    install_module_into(&modules_dir, source)
}

#[derive(Debug)]
pub(crate) enum ExtensionInstallOutcome {
    Fresh { name: String },
    Replace { name: String },
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|e| format!("failed to read permissions for `{}`: {e}", path.display()))?
            .permissions();
        perms.set_mode(perms.mode() | 0o111);
        fs::set_permissions(path, perms)
            .map_err(|e| format!("failed to set executable bit on `{}`: {e}", path.display()))?;
    }
    let _ = path;
    Ok(())
}

pub(crate) fn format_extension_outcome(outcome: &ExtensionInstallOutcome) -> String {
    match outcome {
        ExtensionInstallOutcome::Fresh { name } => {
            format!("installed extension `{name}`")
        }
        ExtensionInstallOutcome::Replace { name } => {
            format!("replaced extension `{name}`")
        }
    }
}

pub(crate) fn install_extension_into(
    extensions_dir: &Path,
    source: &str,
) -> Result<ExtensionInstallOutcome> {
    let resolved = resolve_source(source)?;
    let name = artifact_stem(resolved.label())?;
    let dest = extensions_dir.join(&name);
    let replaced = dest.exists();

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create extensions dir `{}`: {e}",
                parent.display()
            )
        })?;
    }
    fs::copy(resolved.path(), &dest).map_err(|e| {
        format!(
            "failed to install extension from `{}` to `{}`: {e}",
            resolved.path().display(),
            dest.display()
        )
    })?;
    set_executable(&dest)?;

    if replaced {
        Ok(ExtensionInstallOutcome::Replace { name })
    } else {
        Ok(ExtensionInstallOutcome::Fresh { name })
    }
}

pub(crate) fn install_extension(source: &str) -> Result<ExtensionInstallOutcome> {
    let extensions_dir = crate::config::default_config_dir()?.join("extensions");
    install_extension_into(&extensions_dir, source)
}

pub(crate) fn list_modules_in(modules_dir: &Path) -> Result<Vec<String>> {
    let kinds = crate::config::discover_module_kinds(modules_dir)?;
    let probe = MetadataProbe::new()?;
    let mut lines = Vec::with_capacity(kinds.len());
    for kind in kinds {
        match probe.read(modules_dir, &kind) {
            Ok(metadata) => lines.push(format!(
                "{kind}\t{}\t{}\t{}",
                metadata.display_name, metadata.version, metadata.author
            )),
            Err(_) => lines.push(kind),
        }
    }
    Ok(lines)
}

pub(crate) fn list_modules() -> Result<Vec<String>> {
    let modules_dir = crate::config::default_config_dir()?.join("modules");
    list_modules_in(&modules_dir)
}

pub(crate) fn remove_module_from(modules_dir: &Path, name: &str) -> Result<()> {
    if !is_safe_extension_name(name) {
        return Err(format!("invalid module name `{name}`").into());
    }
    let path = modules_dir.join(format!("{name}.wasm"));
    if !path.exists() {
        return Err(format!("module `{name}` is not installed").into());
    }
    fs::remove_file(&path).map_err(|e| format!("failed to remove module `{name}`: {e}"))?;
    Ok(())
}

pub(crate) fn remove_module(name: &str) -> Result<()> {
    let modules_dir = crate::config::default_config_dir()?.join("modules");
    remove_module_from(&modules_dir, name)
}

pub(crate) fn list_extensions_in(extensions_dir: &Path) -> Result<Vec<String>> {
    if !extensions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(extensions_dir)
        .map_err(|e| format!("cannot read {}: {e}", extensions_dir.display()))?
    {
        let entry =
            entry.map_err(|e| format!("cannot read entry in {}: {e}", extensions_dir.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_safe_extension_name(name) {
            names.push(name.to_string());
        }
    }
    names.sort_unstable();
    Ok(names)
}

pub(crate) fn list_extensions() -> Result<Vec<String>> {
    let extensions_dir = crate::config::default_config_dir()?.join("extensions");
    list_extensions_in(&extensions_dir)
}

pub(crate) fn remove_extension_from(extensions_dir: &Path, name: &str) -> Result<()> {
    if !is_safe_extension_name(name) {
        return Err(format!("invalid extension name `{name}`").into());
    }
    let path = extensions_dir.join(name);
    if !path.exists() {
        return Err(format!("extension `{name}` is not installed").into());
    }
    fs::remove_file(&path).map_err(|e| format!("failed to remove extension `{name}`: {e}"))?;
    Ok(())
}

pub(crate) fn remove_extension(name: &str) -> Result<()> {
    let extensions_dir = crate::config::default_config_dir()?.join("extensions");
    remove_extension_from(&extensions_dir, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn meta(version: &str) -> Metadata {
        Metadata {
            display_name: "Test".to_string(),
            version: version.to_string(),
            author: "Author".to_string(),
        }
    }

    fn temp_modules_dir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "smstatus-install-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn find_or_build_guest_wasm(package: &str) -> PathBuf {
        static BATTERY: OnceLock<PathBuf> = OnceLock::new();
        static CPU: OnceLock<PathBuf> = OnceLock::new();

        let cache = match package {
            "battery" => &BATTERY,
            "cpu" => &CPU,
            other => panic!("unsupported guest fixture `{other}`"),
        };

        cache
            .get_or_init(|| {
                let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let wasm = manifest_dir
                    .join("target/wasm32-wasip2/debug")
                    .join(format!("{package}.wasm"));
                if wasm.exists() {
                    return wasm;
                }

                let status = std::process::Command::new(env!("CARGO"))
                    .current_dir(&manifest_dir)
                    .args([
                        "build",
                        "-p",
                        package,
                        "--target",
                        "wasm32-wasip2",
                    ])
                    .status()
                    .unwrap_or_else(|e| panic!("failed to spawn cargo build -p {package}: {e}"));
                assert!(
                    status.success() && wasm.exists(),
                    "guest wasm missing at {}; build with `cargo build -p {package} --target wasm32-wasip2`",
                    wasm.display()
                );
                wasm
            })
            .clone()
    }

    #[test]
    fn artifact_stem_rejects_unsafe_names() {
        assert!(artifact_stem(Path::new("..wasm")).is_err());
        assert!(artifact_stem(Path::new(".")).is_err());
        assert_eq!(artifact_stem(Path::new("cpu.wasm")).unwrap(), "cpu");
    }

    #[test]
    fn decide_module_install_fresh_when_missing() {
        let candidate = meta("1.0.0");
        assert_eq!(
            decide_module_install(None, &candidate),
            ModuleInstallAction::Fresh
        );
    }

    #[test]
    fn decide_module_install_skip_when_same_version() {
        let installed = meta("1.0.0");
        let candidate = meta("1.0.0");
        assert_eq!(
            decide_module_install(Some(&installed), &candidate),
            ModuleInstallAction::Skip
        );
    }

    #[test]
    fn decide_module_install_replace_when_different_version() {
        let installed = meta("1.0.0");
        let candidate = meta("2.0.0");
        assert_eq!(
            decide_module_install(Some(&installed), &candidate),
            ModuleInstallAction::Replace
        );
    }

    #[test]
    fn install_module_into_fresh_and_skip_same_version() {
        let modules_dir = temp_modules_dir("fresh-skip");
        let battery = find_or_build_guest_wasm("battery");

        let first = install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();
        match &first {
            ModuleInstallOutcome::Fresh { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Fresh, got {other:?}"),
        }
        assert!(modules_dir.join("battery.wasm").exists());

        let second = install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();
        match second {
            ModuleInstallOutcome::Skip { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    fn guest_wasm_with_patched_version(package: &str, new_version: &[u8; 5]) -> PathBuf {
        let original = find_or_build_guest_wasm(package);
        let bytes = fs::read(&original).unwrap();
        let Some(idx) = bytes.windows(5).position(|w| w == b"0.1.0") else {
            panic!("{} wasm missing version string 0.1.0 to patch", package);
        };
        let mut patched = bytes;
        patched[idx..idx + 5].copy_from_slice(new_version);
        let dir = temp_modules_dir(&format!("patched-{package}"));
        let path = dir.join(format!("{package}.wasm"));
        fs::write(&path, patched).unwrap();
        path
    }

    #[test]
    fn install_module_into_replaces_different_version() {
        let modules_dir = temp_modules_dir("replace-version");
        let battery = find_or_build_guest_wasm("battery");
        install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();

        let newer = guest_wasm_with_patched_version("battery", b"9.9.9");
        let outcome = install_module_into(&modules_dir, newer.to_str().unwrap()).unwrap();
        match outcome {
            ModuleInstallOutcome::Replace { kind, metadata } => {
                assert_eq!(kind, "battery");
                assert_eq!(metadata.version, "9.9.9");
            }
            other => panic!("expected Replace, got {other:?}"),
        }
        assert_eq!(
            fs::read(modules_dir.join("battery.wasm")).unwrap(),
            fs::read(&newer).unwrap()
        );
    }

    #[test]
    fn install_module_into_replaces_unreadable_installed() {
        let modules_dir = temp_modules_dir("replace-corrupt");
        let battery = find_or_build_guest_wasm("battery");
        install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();

        let dest = modules_dir.join("battery.wasm");
        fs::write(&dest, b"not a wasm module").unwrap();

        let outcome = install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();
        match outcome {
            ModuleInstallOutcome::Replace { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Replace, got {other:?}"),
        }
        assert_eq!(fs::read(&dest).unwrap(), fs::read(&battery).unwrap());
    }

    #[test]
    fn place_module_bytes_overwrites_dest_contents() {
        let modules_dir = temp_modules_dir("replace-bytes");
        let battery = find_or_build_guest_wasm("battery");
        let cpu = find_or_build_guest_wasm("cpu");
        let dest = modules_dir.join("battery.wasm");

        place_module_bytes(&battery, &dest).unwrap();
        let before = fs::read(&dest).unwrap();
        assert_eq!(before, fs::read(&battery).unwrap());

        place_module_bytes(&cpu, &dest).unwrap();
        let after = fs::read(&dest).unwrap();
        assert_eq!(after, fs::read(&cpu).unwrap());
        assert_ne!(after, before);
    }

    #[test]
    fn resolve_source_rejects_missing_local_path() {
        let err = resolve_source("/no/such/smstatus-module.wasm").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    fn find_or_build_echo() -> PathBuf {
        static ECHO: OnceLock<PathBuf> = OnceLock::new();
        ECHO.get_or_init(|| {
            if let Ok(path) = std::env::var("CARGO_BIN_EXE_echo") {
                let path = PathBuf::from(path);
                if path.exists() {
                    return path;
                }
            }

            let mut dir = std::env::current_exe().unwrap();
            dir.pop();
            if dir.ends_with("deps") {
                dir.pop();
            }
            let bin = dir.join("echo");
            if bin.exists() {
                return bin;
            }

            let target_dir = dir.parent().expect("debug dir has a target-dir parent");
            let status = std::process::Command::new(env!("CARGO"))
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .args(["build", "-p", "echo", "--target-dir"])
                .arg(target_dir)
                .status()
                .expect("failed to spawn cargo build -p echo");
            assert!(
                status.success() && bin.exists(),
                "echo fixture missing at {}; build with `cargo build -p echo --target-dir {}`",
                bin.display(),
                target_dir.display()
            );
            bin
        })
        .clone()
    }

    #[test]
    fn install_extension_into_places_binary_where_is_installed_sees_it() {
        use crate::extension::ExtensionRegistry;

        let base = temp_modules_dir("ext-install");
        let extensions_dir = base.join("extensions");
        let echo = find_or_build_echo();

        let outcome = install_extension_into(&extensions_dir, echo.to_str().unwrap()).unwrap();
        match outcome {
            ExtensionInstallOutcome::Fresh { name } => assert_eq!(name, "echo"),
            other => panic!("expected Fresh, got {other:?}"),
        }

        let registry = ExtensionRegistry::new(extensions_dir.clone(), base.join("sockets"));
        assert!(registry.is_installed("echo"));

        let again = install_extension_into(&extensions_dir, echo.to_str().unwrap()).unwrap();
        match again {
            ExtensionInstallOutcome::Replace { name } => assert_eq!(name, "echo"),
            other => panic!("expected Replace, got {other:?}"),
        }
        assert!(registry.is_installed("echo"));
    }

    #[test]
    fn list_and_remove_module_round_trip() {
        let modules_dir = temp_modules_dir("mod-list-rm");
        let battery = find_or_build_guest_wasm("battery");
        install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();

        let listed = list_modules_in(&modules_dir).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].starts_with("battery\t"));

        remove_module_from(&modules_dir, "battery").unwrap();
        assert!(!modules_dir.join("battery.wasm").exists());
        assert!(list_modules_in(&modules_dir).unwrap().is_empty());
        let err = remove_module_from(&modules_dir, "battery").unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn list_and_remove_extension_round_trip() {
        let base = temp_modules_dir("ext-list-rm");
        let extensions_dir = base.join("extensions");
        let echo = find_or_build_echo();
        install_extension_into(&extensions_dir, echo.to_str().unwrap()).unwrap();

        assert_eq!(
            list_extensions_in(&extensions_dir).unwrap(),
            vec!["echo".to_string()]
        );

        remove_extension_from(&extensions_dir, "echo").unwrap();
        assert!(!extensions_dir.join("echo").exists());
        assert!(list_extensions_in(&extensions_dir).unwrap().is_empty());
        let err = remove_extension_from(&extensions_dir, "echo").unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }
}
