use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use flate2::read::GzDecoder;

use sha2::{Digest, Sha256};

use crate::control::{self, NotifyOutcome};
use crate::error::Result;
use crate::extension::is_safe_extension_name;
use crate::manifest;
use crate::manifest::Metadata;
use crate::meta;
use crate::reload::ReloadRequest;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_UNPACKED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;

#[cfg(test)]
pub(crate) mod test_fixtures;

#[derive(Debug, Clone, Default)]
pub(crate) struct InstallOptions {
    pub allow_insecure_http: bool,
    pub expected_sha256: Option<String>,
    pub force: bool,
}

#[derive(Debug)]
pub(crate) struct InstallOutput<T> {
    pub value: T,
    pub warnings: Vec<String>,
}

struct DecompressLimit<R> {
    inner: R,
    limit: u64,
    read: u64,
}

impl<R: Read> DecompressLimit<R> {
    fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            limit,
            read: 0,
        }
    }
}

impl<R: Read> Read for DecompressLimit<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        if self.read > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "decompressed archive exceeds maximum size of {} bytes",
                    self.limit
                ),
            ));
        }
        Ok(n)
    }
}

fn notify_module_reload(kind: &str) {
    match control::notify_running(ReloadRequest::module(kind)) {
        Ok(NotifyOutcome::Delivered | NotifyOutcome::NotRunning) => {}
        Err(err) => {
            log::warn!("failed to notify running daemon about module `{kind}` reload: {err}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceKind {
    Local,
    Url,
}

#[derive(Debug)]
pub(crate) struct ResolvedSource {
    path: PathBuf,
    kind: SourceKind,
}

impl ResolvedSource {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn kind(&self) -> SourceKind {
        self.kind
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

pub(crate) fn decide_extension_install(
    installed: Option<&Metadata>,
    candidate: &Metadata,
    force: bool,
) -> ModuleInstallAction {
    match installed {
        None => ModuleInstallAction::Fresh,
        Some(existing) if existing.version == candidate.version => {
            if force {
                ModuleInstallAction::Replace
            } else {
                ModuleInstallAction::Skip
            }
        }
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

const EXTENSION_NATIVE_CODE_WARNING: &str =
    "extension binaries are native code and run with your full user privileges";

fn source_label_for_install(source: &str) -> PathBuf {
    if source.starts_with("http://") || source.starts_with("https://") {
        PathBuf::from(url_basename(source))
    } else {
        PathBuf::from(source)
    }
}

fn warn_extension_native_code() -> String {
    EXTENSION_NATIVE_CODE_WARNING.to_string()
}

pub(crate) fn is_remote_install_source(source: &str) -> bool {
    is_remote_url(source)
}

fn is_remote_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

fn reject_cleartext_download_urls(urls: &[String], allow_insecure_http: bool) -> Result<()> {
    if allow_insecure_http {
        return Ok(());
    }
    for url in urls {
        if url.starts_with("http://") {
            return Err(
                "cleartext HTTP downloads are disabled; pass --allow-insecure-http to override"
                    .into(),
            );
        }
    }
    Ok(())
}

fn collect_download_urls(source: &str, response: &impl ureq::ResponseExt) -> Vec<String> {
    let mut urls = vec![source.to_string()];
    if let Some(history) = response.get_redirect_history() {
        for uri in history {
            urls.push(uri.to_string());
        }
    }
    urls.push(response.get_uri().to_string());
    urls
}

fn url_basename(source: &str) -> &str {
    let without_query = source.split('?').next().unwrap_or(source);
    without_query.rsplit('/').next().unwrap_or(without_query)
}

pub(crate) fn resolve_source(source: &str, options: &InstallOptions) -> Result<ResolvedSource> {
    if source.starts_with("http://") && !options.allow_insecure_http {
        return Err(
            "cleartext HTTP downloads are disabled; pass --allow-insecure-http to override".into(),
        );
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        let basename = url_basename(source);
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(DOWNLOAD_TIMEOUT))
                .save_redirect_history(true)
                .build(),
        );
        let mut response = agent
            .get(source)
            .call()
            .map_err(|e| format!("failed to download `{source}`: {e}"))?;
        reject_cleartext_download_urls(
            &collect_download_urls(source, &response),
            options.allow_insecure_http,
        )?;
        let file_name = if basename.is_empty() {
            format!("smstatus-download-{}.tar.gz", std::process::id())
        } else {
            format!("smstatus-download-{}-{basename}", std::process::id())
        };
        let temp_path = std::env::temp_dir().join(file_name);
        let reader = response.body_mut().with_config().reader();
        write_limited_response_body(reader, &temp_path, MAX_DOWNLOAD_BYTES)
            .map_err(|e| format!("failed to read download body for `{source}`: {e}"))?;
        Ok(ResolvedSource {
            path: temp_path,
            kind: SourceKind::Url,
        })
    } else {
        let path = PathBuf::from(source);
        if !path.exists() {
            return Err(format!("source path `{}` does not exist", path.display()).into());
        }
        Ok(ResolvedSource {
            path,
            kind: SourceKind::Local,
        })
    }
}

fn write_limited_response_body<R: Read>(mut reader: R, dest: &Path, max_bytes: u64) -> Result<()> {
    let mut file = fs::File::create(dest)
        .map_err(|e| format!("failed to create temp download `{}`: {e}", dest.display()))?;
    let mut buffer = [0u8; 8192];
    let mut total: u64 = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("failed to read download body: {e}"))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > max_bytes {
            let _ = fs::remove_file(dest);
            return Err(format!("download exceeds maximum size of {max_bytes} bytes").into());
        }
        file.write_all(&buffer[..read])
            .map_err(|e| format!("failed to write temp download `{}`: {e}", dest.display()))?;
    }
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

fn normalize_sha256(hash: &str) -> Result<String> {
    let normalized = hash.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("SHA-256 hash must be 64 hexadecimal characters".into());
    }
    Ok(normalized)
}

fn resolve_expected_sha256(source: &str, options: &InstallOptions) -> Result<Option<String>> {
    if let Some(hash) = options.expected_sha256.as_ref() {
        let normalized = normalize_sha256(hash)?;
        if !normalized.is_empty() {
            return Ok(Some(normalized));
        }
    }
    let path = Path::new(source);
    if !path.exists() {
        return Ok(None);
    }
    let sidecar = PathBuf::from(format!("{source}.sha256"));
    if !sidecar.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&sidecar).map_err(|e| {
        format!(
            "failed to read SHA-256 sidecar `{}`: {e}",
            sidecar.display()
        )
    })?;
    let hash = contents
        .lines()
        .next()
        .ok_or_else(|| format!("SHA-256 sidecar `{}` is empty", sidecar.display()))?
        .trim();
    Ok(Some(normalize_sha256(hash)?))
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file = fs::File::open(path).map_err(|e| {
        format!(
            "failed to open `{}` for SHA-256 verification: {e}",
            path.display()
        )
    })?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| {
        format!(
            "failed to read `{}` for SHA-256 verification: {e}",
            path.display()
        )
    })?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(format!(
            "SHA-256 mismatch for `{}`: expected {expected}, got {actual}",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_resolved_source(
    resolved: &ResolvedSource,
    source: &str,
    options: &InstallOptions,
    warnings: &mut Vec<String>,
) -> Result<()> {
    match resolve_expected_sha256(source, options)? {
        Some(expected) => verify_sha256(resolved.path(), &expected),
        None if resolved.kind() == SourceKind::Url => {
            Err("remote install requires --sha256 or a catalog-supplied hash".into())
        }
        None => {
            warnings.push("installing local archive without SHA-256 verification".to_string());
            Ok(())
        }
    }
}

fn validate_tar_entry_path(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(format!("archive entry path `{}` is absolute", path.display()).into());
            }
            std::path::Component::ParentDir => {
                return Err(format!(
                    "archive entry path `{}` contains parent directory component",
                    path.display()
                )
                .into());
            }
            _ => {}
        }
    }
    Ok(())
}

fn unpack_archive(archive: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)
        .map_err(|e| format!("failed to create unpack dir `{}`: {e}", dest.display()))?;
    let file = fs::File::open(archive)
        .map_err(|e| format!("failed to open archive `{}`: {e}", archive.display()))?;
    let decoder = DecompressLimit::new(GzDecoder::new(file), MAX_UNPACKED_BYTES);
    let mut tar = tar::Archive::new(decoder);
    let mut unpacked_bytes: u64 = 0;
    for entry in tar.entries().map_err(|e| {
        format!(
            "failed to read archive entries from `{}`: {e}",
            archive.display()
        )
    })? {
        let mut entry = entry.map_err(|e| {
            format!(
                "failed to read archive entry from `{}`: {e}",
                archive.display()
            )
        })?;
        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::GNUSparse => {}
            tar::EntryType::Directory => {}
            other => {
                return Err(format!(
                    "unsupported archive entry type `{other:?}` in `{}`",
                    archive.display()
                )
                .into());
            }
        }
        let path = entry
            .path()
            .map_err(|e| format!("invalid archive entry path in `{}`: {e}", archive.display()))?;
        validate_tar_entry_path(&path)?;
        unpacked_bytes = unpacked_bytes.saturating_add(entry.header().size().unwrap_or(0));
        if unpacked_bytes > MAX_UNPACKED_BYTES {
            return Err(format!(
                "archive `{}` exceeds maximum unpacked size of {MAX_UNPACKED_BYTES} bytes",
                archive.display()
            )
            .into());
        }
        entry.unpack_in(dest).map_err(|e| {
            format!(
                "failed to unpack archive entry in `{}`: {e}",
                archive.display()
            )
        })?;
    }
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
    options: &InstallOptions,
) -> Result<InstallOutput<ModuleInstallOutcome>> {
    let check_path = source_label_for_install(source);
    if !has_tar_gz_extension(&check_path) {
        return Err(format!(
            "module source must be a `.tar.gz` archive, got `{}`",
            check_path.display()
        )
        .into());
    }

    let mut warnings = Vec::new();
    let resolved = resolve_source(source, options)?;
    verify_resolved_source(&resolved, source, options, &mut warnings)?;

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
                        return Ok(InstallOutput {
                            value: ModuleInstallOutcome::Skip {
                                kind,
                                metadata: installed,
                            },
                            warnings,
                        });
                    }
                    ModuleInstallAction::Replace => {
                        let (_scratch, staged) = stage_module_package(&root)?;
                        place_package_dir(&staged, &dest)?;
                        notify_module_reload(&kind);
                        return Ok(InstallOutput {
                            value: ModuleInstallOutcome::Replace {
                                kind,
                                metadata: candidate,
                            },
                            warnings,
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
                notify_module_reload(&kind);
                return Ok(InstallOutput {
                    value: ModuleInstallOutcome::Replace {
                        kind,
                        metadata: candidate,
                    },
                    warnings,
                });
            }
        }
    }

    let (_scratch, staged) = stage_module_package(&root)?;
    place_package_dir(&staged, &dest)?;
    notify_module_reload(&kind);
    Ok(InstallOutput {
        value: ModuleInstallOutcome::Fresh {
            kind,
            metadata: candidate,
        },
        warnings,
    })
}

