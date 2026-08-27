use std::path::Path;

use crate::config::{
    PROGRAM_CONFIG_FILE, active_config_path, discover_module_kinds, read_active_name,
};
use crate::error::Result;
use crate::extension::{ExtensionRegistry, is_safe_extension_name};
use crate::install::list_extensions_in;
use crate::manifest::{self, ModuleManifest, RequiredExtension};
use crate::module::{UnmetExtension, unmet_extensions};
use crate::schema_probe::SchemaProbe;
use crate::version::{self, HOST_MODULES_API};

pub(crate) enum CheckStatus {
    Ok,
    Fail,
}

pub(crate) struct Check {
    pub status: CheckStatus,
    pub label: String,
    pub detail: String,
    pub hint: Option<String>,
}

fn ok_check(label: impl Into<String>, detail: impl Into<String>) -> Check {
    Check {
        status: CheckStatus::Ok,
        label: label.into(),
        detail: detail.into(),
        hint: None,
    }
}

fn fail_check(
    label: impl Into<String>,
    detail: impl Into<String>,
    hint: Option<impl Into<String>>,
) -> Check {
    Check {
        status: CheckStatus::Fail,
        label: label.into(),
        detail: detail.into(),
        hint: hint.map(Into::into),
    }
}

pub(crate) fn format_check(check: &Check) -> String {
    match check.status {
        CheckStatus::Ok => format!("ok {}: {}", check.label, check.detail),
        CheckStatus::Fail => match &check.hint {
            Some(hint) => format!("fail {}: {} - {}", check.label, check.detail, hint),
            None => format!("fail {}: {}", check.label, check.detail),
        },
    }
}

pub(crate) fn format_report(checks: &[Check]) -> Vec<String> {
    checks.iter().map(format_check).collect()
}

pub(crate) fn report_failed(checks: &[Check]) -> bool {
    checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Fail))
}

fn layout_dir_check(label: &str, path: &Path) -> Check {
    if path.is_dir() {
        ok_check(label, path.display().to_string())
    } else {
        fail_check(label, "missing", Some("run smstatus init"))
    }
}

fn layout_file_check(label: &str, path: &Path) -> Check {
    if path.is_file() {
        ok_check(label, path.display().to_string())
    } else {
        fail_check(label, "missing", Some("run smstatus init"))
    }
}

pub(crate) fn check_host_layout(config_dir: &Path) -> Vec<Check> {
    let modules_dir = config_dir.join("modules");
    let extensions_dir = config_dir.join("extensions");
    let config_path = config_dir.join(PROGRAM_CONFIG_FILE);

    vec![
        layout_dir_check("config dir", config_dir),
        layout_dir_check("modules/", &modules_dir),
        layout_dir_check("extensions/", &extensions_dir),
        layout_file_check("config.toml", &config_path),
        match read_active_name(config_dir) {
            Ok(name) => ok_check("active preset", name),
            Err(err) => fail_check(
                "active preset",
                err.to_string(),
                Some("fix [presets].active in config.toml or run smstatus init"),
            ),
        },
        match active_config_path(config_dir) {
            Ok(path) => ok_check("preset file", path.display().to_string()),
            Err(err) => fail_check(
                "preset file",
                err.to_string(),
                Some("run smstatus preset list or smstatus init"),
            ),
        },
        match discover_module_kinds(&modules_dir) {
            Ok(kinds) => ok_check("installed modules", format!("{} kinds", kinds.len())),
            Err(err) => fail_check("installed modules", err.to_string(), None::<String>),
        },
        match list_extensions_in(&extensions_dir) {
            Ok(names) => ok_check("installed extensions", format!("{} packages", names.len())),
            Err(err) => fail_check("installed extensions", err.to_string(), None::<String>),
        },
    ]
}

fn extension_registry_for_config(config_dir: &Path) -> Result<ExtensionRegistry> {
    Ok(ExtensionRegistry::new(
        config_dir.join("extensions"),
        crate::lock::lock_dir()?.join("extensions"),
    ))
}

fn host_modules_api_label() -> String {
    format!(
        "{}.{}.{}",
        HOST_MODULES_API.0, HOST_MODULES_API.1, HOST_MODULES_API.2
    )
}

fn incompatible_extension_hint(name: &str, required: &[RequiredExtension]) -> String {
    let req = required
        .iter()
        .find(|req| req.name == name)
        .map(|req| format!("{}.{}", req.version.major, req.version.minor))
        .unwrap_or_else(|| "0.0".to_string());
    format!("upgrade extension {name} to >= {req}")
}

