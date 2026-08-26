use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Result;
use crate::extension::is_safe_extension_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub display_name: String,
    pub version: String,
    pub author: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ApiVersionReq {
    pub major: u32,
    pub minor: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct RequiredExtension {
    pub name: String,
    pub version: ApiVersionReq,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ModuleManifest {
    pub name: String,
    pub version: String,
    pub author: String,
    pub display_name: String,
    #[serde(rename = "modules-api")]
    pub modules_api: ApiVersionReq,
    #[serde(default)]
    pub required_extensions: Vec<RequiredExtension>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ExtensionManifest {
    pub name: String,
    pub version: String,
    pub author: String,
    #[serde(rename = "extensions-api")]
    pub extensions_api: ApiVersionReq,
}

impl ModuleManifest {
    pub(crate) fn to_metadata(&self) -> Metadata {
        Metadata {
            display_name: self.display_name.clone(),
            version: self.version.clone(),
            author: self.author.clone(),
        }
    }
}

pub(crate) fn module_dir(modules_dir: &Path, name: &str) -> PathBuf {
    modules_dir.join(name)
}

pub(crate) fn module_wasm_path(modules_dir: &Path, name: &str) -> PathBuf {
    module_dir(modules_dir, name).join("module.wasm")
}

pub(crate) fn module_manifest_path(modules_dir: &Path, name: &str) -> PathBuf {
    module_dir(modules_dir, name).join("manifest.toml")
}

pub(crate) fn extension_dir(extensions_dir: &Path, name: &str) -> PathBuf {
    extensions_dir.join(name)
}

pub(crate) fn extension_binary_path(extensions_dir: &Path, name: &str) -> PathBuf {
    extension_dir(extensions_dir, name).join("extension")
}

pub(crate) fn extension_manifest_path(extensions_dir: &Path, name: &str) -> PathBuf {
    extension_dir(extensions_dir, name).join("manifest.toml")
}

fn parse_module_manifest_str(text: &str) -> Result<ModuleManifest> {
    let manifest: ModuleManifest =
        toml::from_str(text).map_err(|e| format!("failed to parse module manifest: {e}"))?;
    if !is_safe_extension_name(&manifest.name) {
        return Err(format!("invalid module name `{}`", manifest.name).into());
    }
    for ext in &manifest.required_extensions {
        if !is_safe_extension_name(&ext.name) {
            return Err(format!("invalid required extension name `{}`", ext.name).into());
        }
    }
    Ok(manifest)
}

fn parse_extension_manifest_str(text: &str) -> Result<ExtensionManifest> {
    let manifest: ExtensionManifest =
        toml::from_str(text).map_err(|e| format!("failed to parse extension manifest: {e}"))?;
    if !is_safe_extension_name(&manifest.name) {
        return Err(format!("invalid extension name `{}`", manifest.name).into());
    }
    Ok(manifest)
}

pub(crate) fn read_module_manifest_from(path: &Path) -> Result<ModuleManifest> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("failed to read module manifest `{}`: {e}", path.display()))?;
    parse_module_manifest_str(&text)
}

pub(crate) fn read_extension_manifest_from(path: &Path) -> Result<ExtensionManifest> {
    let text = fs::read_to_string(path).map_err(|e| {
        format!(
            "failed to read extension manifest `{}`: {e}",
            path.display()
        )
    })?;
    parse_extension_manifest_str(&text)
}

pub(crate) fn read_module_manifest(modules_dir: &Path, name: &str) -> Result<ModuleManifest> {
    if !is_safe_extension_name(name) {
        return Err(format!("invalid module name `{name}`").into());
    }
    read_module_manifest_from(&module_manifest_path(modules_dir, name))
}

pub(crate) fn read_extension_manifest(
    extensions_dir: &Path,
    name: &str,
) -> Result<ExtensionManifest> {
    if !is_safe_extension_name(name) {
        return Err(format!("invalid extension name `{name}`").into());
    }
    read_extension_manifest_from(&extension_manifest_path(extensions_dir, name))
}
