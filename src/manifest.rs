use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(crate) struct ModulePermission {
    pub extension: String,
    pub method: String,
    #[serde(flatten)]
    pub constraints: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(crate) struct ModuleManifest {
    pub name: String,
    pub version: String,
    pub author: String,
    pub display_name: String,
    #[serde(rename = "modules-api")]
    pub modules_api: ApiVersionReq,
    #[serde(default)]
    pub required_extensions: Vec<RequiredExtension>,
    #[serde(default)]
    pub permissions: Vec<ModulePermission>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ExtensionManifest {
    pub name: String,
    pub version: String,
    pub author: String,
    #[serde(rename = "extensions-api")]
    pub extensions_api: ApiVersionReq,
}

impl ModulePermission {
    pub(crate) fn to_protocol_entry(&self) -> Result<extension_protocol::PermissionEntry> {
        let mut constraints = BTreeMap::new();
        for (key, value) in &self.constraints {
            constraints.insert(key.clone(), toml_value_to_json(value)?);
        }
        Ok(extension_protocol::PermissionEntry {
            extension: self.extension.clone(),
            method: self.method.clone(),
            constraints,
        })
    }
}

impl ModuleManifest {
    pub(crate) fn to_metadata(&self) -> Metadata {
        Metadata {
            display_name: self.display_name.clone(),
            version: self.version.clone(),
            author: self.author.clone(),
        }
    }

    pub(crate) fn frozen_protocol_permissions(
        &self,
    ) -> Result<Arc<[extension_protocol::PermissionEntry]>> {
        let mut entries = Vec::with_capacity(self.permissions.len());
        for perm in &self.permissions {
            entries.push(perm.to_protocol_entry()?);
        }
        Ok(Arc::from(entries))
    }
}

fn toml_value_to_json(value: &toml::Value) -> Result<serde_json::Value> {
    match value {
        toml::Value::String(s) => Ok(serde_json::Value::String(s.clone())),
        toml::Value::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
        toml::Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        toml::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(toml_value_to_json(item)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        other => Err(format!("unsupported TOML constraint type `{}`", other.type_str()).into()),
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
    for perm in &manifest.permissions {
        if !is_safe_extension_name(&perm.extension) {
            return Err(format!("invalid permission extension name `{}`", perm.extension).into());
        }
        if perm.method.is_empty() {
            return Err("permission method must be non-empty".into());
        }
        if perm.method == extension_protocol::CHECK_METHOD {
            return Err(format!(
                "permission method `{}` is reserved",
                extension_protocol::CHECK_METHOD
            )
            .into());
        }
        if !manifest
            .required_extensions
            .iter()
            .any(|ext| ext.name == perm.extension)
        {
            return Err(format!(
                "permission references extension `{}` which is not in required_extensions",
                perm.extension
            )
            .into());
        }
        perm.to_protocol_entry()?;
    }
    Ok(manifest)
}

pub(crate) fn parse_extension_manifest_str(text: &str) -> Result<ExtensionManifest> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_manifest(extra: &str) -> String {
        format!(
            r#"
name = "demo"
version = "1.0.0"
author = "test"
display_name = "Demo"
modules-api = {{ major = 0, minor = 1 }}

[[required_extensions]]
name = "fs"
version = {{ major = 0, minor = 1 }}

[[required_extensions]]
name = "http"
version = {{ major = 0, minor = 1 }}
{extra}
"#
        )
    }

    #[test]
    fn parses_permissions_with_flattened_prefix_constraints() {
        let manifest = parse_module_manifest_str(&base_manifest(
            r#"
[[permissions]]
extension = "fs"
method = "read"
path_prefixes = ["/proc/", "~/.claude/"]

[[permissions]]
extension = "http"
method = "get"
url_prefixes = ["https://api.anthropic.com/"]
"#,
        ))
        .unwrap();
        assert_eq!(manifest.permissions.len(), 2);
        assert_eq!(manifest.permissions[0].extension, "fs");
        assert_eq!(manifest.permissions[0].method, "read");
        let prefixes = manifest.permissions[0]
            .constraints
            .get("path_prefixes")
            .unwrap();
        assert!(matches!(prefixes, toml::Value::Array(_)));
        let entry = manifest.permissions[0].to_protocol_entry().unwrap();
        assert_eq!(
            entry.constraints.get("path_prefixes").unwrap(),
            &serde_json::json!(["/proc/", "~/.claude/"])
        );
        assert_eq!(manifest.frozen_protocol_permissions().unwrap().len(), 2);
    }

    #[test]
    fn permissions_default_to_empty() {
        let manifest = parse_module_manifest_str(&base_manifest("")).unwrap();
        assert!(manifest.permissions.is_empty());
    }

    #[test]
    fn rejects_reserved_check_method() {
        let err = parse_module_manifest_str(&base_manifest(
            r#"
[[permissions]]
extension = "fs"
method = "check"
"#,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("reserved"));
    }

    #[test]
    fn rejects_invalid_permission_extension_name() {
        let err = parse_module_manifest_str(&base_manifest(
            r#"
[[permissions]]
extension = "../evil"
method = "read"
"#,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("invalid permission extension"));
    }

    #[test]
    fn rejects_permission_for_undeclared_extension() {
        let err = parse_module_manifest_str(&base_manifest(
            r#"
[[permissions]]
extension = "echo"
method = "ping"
"#,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("not in required_extensions"));
    }

    #[test]
    fn rejects_malformed_constraint_types() {
        let err = parse_module_manifest_str(&base_manifest(
            r#"
[[permissions]]
extension = "fs"
method = "read"
path_prefixes = { nested = true }
"#,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("unsupported TOML constraint type"));
    }
}