fn check_kind_name(kind: &str) -> Check {
    if is_safe_extension_name(kind) {
        ok_check("kind name", kind)
    } else {
        fail_check(
            "kind name",
            format!("invalid module name `{kind}`"),
            None::<String>,
        )
    }
}

fn check_module_dir(modules_dir: &Path, kind: &str) -> Check {
    let module_dir = manifest::module_dir(modules_dir, kind);
    if module_dir.is_dir() {
        ok_check("module dir", module_dir.display().to_string())
    } else {
        fail_check(
            "module dir",
            "missing",
            Some("smstatus module install <source>"),
        )
    }
}

fn check_module_manifest(modules_dir: &Path, kind: &str) -> (Check, Option<ModuleManifest>) {
    match manifest::read_module_manifest(modules_dir, kind) {
        Ok(parsed) => (
            ok_check(
                "manifest.toml",
                manifest::module_manifest_path(modules_dir, kind)
                    .display()
                    .to_string(),
            ),
            Some(parsed),
        ),
        Err(err) => (
            fail_check(
                "manifest.toml",
                err.to_string(),
                Some(format!(
                    "fix {}",
                    manifest::module_manifest_path(modules_dir, kind).display()
                )),
            ),
            None,
        ),
    }
}

fn check_modules_api(kind: &str, manifest: &ModuleManifest) -> Check {
    let required = (manifest.modules_api.major, manifest.modules_api.minor, 0);
    match version::check_modules_api_compatible(kind, required) {
        Ok(()) => ok_check("modules-api", format!("{}.{}", required.0, required.1)),
        Err(err) => fail_check(
            "modules-api",
            err.to_string(),
            Some(format!(
                "rebuild module for host modules-api {} or upgrade smstatus",
                host_modules_api_label()
            )),
        ),
    }
}

fn check_module_wasm(modules_dir: &Path, kind: &str) -> Check {
    let wasm_path = manifest::module_wasm_path(modules_dir, kind);
    if wasm_path.is_file() {
        ok_check("module.wasm", wasm_path.display().to_string())
    } else {
        fail_check(
            "module.wasm",
            "missing",
            Some("smstatus module install <source>"),
        )
    }
}

fn check_required_extensions(config_dir: &Path, manifest: &ModuleManifest) -> Result<Vec<Check>> {
    let registry = extension_registry_for_config(config_dir)?;
    let unmet = unmet_extensions(&manifest.required_extensions, &registry);
    let mut checks = Vec::new();

    for req in &manifest.required_extensions {
        let failed = unmet.iter().any(|item| match item {
            UnmetExtension::Missing(name) | UnmetExtension::Incompatible(name) => name == &req.name,
        });
        if !failed {
            checks.push(ok_check(format!("extension {}", req.name), "installed"));
        }
    }

    for item in &unmet {
        match item {
            UnmetExtension::Missing(name) => {
                checks.push(fail_check(
                    format!("extension {name}"),
                    "missing",
                    Some(format!(
                        "install extension {name} (smstatus extension install <source>)"
                    )),
                ));
            }
            UnmetExtension::Incompatible(name) => {
                checks.push(fail_check(
                    format!("extension {name}"),
                    "incompatible version",
                    Some(incompatible_extension_hint(
                        name,
                        &manifest.required_extensions,
                    )),
                ));
            }
        }
    }

    Ok(checks)
}

fn check_config_schema_probe(modules_dir: &Path, kind: &str) -> Check {
    match SchemaProbe::new() {
        Ok(schema_probe) => match schema_probe.read_after_stable(modules_dir, kind) {
            Ok(schema) => match crate::schema::validate_schema(kind, kind, &schema) {
                Ok(()) => ok_check("config-schema", "valid"),
                Err(err) => fail_check(
                    "config-schema",
                    err.to_string(),
                    Some("rebuild module.wasm"),
                ),
            },
            Err(err) => fail_check(
                "config-schema",
                err.to_string(),
                Some("rebuild module.wasm"),
            ),
        },
        Err(err) => fail_check(
            "config-schema",
            err.to_string(),
            Some("rebuild module.wasm"),
        ),
    }
}

