use std::path::Path;

use crate::config::BarConfig;
use crate::extension::ExtensionRegistry;
use crate::manifest::RequiredExtension;
use crate::version;

use super::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum RequirementStatus {
    Ok,
    Missing,
    Incompatible,
}

pub(in crate::tui) fn requirement_status(
    req: &RequiredExtension,
    registry: &ExtensionRegistry,
) -> RequirementStatus {
    if !registry.is_installed(&req.name) {
        return RequirementStatus::Missing;
    }
    let Ok(installed) = registry.installed_package_version(&req.name) else {
        return RequirementStatus::Incompatible;
    };
    if version::package_version_meets_floor(installed, req.version.major, req.version.minor) {
        RequirementStatus::Ok
    } else {
        RequirementStatus::Incompatible
    }
}

pub(in crate::tui) fn format_requirement_line(
    req: &RequiredExtension,
    status: RequirementStatus,
) -> String {
    match status {
        RequirementStatus::Ok => format!("{} ok", req.name),
        RequirementStatus::Missing => format!("{} missing", req.name),
        RequirementStatus::Incompatible => format!(
            "{} incompatible (>={}.{})",
            req.name, req.version.major, req.version.minor
        ),
    }
}

fn extension_registry_for_status(extensions_dir: &Path) -> ExtensionRegistry {
    let socket_dir = crate::lock::lock_dir()
        .map(|d| d.join("extensions"))
        .unwrap_or_else(|_| extensions_dir.join("sockets"));
    ExtensionRegistry::new(extensions_dir.to_path_buf(), socket_dir)
}

fn extension_overlay_label(extensions_dir: &Path, name: &str) -> String {
    match crate::manifest::read_extension_manifest(extensions_dir, name) {
        Ok(manifest) => format!("{name} {}", manifest.version),
        Err(_) => name.to_string(),
    }
}

impl App {
    pub(super) fn refresh_extension_display_cache(&mut self) {
        self.extension_overlay_labels = match self.extensions_dir.as_ref() {
            Some(extensions_dir) => self
                .installed_extensions
                .iter()
                .map(|name| extension_overlay_label(extensions_dir, name))
                .collect(),
            None => self.installed_extensions.clone(),
        };
        let registry = self
            .extensions_dir
            .as_ref()
            .map(|dir| extension_registry_for_status(dir.as_path()));
        self.requirement_lines_by_kind = self
            .required_extensions_by_kind
            .iter()
            .map(|(kind, reqs)| {
                let lines = if reqs.is_empty() {
                    vec!["(no required extensions)".to_string()]
                } else {
                    reqs.iter()
                        .map(|req| {
                            let status = match &registry {
                                Some(registry) => requirement_status(req, registry),
                                None => RequirementStatus::Missing,
                            };
                            format_requirement_line(req, status)
                        })
                        .collect()
                };
                (kind.clone(), lines)
            })
            .collect();
    }

    pub(super) fn requirement_header_line_count(&self) -> usize {
        let Some(kind) = self.selected_module_kind() else {
            return 0;
        };
        self.requirement_lines_by_kind.get(kind).map_or(0, Vec::len)
    }

    fn selected_module_kind(&self) -> Option<&str> {
        let idx = self.selected_index?;
        let entry = self.modules.as_ref()?.get(idx)?;
        Some(BarConfig::split_module_entry(entry).0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ApiVersionReq;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn temp_dir() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "smstatus-req-status-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn install_extension(extensions_dir: &Path, name: &str, version: &str) {
        let pkg = extensions_dir.join(name);
        std::fs::create_dir_all(&pkg).unwrap();
        let path = pkg.join("extension");
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            pkg.join("manifest.toml"),
            format!(
                "name = \"{name}\"\nversion = \"{version}\"\nauthor = \"test\"\nextensions-api = {{ major = 0, minor = 1 }}\n"
            ),
        )
        .unwrap();
    }

    fn req(name: &str, major: u32, minor: u32) -> RequiredExtension {
        RequiredExtension {
            name: name.to_string(),
            version: ApiVersionReq { major, minor },
        }
    }

    #[test]
    fn requirement_status_present_when_installed_and_meets_floor() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        install_extension(&extensions_dir, "echo", "1.2.0");
        let registry = ExtensionRegistry::new(extensions_dir, base.join("sockets"));
        assert_eq!(
            requirement_status(&req("echo", 1, 2), &registry),
            RequirementStatus::Ok
        );
    }

    #[test]
    fn requirement_status_missing_when_not_installed() {
        let base = temp_dir();
        let registry = ExtensionRegistry::new(base.join("extensions"), base.join("sockets"));
        assert_eq!(
            requirement_status(&req("echo", 1, 0), &registry),
            RequirementStatus::Missing
        );
    }

    #[test]
    fn requirement_status_incompatible_when_version_below_floor() {
        let base = temp_dir();
        let extensions_dir = base.join("extensions");
        install_extension(&extensions_dir, "echo", "1.0.0");
        let registry = ExtensionRegistry::new(extensions_dir, base.join("sockets"));
        assert_eq!(
            requirement_status(&req("echo", 1, 2), &registry),
            RequirementStatus::Incompatible
        );
    }
}
