use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::error::Result;

use super::unique_temp_dir;

pub(crate) fn write_tar_gz(staging: &Path, archive: &Path) -> Result<()> {
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

pub(crate) fn pack_minimal_extension_archive(name: &str) -> Result<PathBuf> {
    let staging = unique_temp_dir(&format!("ext-pack-{name}"))?;
    fs::write(
        staging.join("manifest.toml"),
        format!(
            "name = \"{name}\"\nversion = \"0.1.0\"\nauthor = \"test\"\nextensions-api = {{ major = 0, minor = 1 }}\n"
        ),
    )
    .map_err(|e| format!("failed to write extension manifest: {e}"))?;
    let binary = staging.join("extension");
    fs::write(&binary, "#!/bin/sh\nexit 0\n")
        .map_err(|e| format!("failed to write extension binary stub: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod extension binary stub: {e}"))?;
    }
    let archive = PathBuf::from(format!("{}.tar.gz", staging.display()));
    write_tar_gz(&staging, &archive)?;
    Ok(archive)
}

pub(crate) fn pack_minimal_module_archive(name: &str) -> Result<PathBuf> {
    let staging = unique_temp_dir(&format!("mod-pack-{name}"))?;
    fs::write(
        staging.join("manifest.toml"),
        format!(
            "name = \"{name}\"\ndisplay_name = \"{name}\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = {{ major = 0, minor = 1 }}\n"
        ),
    )
    .map_err(|e| format!("failed to write module manifest: {e}"))?;
    fs::write(staging.join("module.wasm"), b"\0asm")
        .map_err(|e| format!("failed to write module wasm stub: {e}"))?;
    let archive = PathBuf::from(format!("{}.tar.gz", staging.display()));
    write_tar_gz(&staging, &archive)?;
    Ok(archive)
}

pub(crate) fn write_tar_gz_bytes(archive: &Path, tar_bytes: &[u8]) -> Result<()> {
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