pub(crate) fn check_module_kind(config_dir: &Path, kind: &str, probe: bool) -> Result<Vec<Check>> {
    let modules_dir = config_dir.join("modules");
    if !modules_dir.is_dir() {
        return Ok(vec![fail_check(
            "module dir",
            "config layout incomplete",
            Some("run smstatus init"),
        )]);
    }

    let mut checks = Vec::new();
    checks.push(check_kind_name(kind));
    checks.push(check_module_dir(&modules_dir, kind));

    let (manifest_check, manifest) = check_module_manifest(&modules_dir, kind);
    checks.push(manifest_check);

    if let Some(ref parsed) = manifest {
        checks.push(check_modules_api(kind, parsed));
    }

    let wasm_path = manifest::module_wasm_path(&modules_dir, kind);
    checks.push(check_module_wasm(&modules_dir, kind));

    if let Some(ref parsed) = manifest {
        checks.extend(check_required_extensions(config_dir, parsed)?);
    }

    if probe && wasm_path.is_file() {
        checks.push(check_config_schema_probe(&modules_dir, kind));
    }

    Ok(checks)
}

pub(crate) fn cmd_doctor() -> Result<(Vec<String>, bool)> {
    let config_dir = crate::config::default_config_dir()?;
    let checks = check_host_layout(&config_dir);
    let failed = report_failed(&checks);
    Ok((format_report(&checks), failed))
}