pub(crate) fn install_module(
    source: &str,
    options: &InstallOptions,
) -> Result<InstallOutput<ModuleInstallOutcome>> {
    let modules_dir = crate::config::default_config_dir()?.join("modules");
    install_module_into(&modules_dir, source, options)
}

#[derive(Debug)]
pub(crate) enum ExtensionInstallOutcome {
    Fresh { name: String },
    Skip { name: String, metadata: Metadata },
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
        ExtensionInstallOutcome::Skip { name, metadata } => {
            format!(
                "extension `{name}` already installed ({})",
                format_meta(metadata)
            )
        }
        ExtensionInstallOutcome::Replace { name } => {
            format!(
                "replaced extension `{name}`; run `smstatus reload --extension {name}` to use the new binary"
            )
        }
    }
}

pub(crate) fn install_extension_into(
    extensions_dir: &Path,
    source: &str,
    options: &InstallOptions,
) -> Result<InstallOutput<ExtensionInstallOutcome>> {
    let check_path = source_label_for_install(source);
    if !has_tar_gz_extension(&check_path) {
        return Err(format!(
            "extension source must be a `.tar.gz` archive, got `{}`",
            check_path.display()
        )
        .into());
    }

    let mut warnings = vec![warn_extension_native_code()];
    let resolved = resolve_source(source, options)?;
    verify_resolved_source(&resolved, source, options, &mut warnings)?;

    let unpack = ScratchDir(unique_temp_dir("ext-unpack")?);
    unpack_archive(resolved.path(), &unpack.0)?;
    let root = package_root(&unpack.0)?;
    require_extension_files(&root)?;
    let manifest = manifest::read_extension_manifest_from(&root.join("manifest.toml"))?;
    let name = manifest.name.clone();
    let candidate = manifest.to_metadata();
    let dest = manifest::extension_dir(extensions_dir, &name);

    if dest.exists() {
        match manifest::read_extension_manifest(extensions_dir, &name) {
            Ok(installed_manifest) => {
                let installed = installed_manifest.to_metadata();
                match decide_extension_install(Some(&installed), &candidate, options.force) {
                    ModuleInstallAction::Skip => {
                        return Ok(InstallOutput {
                            value: ExtensionInstallOutcome::Skip {
                                name,
                                metadata: installed,
                            },
                            warnings,
                        });
                    }
                    ModuleInstallAction::Replace => {
                        let (_scratch, staged) = stage_extension_package(&root)?;
                        place_package_dir(&staged, &dest)?;
                        set_executable(&manifest::extension_binary_path(extensions_dir, &name))?;
                        let _ = manifest::read_extension_manifest(extensions_dir, &name)?;
                        return Ok(InstallOutput {
                            value: ExtensionInstallOutcome::Replace { name },
                            warnings,
                        });
                    }
                    ModuleInstallAction::Fresh => {
                        unreachable!("dest exists implies installed is Some")
                    }
                }
            }
            Err(_) => {
                let (_scratch, staged) = stage_extension_package(&root)?;
                place_package_dir(&staged, &dest)?;
                set_executable(&manifest::extension_binary_path(extensions_dir, &name))?;
                let _ = manifest::read_extension_manifest(extensions_dir, &name)?;
                return Ok(InstallOutput {
                    value: ExtensionInstallOutcome::Replace { name },
                    warnings,
                });
            }
        }
    }

    let (_scratch, staged) = stage_extension_package(&root)?;
    place_package_dir(&staged, &dest)?;
    set_executable(&manifest::extension_binary_path(extensions_dir, &name))?;
    let _ = manifest::read_extension_manifest(extensions_dir, &name)?;

    Ok(InstallOutput {
        value: ExtensionInstallOutcome::Fresh { name },
        warnings,
    })
}

