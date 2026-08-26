use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use flate2::read::GzDecoder;

use crate::error::Result;
use crate::extension::is_safe_extension_name;
use crate::manifest;
use crate::manifest::Metadata;
use crate::meta;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceKind {
    Local,
    Url,
}

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

fn has_tar_gz_extension(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with(".tar.gz") || name.ends_with(".tgz")
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
        let file_name = if basename.is_empty() {
            format!("smstatus-download-{}.tar.gz", std::process::id())
        } else {
            format!("smstatus-download-{}-{basename}", std::process::id())
        };
        let label = if basename.is_empty() {
            PathBuf::from(&file_name)
        } else {
            PathBuf::from(basename)
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

fn unique_temp_dir(label: &str) -> Result<PathBuf> {
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "smstatus-install-{label}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create temp dir `{}`: {e}", dir.display()))?;
    Ok(dir)
}

struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn unpack_archive(archive: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)
        .map_err(|e| format!("failed to create unpack dir `{}`: {e}", dest.display()))?;
    let file = fs::File::open(archive)
        .map_err(|e| format!("failed to open archive `{}`: {e}", archive.display()))?;
    let decoder = GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    tar.unpack(dest)
        .map_err(|e| format!("failed to unpack archive `{}`: {e}", archive.display()))?;
    Ok(())
}

fn package_root(unpacked: &Path) -> Result<PathBuf> {
    if unpacked.join("manifest.toml").is_file() {
        return Ok(unpacked.to_path_buf());
    }
    let mut children = Vec::new();
    for entry in fs::read_dir(unpacked)
        .map_err(|e| format!("cannot read unpack dir `{}`: {e}", unpacked.display()))?
    {
        let entry =
            entry.map_err(|e| format!("cannot read entry in `{}`: {e}", unpacked.display()))?;
        children.push(entry.path());
    }
    if children.len() == 1 && children[0].is_dir() && children[0].join("manifest.toml").is_file() {
        return Ok(children[0].clone());
    }
    Err(
        "archive must contain manifest.toml at its root or inside a single top-level directory"
            .into(),
    )
}

fn require_module_files(root: &Path) -> Result<()> {
    if !root.join("manifest.toml").is_file() {
        return Err("module archive missing `manifest.toml`".into());
    }
    if !root.join("module.wasm").is_file() {
        return Err("module archive missing `module.wasm`".into());
    }
    Ok(())
}

fn require_extension_files(root: &Path) -> Result<()> {
    if !root.join("manifest.toml").is_file() {
        return Err("extension archive missing `manifest.toml`".into());
    }
    if !root.join("extension").is_file() {
        return Err("extension archive missing `extension` binary".into());
    }
    Ok(())
}

fn copy_install_file(src: &Path, dest: &Path) -> Result<()> {
    let file_meta = fs::symlink_metadata(src)
        .map_err(|e| format!("failed to stat `{}`: {e}", src.display()))?;
    if file_meta.file_type().is_symlink() {
        return Err(format!("refusing to install symlink `{}`", src.display()).into());
    }
    if !file_meta.is_file() {
        return Err(format!("expected a regular file at `{}`", src.display()).into());
    }
    fs::copy(src, dest).map_err(|e| {
        format!(
            "failed to copy `{}` to `{}`: {e}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn copy_dir_all(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).map_err(|e| format!("failed to create `{}`: {e}", dest.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("cannot read `{}`: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("cannot read entry in `{}`: {e}", src.display()))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let file_meta = fs::symlink_metadata(&from)
            .map_err(|e| format!("failed to stat `{}`: {e}", from.display()))?;
        if file_meta.file_type().is_symlink() {
            return Err(format!("refusing to copy symlink `{}`", from.display()).into());
        }
        if file_meta.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if file_meta.is_file() {
            fs::copy(&from, &to).map_err(|e| {
                format!(
                    "failed to copy `{}` to `{}`: {e}",
                    from.display(),
                    to.display()
                )
            })?;
        } else {
            return Err(format!("unsupported file type at `{}`", from.display()).into());
        }
    }
    Ok(())
}

fn move_or_copy_dir(src: &Path, dest: &Path) -> Result<()> {
    if fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    copy_dir_all(src, dest)?;
    let _ = fs::remove_dir_all(src);
    Ok(())
}

fn unique_backup_path(dest: &Path) -> Result<PathBuf> {
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let parent = dest
        .parent()
        .ok_or_else(|| format!("package path `{}` has no parent", dest.display()))?;
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("package path `{}` has no file name", dest.display()))?;
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".{name}.bak-{}-{id}", std::process::id())))
}