pub(crate) fn cmd_module_doctor(kind: &str, probe: bool) -> Result<(Vec<String>, bool)> {
    let config_dir = crate::config::default_config_dir()?;
    let checks = check_module_kind(&config_dir, kind, probe)?;
    let failed = report_failed(&checks);
    Ok((format_report(&checks), failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::init_config_layout;
    use crate::config::test_fixtures::{unique_config_dir, write_program_config};
    use crate::extension::ExtensionRegistry;

    fn registry_with_installed(config_dir: &Path, entries: &[(&str, &str)]) -> ExtensionRegistry {
        let extensions_dir = config_dir.join("extensions");
        std::fs::create_dir_all(&extensions_dir).unwrap();
        for &(name, version) in entries {
            let pkg = extensions_dir.join(name);
            std::fs::create_dir_all(&pkg).unwrap();
            std::fs::write(pkg.join("extension"), "").unwrap();
            std::fs::write(
                pkg.join("manifest.toml"),
                format!(
                    "name = \"{name}\"\nversion = \"{version}\"\nauthor = \"test\"\nextensions-api = {{ major = 0, minor = 1 }}\n"
                ),
            )
            .unwrap();
        }
        ExtensionRegistry::new(extensions_dir, config_dir.join("sockets"))
    }

    fn write_minimal_module(config_dir: &Path, kind: &str, manifest: &str, wasm: Option<&[u8]>) {
        let modules_dir = config_dir.join("modules");
        let pkg = modules_dir.join(kind);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("manifest.toml"), manifest).unwrap();
        if let Some(bytes) = wasm {
            std::fs::write(pkg.join("module.wasm"), bytes).unwrap();
        }
    }

    #[test]
    fn check_host_layout_flags_missing_tree() {
        let dir = unique_config_dir("doctor-host-missing");
        let checks = check_host_layout(&dir);
        assert!(report_failed(&checks));
        assert!(
            checks
                .iter()
                .any(|check| check.label == "config dir"
                    && matches!(check.status, CheckStatus::Fail))
        );
    }

    #[test]
    fn check_host_layout_passes_after_init() {
        let dir = unique_config_dir("doctor-host-init");
        init_config_layout(&dir, false).unwrap();
        let checks = check_host_layout(&dir);
        assert!(!report_failed(&checks));
    }

    #[test]
    fn check_host_layout_flags_broken_active_preset() {
        let dir = unique_config_dir("doctor-host-broken-preset");
        init_config_layout(&dir, false).unwrap();
        write_program_config(&dir, "[presets]\nactive = \"missing\"\n");
        let checks = check_host_layout(&dir);
        assert!(report_failed(&checks));
        assert!(
            checks
                .iter()
                .any(|check| check.label == "preset file"
                    && matches!(check.status, CheckStatus::Fail))
        );
    }

    #[test]
    fn check_module_kind_reports_missing_dir() {
        let dir = unique_config_dir("doctor-module-missing");
        init_config_layout(&dir, false).unwrap();
        let checks = check_module_kind(&dir, "cpu", false).unwrap();
        assert!(report_failed(&checks));
        assert!(
            checks
                .iter()
                .any(|check| check.label == "module dir"
                    && matches!(check.status, CheckStatus::Fail))
        );
    }

    #[test]
    fn check_module_kind_reports_modules_api_mismatch() {
        let dir = unique_config_dir("doctor-module-api");
        init_config_layout(&dir, false).unwrap();
        write_minimal_module(
            &dir,
            "cpu",
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 9, minor = 0 }\n",
            Some(b"\0asm"),
        );
        let checks = check_module_kind(&dir, "cpu", false).unwrap();
        assert!(report_failed(&checks));
        let line = format_check(
            checks
                .iter()
                .find(|check| check.label == "modules-api")
                .unwrap(),
        );
        assert!(line.starts_with("fail modules-api:"));
    }

    #[test]
    fn check_module_kind_reports_missing_extension() {
        let dir = unique_config_dir("doctor-module-ext-missing");
        init_config_layout(&dir, false).unwrap();
        write_minimal_module(
            &dir,
            "cpu",
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 0, minor = 1 }\nrequired_extensions = [{ name = \"fs\", version = { major = 0, minor = 1 } }]\n",
            Some(b"\0asm"),
        );
        let checks = check_module_kind(&dir, "cpu", false).unwrap();
        assert!(report_failed(&checks));
        assert!(checks.iter().any(|check| {
            check.label == "extension fs"
                && matches!(check.status, CheckStatus::Fail)
                && check
                    .hint
                    .as_deref()
                    .unwrap_or("")
                    .contains("install extension fs")
        }));
    }

    #[test]
    fn check_module_kind_reports_incompatible_extension() {
        let dir = unique_config_dir("doctor-module-ext-bad");
        init_config_layout(&dir, false).unwrap();
        registry_with_installed(&dir, &[("fs", "0.1.0")]);
        write_minimal_module(
            &dir,
            "cpu",
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 0, minor = 1 }\nrequired_extensions = [{ name = \"fs\", version = { major = 0, minor = 2 } }]\n",
            Some(b"\0asm"),
        );
        let checks = check_module_kind(&dir, "cpu", false).unwrap();
        assert!(report_failed(&checks));
        let hint = checks
            .iter()
            .find(|check| check.label == "extension fs")
            .and_then(|check| check.hint.clone())
            .unwrap();
        assert!(hint.contains(">= 0.2"));
    }

    #[test]
    fn check_module_kind_ok_minimal_fixture() {
        let dir = unique_config_dir("doctor-module-ok");
        init_config_layout(&dir, false).unwrap();
        write_minimal_module(
            &dir,
            "cpu",
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 0, minor = 1 }\n",
            Some(b"\0asm"),
        );
        let checks = check_module_kind(&dir, "cpu", false).unwrap();
        assert!(!report_failed(&checks));
    }

    #[test]
    fn check_module_kind_reports_ok_for_satisfied_extensions_when_one_missing() {
        let dir = unique_config_dir("doctor-module-ext-partial");
        init_config_layout(&dir, false).unwrap();
        registry_with_installed(&dir, &[("fs", "0.1.0")]);
        write_minimal_module(
            &dir,
            "cpu",
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 0, minor = 1 }\nrequired_extensions = [{ name = \"fs\", version = { major = 0, minor = 1 } }, { name = \"mem\", version = { major = 0, minor = 1 } }]\n",
            Some(b"\0asm"),
        );
        let checks = check_module_kind(&dir, "cpu", false).unwrap();
        assert!(report_failed(&checks));
        assert!(
            checks
                .iter()
                .any(|check| check.label == "extension fs"
                    && matches!(check.status, CheckStatus::Ok))
        );
        assert!(checks.iter().any(
            |check| check.label == "extension mem" && matches!(check.status, CheckStatus::Fail)
        ));
    }

    #[test]
    fn check_module_kind_probe_fails_on_invalid_wasm() {
        let dir = unique_config_dir("doctor-module-probe");
        init_config_layout(&dir, false).unwrap();
        write_minimal_module(
            &dir,
            "cpu",
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 0, minor = 1 }\n",
            Some(b"\0asm"),
        );
        let checks = check_module_kind(&dir, "cpu", true).unwrap();
        assert!(report_failed(&checks));
        let line = format_check(
            checks
                .iter()
                .find(|check| check.label == "config-schema")
                .unwrap(),
        );
        assert!(line.starts_with("fail config-schema:"));
    }
}