pub(crate) fn install_extension(
    source: &str,
    options: &InstallOptions,
) -> Result<InstallOutput<ExtensionInstallOutcome>> {
    let extensions_dir = crate::config::default_config_dir()?.join("extensions");
    install_extension_into(&extensions_dir, source, options)
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
        pack_echo_archive_version("0.1.0")
    }

    fn pack_echo_archive_version(version: &str) -> PathBuf {
        let echo = find_or_build_echo();
        let staging = temp_modules_dir(&format!("pack-echo-{version}"));
        fs::write(
            staging.join("manifest.toml"),
            extension_manifest_toml("echo", version),
        )
        .unwrap();
        fs::copy(&echo, staging.join("extension")).unwrap();
        let archive = PathBuf::from(format!("{}.tar.gz", staging.display()));
        write_tar_gz(&staging, &archive).unwrap();
        archive
    }

    fn set_raw_tar_path(header: &mut tar::Header, path: &str) {
        let gnu = header.as_gnu_mut().expect("gnu header");
        gnu.name.fill(0);
        let bytes = path.as_bytes();
        let len = bytes.len().min(gnu.name.len().saturating_sub(1));
        gnu.name[..len].copy_from_slice(&bytes[..len]);
        header.set_cksum();
    }

    fn write_raw_tar_gz(archive: &Path, tar_bytes: &[u8]) -> Result<()> {
        let file = fs::File::create(archive)
            .map_err(|e| format!("failed to create archive `{}`: {e}", archive.display()))?;
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder
            .write_all(tar_bytes)
            .map_err(|e| format!("failed to write archive `{}`: {e}", archive.display()))?;
        encoder
            .finish()
            .map_err(|e| format!("failed to finish archive: {e}"))?;
        Ok(())
    }

    fn write_tar_gz_with<F>(archive: &Path, add_entries: F) -> Result<()>
    where
        F: FnOnce(&mut tar::Builder<GzEncoder<std::fs::File>>) -> Result<()>,
    {
        let file = fs::File::create(archive)
            .map_err(|e| format!("failed to create archive `{}`: {e}", archive.display()))?;
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        add_entries(&mut builder)?;
        builder
            .finish()
            .map_err(|e| format!("failed to finish archive: {e}"))?;
        Ok(())
    }

    fn append_symlink(
        builder: &mut tar::Builder<GzEncoder<std::fs::File>>,
        name: &str,
        target: &str,
    ) -> Result<()> {
        let mut header = tar::Header::new_gnu();
        header
            .set_path(name)
            .map_err(|e| format!("failed to set symlink path: {e}"))?;
        header
            .set_link_name(target)
            .map_err(|e| format!("failed to set symlink target: {e}"))?;
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        builder
            .append(&header, &[] as &[u8])
            .map_err(|e| format!("failed to append symlink: {e}"))?;
        Ok(())
    }

    fn unpack_to_temp(archive: &Path) -> PathBuf {
        let dest = unique_temp_dir("unpack-test").unwrap();
        unpack_archive(archive, &dest).unwrap();
        dest
    }

    #[test]
    fn unpack_archive_rejects_absolute_path_member() {
        let archive = temp_modules_dir("tar-abs").join("evil.tar.gz");
        write_tar_gz_with(&archive, |builder| {
            let mut header = tar::Header::new_gnu();
            set_raw_tar_path(&mut header, "/etc/passwd");
            header.set_size(4);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, &b"evil"[..])?;
            Ok(())
        })
        .unwrap();
        let dest = unique_temp_dir("tar-abs-unpack").unwrap();
        let err = unpack_archive(&archive, &dest).unwrap_err();
        assert!(err.to_string().contains("absolute"));
    }

    #[test]
    fn unpack_archive_rejects_parent_dir_member() {
        let archive = temp_modules_dir("tar-parent").join("evil.tar.gz");
        write_tar_gz_with(&archive, |builder| {
            let mut header = tar::Header::new_gnu();
            set_raw_tar_path(&mut header, "../outside.txt");
            header.set_size(4);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, &b"evil"[..])?;
            Ok(())
        })
        .unwrap();
        let dest = unique_temp_dir("tar-parent-unpack").unwrap();
        let err = unpack_archive(&archive, &dest).unwrap_err();
        assert!(err.to_string().contains("parent directory"));
    }

    #[test]
    fn unpack_archive_rejects_symlink_member() {
        let archive = temp_modules_dir("tar-symlink").join("evil.tar.gz");
        write_tar_gz_with(&archive, |builder| {
            append_symlink(builder, "link", "/etc/passwd")
        })
        .unwrap();
        let dest = unique_temp_dir("tar-symlink-unpack").unwrap();
        let err = unpack_archive(&archive, &dest).unwrap_err();
        assert!(err.to_string().contains("Symlink"));
    }

    #[test]
    fn unpack_archive_rejects_oversized_aggregate() {
        let archive = temp_modules_dir("tar-oversized").join("evil.tar.gz");
        let mut header = tar::Header::new_gnu();
        set_raw_tar_path(&mut header, "large.bin");
        header.set_size(65 * 1024 * 1024);
        header.set_mode(0o644);
        header.set_cksum();
        let mut tar_bytes = header.as_bytes().to_vec();
        tar_bytes.extend_from_slice(&[0u8; 512]);
        tar_bytes.extend_from_slice(&[0u8; 512]);
        write_raw_tar_gz(&archive, &tar_bytes).unwrap();
        let dest = unique_temp_dir("tar-oversized-unpack").unwrap();
        let err = unpack_archive(&archive, &dest).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum unpacked size"));
    }

    #[test]
    fn unpack_archive_accepts_legitimate_module_and_extension_archives() {
        let module = pack_module_archive("battery", "0.1.0");
        let module_dest = unpack_to_temp(&module);
        assert!(package_root(&module_dest).is_ok());

        let extension = pack_echo_archive();
        let extension_dest = unpack_to_temp(&extension);
        assert!(package_root(&extension_dest).is_ok());
    }

    fn install_options() -> InstallOptions {
        InstallOptions::default()
    }

    fn module_value(result: Result<InstallOutput<ModuleInstallOutcome>>) -> ModuleInstallOutcome {
        result.unwrap().value
    }

    fn extension_value(
        result: Result<InstallOutput<ExtensionInstallOutcome>>,
    ) -> ExtensionInstallOutcome {
        result.unwrap().value
    }

    fn sha256_hex(path: &Path) -> String {
        let mut file = fs::File::open(path).unwrap();
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher).unwrap();
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn verify_resolved_source_requires_hash_for_remote() {
        let resolved = ResolvedSource {
            path: PathBuf::from("/tmp/smstatus-remote-test.tar.gz"),
            kind: SourceKind::Url,
        };
        let mut warnings = Vec::new();
        let err = verify_resolved_source(
            &resolved,
            "https://example.com/remote.tar.gz",
            &install_options(),
            &mut warnings,
        )
        .unwrap_err();
        assert!(err.to_string().contains("requires --sha256"));
    }

    #[test]
    fn install_module_into_accepts_matching_local_sidecar() {
        let modules_dir = temp_modules_dir("sidecar-match");
        let archive = pack_module_archive("battery", "0.1.0");
        fs::write(
            format!("{}.sha256", archive.display()),
            format!("{}\n", sha256_hex(&archive)),
        )
        .unwrap();
        let outcome = module_value(install_module_into(
            &modules_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match outcome {
            ModuleInstallOutcome::Fresh { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[test]
    fn install_module_into_rejects_wrong_sha256() {
        let modules_dir = temp_modules_dir("sidecar-mismatch");
        let archive = pack_module_archive("battery", "0.1.0");
        let options = InstallOptions {
            expected_sha256: Some("0".repeat(64)),
            ..Default::default()
        };
        let err =
            install_module_into(&modules_dir, archive.to_str().unwrap(), &options).unwrap_err();
        assert!(err.to_string().contains("SHA-256 mismatch"));
    }

    #[test]
    fn install_module_into_local_without_sidecar_succeeds() {
        let modules_dir = temp_modules_dir("no-sidecar");
        let archive = pack_module_archive("cpu", "0.1.0");
        let outcome = module_value(install_module_into(
            &modules_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match outcome {
            ModuleInstallOutcome::Fresh { kind, .. } => assert_eq!(kind, "cpu"),
            other => panic!("expected Fresh, got {other:?}"),
        }
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
    fn decide_extension_install_skip_same_version_without_force() {
        let installed = meta("1.0.0");
        let candidate = meta("1.0.0");
        assert_eq!(
            decide_extension_install(Some(&installed), &candidate, false),
            ModuleInstallAction::Skip
        );
    }

    #[test]
    fn decide_extension_install_replace_different_version() {
        let installed = meta("1.0.0");
        let candidate = meta("2.0.0");
        assert_eq!(
            decide_extension_install(Some(&installed), &candidate, false),
            ModuleInstallAction::Replace
        );
    }

    #[test]
    fn decide_extension_install_force_replaces_same_version() {
        let installed = meta("1.0.0");
        let candidate = meta("1.0.0");
        assert_eq!(
            decide_extension_install(Some(&installed), &candidate, true),
            ModuleInstallAction::Replace
        );
    }

    #[test]
    fn install_module_into_fresh_and_skip_same_version() {
        let modules_dir = temp_modules_dir("fresh-skip");
        let archive = pack_module_archive("battery", "0.1.0");

        let first = module_value(install_module_into(
            &modules_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match &first {
            ModuleInstallOutcome::Fresh { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Fresh, got {other:?}"),
        }
        assert!(manifest::module_manifest_path(&modules_dir, "battery").exists());
        assert!(manifest::module_wasm_path(&modules_dir, "battery").exists());

        let second = module_value(install_module_into(
            &modules_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match second {
            ModuleInstallOutcome::Skip { kind, .. } => assert_eq!(kind, "battery"),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn install_module_into_replaces_different_version() {
        let modules_dir = temp_modules_dir("replace-version");
        let battery = pack_module_archive("battery", "0.1.0");
        let _ = module_value(install_module_into(
            &modules_dir,
            battery.to_str().unwrap(),
            &install_options(),
        ));

        let newer = pack_module_archive("battery", "9.9.9");
        let outcome = module_value(install_module_into(
            &modules_dir,
            newer.to_str().unwrap(),
            &install_options(),
        ));
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
        let _ = module_value(install_module_into(
            &modules_dir,
            battery.to_str().unwrap(),
            &install_options(),
        ));

        fs::write(
            manifest::module_manifest_path(&modules_dir, "battery"),
            b"not a toml manifest",
        )
        .unwrap();

        let outcome = module_value(install_module_into(
            &modules_dir,
            battery.to_str().unwrap(),
            &install_options(),
        ));
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
        let err = install_module_into(&modules_dir, wasm.to_str().unwrap(), &install_options())
            .unwrap_err();
        assert!(err.to_string().contains(".tar.gz"));
    }

    #[test]
    fn resolve_source_rejects_cleartext_http_by_default() {
        let err = resolve_source("http://example.com/x.tar.gz", &install_options()).unwrap_err();
        assert!(err.to_string().contains("--allow-insecure-http"));
    }

    #[test]
    fn resolve_source_allows_cleartext_http_when_enabled() {
        let options = InstallOptions {
            allow_insecure_http: true,
            ..Default::default()
        };
        let err = resolve_source("http://example.com/x.tar.gz", &options).unwrap_err();
        assert!(!err.to_string().contains("--allow-insecure-http"));
    }

    #[test]
    fn write_limited_response_body_rejects_oversized_reader() {
        struct OversizedReader {
            remaining: usize,
        }

        impl Read for OversizedReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.remaining == 0 {
                    return Ok(0);
                }
                let n = buf.len().min(self.remaining);
                buf[..n].fill(0);
                self.remaining -= n;
                Ok(n)
            }
        }

        let dest = unique_temp_dir("download-cap")
            .unwrap()
            .join("download.tar.gz");
        let max_bytes = 1024;
        let err = write_limited_response_body(
            OversizedReader {
                remaining: max_bytes as usize + 1,
            },
            &dest,
            max_bytes,
        )
        .unwrap_err();
        assert!(err.to_string().contains("exceeds maximum size"));
        assert!(!dest.exists());
    }

    #[test]
    fn resolve_source_rejects_missing_local_path() {
        let err =
            resolve_source("/no/such/smstatus-module.tar.gz", &install_options()).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn install_extension_into_places_binary_where_is_installed_sees_it() {
        use crate::extension::ExtensionRegistry;

        let base = temp_modules_dir("ext-install");
        let extensions_dir = base.join("extensions");
        let archive = pack_echo_archive();

        let outcome = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match outcome {
            ExtensionInstallOutcome::Fresh { name } => assert_eq!(name, "echo"),
            other => panic!("expected Fresh, got {other:?}"),
        }
        assert!(manifest::extension_manifest_path(&extensions_dir, "echo").exists());
        assert!(manifest::extension_binary_path(&extensions_dir, "echo").exists());

        let registry = ExtensionRegistry::new(extensions_dir.clone(), base.join("sockets"));
        assert!(registry.is_installed("echo"));

        let again = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match again {
            ExtensionInstallOutcome::Skip { name, .. } => assert_eq!(name, "echo"),
            other => panic!("expected Skip, got {other:?}"),
        }
        assert!(registry.is_installed("echo"));
    }

    #[test]
    fn install_extension_into_fresh_and_skip_same_version() {
        let base = temp_modules_dir("ext-fresh-skip");
        let extensions_dir = base.join("extensions");
        let archive = pack_echo_archive();

        let first = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match &first {
            ExtensionInstallOutcome::Fresh { name } => assert_eq!(name, "echo"),
            other => panic!("expected Fresh, got {other:?}"),
        }
        assert!(manifest::extension_manifest_path(&extensions_dir, "echo").exists());
        assert!(manifest::extension_binary_path(&extensions_dir, "echo").exists());

        let second = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match second {
            ExtensionInstallOutcome::Skip { name, .. } => assert_eq!(name, "echo"),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn install_extension_into_replaces_different_version_without_force() {
        let base = temp_modules_dir("ext-replace-version");
        let extensions_dir = base.join("extensions");
        let archive = pack_echo_archive();

        let _ = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));

        let newer = pack_echo_archive_version("9.9.9");
        let outcome = extension_value(install_extension_into(
            &extensions_dir,
            newer.to_str().unwrap(),
            &install_options(),
        ));
        match outcome {
            ExtensionInstallOutcome::Replace { name } => assert_eq!(name, "echo"),
            other => panic!("expected Replace, got {other:?}"),
        }
        let installed = manifest::read_extension_manifest(&extensions_dir, "echo").unwrap();
        assert_eq!(installed.version, "9.9.9");
    }

    #[test]
    fn install_extension_into_same_version_skips_without_force_and_replaces_with_force() {
        let base = temp_modules_dir("ext-force");
        let extensions_dir = base.join("extensions");
        let archive = pack_echo_archive();

        let _ = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));

        let outcome = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));
        match outcome {
            ExtensionInstallOutcome::Skip { name, .. } => assert_eq!(name, "echo"),
            other => panic!("expected Skip, got {other:?}"),
        }

        let outcome = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &InstallOptions {
                force: true,
                ..Default::default()
            },
        ));
        match outcome {
            ExtensionInstallOutcome::Replace { name } => assert_eq!(name, "echo"),
            other => panic!("expected Replace, got {other:?}"),
        }
    }

    #[test]
    fn list_and_remove_module_round_trip() {
        let modules_dir = temp_modules_dir("mod-list-rm");
        let battery = pack_module_archive("battery", "0.1.0");
        let _ = module_value(install_module_into(
            &modules_dir,
            battery.to_str().unwrap(),
            &install_options(),
        ));

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
        let _ = extension_value(install_extension_into(
            &extensions_dir,
            archive.to_str().unwrap(),
            &install_options(),
        ));

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

    #[test]
    fn reject_cleartext_redirect_urls_blocks_http_target() {
        let urls = vec![
            "https://example.com/pkg.tar.gz".to_string(),
            "http://example.com/pkg.tar.gz".to_string(),
        ];
        let err = reject_cleartext_download_urls(&urls, false).unwrap_err();
        assert!(err.to_string().contains("--allow-insecure-http"));
    }

    #[test]
    fn normalize_sha256_rejects_invalid_format() {
        let err = normalize_sha256("not-a-hash").unwrap_err();
        assert!(
            err.to_string()
                .contains("SHA-256 hash must be 64 hexadecimal characters")
        );
    }

    #[test]
    fn decompress_limit_rejects_oversized_stream() {
        struct EndlessReader;

        impl Read for EndlessReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                buf.fill(0);
                Ok(buf.len())
            }
        }

        let mut limited = DecompressLimit::new(EndlessReader, 1024);
        let mut sink = [0u8; 2048];
        let err = limited.read(&mut sink).unwrap_err();
        assert!(
            err.to_string()
                .contains("decompressed archive exceeds maximum size")
        );
    }

    #[test]
    fn install_module_into_rejects_non_tar_gz_before_download() {
        let modules_dir = temp_modules_dir("reject-extension");
        let err = install_module_into(
            &modules_dir,
            "https://example.com/module.zip",
            &install_options(),
        )
        .unwrap_err();
        assert!(err.to_string().contains(".tar.gz"));
    }

    #[test]
    fn install_module_into_local_without_sidecar_emits_warning() {
        let modules_dir = temp_modules_dir("warn-no-sidecar");
        let archive = pack_module_archive("cpu", "0.1.0");
        let output =
            install_module_into(&modules_dir, archive.to_str().unwrap(), &install_options())
                .unwrap();
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.contains("without SHA-256 verification"))
        );
    }
}