fn place_package_dir(src: &Path, dest: &Path) -> Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("package path `{}` has no parent", dest.display()))?;
    fs::create_dir_all(parent).map_err(|e| {
        format!(
            "failed to create package parent `{}`: {e}",
            parent.display()
        )
    })?;

    if !dest.exists() {
        return move_or_copy_dir(src, dest);
    }

    let backup = unique_backup_path(dest)?;
    fs::rename(dest, &backup).map_err(|e| {
        format!(
            "failed to move aside existing package `{}`: {e}",
            dest.display()
        )
    })?;

    match move_or_copy_dir(src, dest) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(err) => {
            let _ = fs::remove_dir_all(dest);
            if fs::rename(&backup, dest).is_err() {
                let _ = copy_dir_all(&backup, dest);
            }
            Err(err)
        }
    }
}

fn stage_module_package(root: &Path) -> Result<(ScratchDir, PathBuf)> {
    let scratch = ScratchDir(unique_temp_dir("stage")?);
    let staged = scratch.0.join("pkg");
    fs::create_dir_all(&staged)
        .map_err(|e| format!("failed to create stage dir `{}`: {e}", staged.display()))?;
    copy_install_file(&root.join("manifest.toml"), &staged.join("manifest.toml"))?;
    copy_install_file(&root.join("module.wasm"), &staged.join("module.wasm"))?;
    Ok((scratch, staged))
}

fn stage_extension_package(root: &Path) -> Result<(ScratchDir, PathBuf)> {
    let scratch = ScratchDir(unique_temp_dir("stage")?);
    let staged = scratch.0.join("pkg");
    fs::create_dir_all(&staged)
        .map_err(|e| format!("failed to create stage dir `{}`: {e}", staged.display()))?;
    copy_install_file(&root.join("manifest.toml"), &staged.join("manifest.toml"))?;
    copy_install_file(&root.join("extension"), &staged.join("extension"))?;
    Ok((scratch, staged))
}

pub(crate) fn install_module_into(
    modules_dir: &Path,
    source: &str,
) -> Result<ModuleInstallOutcome> {
    let resolved = resolve_source(source)?;
    let check_path = resolved.label();
    if !has_tar_gz_extension(check_path) {
        return Err(format!(
            "module source must be a `.tar.gz` archive, got `{}`",
            check_path.display()
        )
        .into());
    }

    let unpack = ScratchDir(unique_temp_dir("mod-unpack")?);
    unpack_archive(resolved.path(), &unpack.0)?;
    let root = package_root(&unpack.0)?;
    require_module_files(&root)?;
    let manifest = manifest::read_module_manifest_from(&root.join("manifest.toml"))?;
    let kind = manifest.name.clone();
    let candidate = manifest.to_metadata();
    let dest = manifest::module_dir(modules_dir, &kind);

    if dest.exists() {
        match manifest::read_module_manifest(modules_dir, &kind) {
            Ok(installed_manifest) => {
                let installed = installed_manifest.to_metadata();
                match decide_module_install(Some(&installed), &candidate) {
                    ModuleInstallAction::Skip => {
                        return Ok(ModuleInstallOutcome::Skip {
                            kind,
                            metadata: installed,
                        });
                    }
                    ModuleInstallAction::Replace => {
                        let (_scratch, staged) = stage_module_package(&root)?;
                        place_package_dir(&staged, &dest)?;
                        return Ok(ModuleInstallOutcome::Replace {
                            kind,
                            metadata: candidate,
                        });
                    }
                    ModuleInstallAction::Fresh => {
                        unreachable!("dest exists implies installed is Some")
                    }
                }
            }
            Err(_) => {
                let (_scratch, staged) = stage_module_package(&root)?;
                place_package_dir(&staged, &dest)?;
                return Ok(ModuleInstallOutcome::Replace {
                    kind,
                    metadata: candidate,
                });
            }
        }
    }

    let (_scratch, staged) = stage_module_package(&root)?;
    place_package_dir(&staged, &dest)?;
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
    let check_path = resolved.label();
    if !has_tar_gz_extension(check_path) {
        return Err(format!(
            "extension source must be a `.tar.gz` archive, got `{}`",
            check_path.display()
        )
        .into());
    }

    let unpack = ScratchDir(unique_temp_dir("ext-unpack")?);
    unpack_archive(resolved.path(), &unpack.0)?;
    let root = package_root(&unpack.0)?;
    require_extension_files(&root)?;
    let manifest = manifest::read_extension_manifest_from(&root.join("manifest.toml"))?;
    let name = manifest.name.clone();
    let dest = manifest::extension_dir(extensions_dir, &name);
    let replaced = dest.exists();

    let (_scratch, staged) = stage_extension_package(&root)?;
    place_package_dir(&staged, &dest)?;
    set_executable(&manifest::extension_binary_path(extensions_dir, &name))?;
    let _ = manifest::read_extension_manifest(extensions_dir, &name)?;

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
    let mut lines = Vec::with_capacity(kinds.len());
    for kind in kinds {
        match meta::read(modules_dir, &kind) {
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
    let path = manifest::module_dir(modules_dir, name);
    if !path.exists() {
        return Err(format!("module `{name}` is not installed").into());
    }
    fs::remove_dir_all(&path).map_err(|e| format!("failed to remove module `{name}`: {e}"))?;
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
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_safe_extension_name(name)
            && manifest::extension_manifest_path(extensions_dir, name).is_file()
            && manifest::extension_binary_path(extensions_dir, name).is_file()
        {
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
    let path = manifest::extension_dir(extensions_dir, name);
    if !path.exists() {
        return Err(format!("extension `{name}` is not installed").into());
    }
    fs::remove_dir_all(&path).map_err(|e| format!("failed to remove extension `{name}`: {e}"))?;
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

    use flate2::Compression;
    use flate2::write::GzEncoder;

    fn meta(version: &str) -> Metadata {
        Metadata {
            display_name: "Test".to_string(),
            version: version.to_string(),
            author: "Author".to_string(),
        }
    }

    fn temp_modules_dir(label: &str) -> PathBuf {
        let dir = unique_temp_dir(label).unwrap();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_tar_gz(staging: &Path, archive: &Path) -> Result<()> {
        let file = fs::File::create(archive)
            .map_err(|e| format!("failed to create archive `{}`: {e}", archive.display()))?;
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        builder
            .append_dir_all(".", staging)
            .map_err(|e| format!("failed to append staging dir: {e}"))?;
        builder
            .finish()
            .map_err(|e| format!("failed to finish archive: {e}"))?;
        Ok(())
    }

    fn module_manifest_toml(
        name: &str,
        display_name: &str,
        version: &str,
        required_extensions: &[&str],
        permissions: &str,
    ) -> String {
        let exts = required_extensions
            .iter()
            .map(|e| format!("{{ name = \"{e}\", version = {{ major = 0, minor = 1 }} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "name = \"{name}\"\ndisplay_name = \"{display_name}\"\nversion = \"{version}\"\nauthor = \"ArtemChuban\"\nmodules-api = {{ major = 0, minor = 1 }}\nrequired_extensions = [{exts}]\n{permissions}"
        )
    }

    fn extension_manifest_toml(name: &str, version: &str) -> String {
        format!(
            "name = \"{name}\"\nversion = \"{version}\"\nauthor = \"ArtemChuban\"\nextensions-api = {{ major = 0, minor = 1 }}\n"
        )
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

    fn pack_module_archive(package: &str, version: &str) -> PathBuf {
        let (display_name, required_extensions, permissions): (&str, &[&str], &str) = match package
        {
            "battery" => (
                "Battery",
                &["fs"],
                "\n[[permissions]]\nextension = \"fs\"\nmethod = \"read\"\npath_prefixes = [\"/sys/class/power_supply/\"]\n",
            ),
            "cpu" => (
                "CPU",
                &["fs"],
                "\n[[permissions]]\nextension = \"fs\"\nmethod = \"read\"\npath_prefixes = [\"/proc/\"]\n",
            ),
            other => panic!("unsupported guest fixture `{other}`"),
        };
        let wasm = find_or_build_guest_wasm(package);
        let staging = temp_modules_dir(&format!("pack-{package}-{version}"));
        fs::write(
            staging.join("manifest.toml"),
            module_manifest_toml(
                package,
                display_name,
                version,
                required_extensions,
                permissions,
            ),
        )
        .unwrap();
        fs::copy(&wasm, staging.join("module.wasm")).unwrap();
        let archive = PathBuf::from(format!("{}.tar.gz", staging.display()));
        write_tar_gz(&staging, &archive).unwrap();
        archive
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

    fn pack_echo_archive() -> PathBuf {
        let echo = find_or_build_echo();
        let staging = temp_modules_dir("pack-echo");
        fs::write(
            staging.join("manifest.toml"),
            extension_manifest_toml("echo", "0.1.0"),
        )
        .unwrap();
        fs::copy(&echo, staging.join("extension")).unwrap();
        let archive = PathBuf::from(format!("{}.tar.gz", staging.display()));
        write_tar_gz(&staging, &archive).unwrap();
        archive
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
        let archive = pack_module_archive("battery", "0.1.0");

        let first = install_module_into(&modules_dir, archive.to_str().unwrap()).unwrap();
        match &first {
            ModuleInstallOutcome::Fresh { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Fresh, got {other:?}"),
        }
        assert!(manifest::module_manifest_path(&modules_dir, "battery").exists());
        assert!(manifest::module_wasm_path(&modules_dir, "battery").exists());

        let second = install_module_into(&modules_dir, archive.to_str().unwrap()).unwrap();
        match second {
            ModuleInstallOutcome::Skip { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn install_module_into_replaces_different_version() {
        let modules_dir = temp_modules_dir("replace-version");
        let battery = pack_module_archive("battery", "0.1.0");
        install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();

        let newer = pack_module_archive("battery", "9.9.9");
        let outcome = install_module_into(&modules_dir, newer.to_str().unwrap()).unwrap();
        match outcome {
            ModuleInstallOutcome::Replace { kind, metadata } => {
                assert_eq!(kind, "battery");
                assert_eq!(metadata.version, "9.9.9");
            }
            other => panic!("expected Replace, got {other:?}"),
        }
        let installed = manifest::read_module_manifest(&modules_dir, "battery").unwrap();
        assert_eq!(installed.version, "9.9.9");
    }

    #[test]
    fn install_module_into_replaces_unreadable_installed() {
        let modules_dir = temp_modules_dir("replace-corrupt");
        let battery = pack_module_archive("battery", "0.1.0");
        install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();

        fs::write(
            manifest::module_manifest_path(&modules_dir, "battery"),
            b"not a toml manifest",
        )
        .unwrap();

        let outcome = install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();
        match outcome {
            ModuleInstallOutcome::Replace { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Replace, got {other:?}"),
        }
        let installed = manifest::read_module_manifest(&modules_dir, "battery").unwrap();
        assert_eq!(installed.version, "0.1.0");
    }

    #[test]
    fn install_module_into_rejects_bare_wasm() {
        let modules_dir = temp_modules_dir("reject-wasm");
        let wasm = find_or_build_guest_wasm("battery");
        let err = install_module_into(&modules_dir, wasm.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains(".tar.gz"));
    }

    #[test]
    fn resolve_source_rejects_missing_local_path() {
        let err = resolve_source("/no/such/smstatus-module.tar.gz").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn install_extension_into_places_binary_where_is_installed_sees_it() {
        use crate::extension::ExtensionRegistry;

        let base = temp_modules_dir("ext-install");
        let extensions_dir = base.join("extensions");
        let archive = pack_echo_archive();

        let outcome = install_extension_into(&extensions_dir, archive.to_str().unwrap()).unwrap();
        match outcome {
            ExtensionInstallOutcome::Fresh { name } => assert_eq!(name, "echo"),
            other => panic!("expected Fresh, got {other:?}"),
        }
        assert!(manifest::extension_manifest_path(&extensions_dir, "echo").exists());
        assert!(manifest::extension_binary_path(&extensions_dir, "echo").exists());

        let registry = ExtensionRegistry::new(extensions_dir.clone(), base.join("sockets"));
        assert!(registry.is_installed("echo"));

        let again = install_extension_into(&extensions_dir, archive.to_str().unwrap()).unwrap();
        match again {
            ExtensionInstallOutcome::Replace { name } => assert_eq!(name, "echo"),
            other => panic!("expected Replace, got {other:?}"),
        }
        assert!(registry.is_installed("echo"));
    }

    #[test]
    fn list_and_remove_module_round_trip() {
        let modules_dir = temp_modules_dir("mod-list-rm");
        let battery = pack_module_archive("battery", "0.1.0");
        install_module_into(&modules_dir, battery.to_str().unwrap()).unwrap();

        let listed = list_modules_in(&modules_dir).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].starts_with("battery\t"));

        remove_module_from(&modules_dir, "battery").unwrap();
        assert!(!manifest::module_dir(&modules_dir, "battery").exists());
        assert!(list_modules_in(&modules_dir).unwrap().is_empty());
        let err = remove_module_from(&modules_dir, "battery").unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn list_and_remove_extension_round_trip() {
        let base = temp_modules_dir("ext-list-rm");
        let extensions_dir = base.join("extensions");
        let archive = pack_echo_archive();
        install_extension_into(&extensions_dir, archive.to_str().unwrap()).unwrap();

        assert_eq!(
            list_extensions_in(&extensions_dir).unwrap(),
            vec!["echo".to_string()]
        );

        remove_extension_from(&extensions_dir, "echo").unwrap();
        assert!(!manifest::extension_dir(&extensions_dir, "echo").exists());
        assert!(list_extensions_in(&extensions_dir).unwrap().is_empty());
        let err = remove_extension_from(&extensions_dir, "echo").unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }
}
