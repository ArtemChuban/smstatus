use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use crate::bindings::GuestModule;
use crate::config::BarConfig;
use crate::error::Result;
use crate::extension::{ExtensionCallAudit, ExtensionRegistry};
use crate::host::{self, HostState};
use crate::manifest::RequiredExtension;
use crate::version;

pub(crate) struct ModuleState {
    kind: String,
    name: String,
    component: Component,
    store: Store<HostState>,
    module: GuestModule,
    config: String,
    permissions: Arc<[extension_protocol::PermissionEntry]>,
    last_output: String,
    next_due: Instant,
}

impl ModuleState {
    pub(crate) fn last_output(&self) -> &str {
        &self.last_output
    }

    pub(crate) fn next_due(&self) -> Instant {
        self.next_due
    }
}

pub(crate) struct ModuleRuntime {
    engine: Engine,
    linker: Linker<HostState>,
    modules_dir: PathBuf,
    fuel_per_tick: u64,
    extensions: Arc<ExtensionRegistry>,
    audit: Arc<ExtensionCallAudit>,
    validated_kinds: RefCell<HashSet<String>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnmetExtension {
    Missing(String),
    Incompatible(String),
}

pub(crate) fn unmet_extensions(
    required: &[RequiredExtension],
    registry: &ExtensionRegistry,
) -> Vec<UnmetExtension> {
    let mut unmet = Vec::new();
    for req in required {
        if !registry.is_installed(&req.name) {
            unmet.push(UnmetExtension::Missing(req.name.clone()));
            continue;
        }
        let Ok(installed) = registry.installed_package_version(&req.name) else {
            unmet.push(UnmetExtension::Incompatible(req.name.clone()));
            continue;
        };
        if !version::package_version_meets_floor(installed, req.version.major, req.version.minor) {
            unmet.push(UnmetExtension::Incompatible(req.name.clone()));
        }
    }
    unmet
}

pub(crate) fn wait_wasm_stable(path: &Path) {
    let mut last_size = None;
    for _ in 0..3 {
        let size = std::fs::metadata(path).ok().map(|m| m.len());
        if size.is_some() && size == last_size {
            return;
        }
        last_size = size;
        thread::sleep(Duration::from_millis(50));
    }
}

impl ModuleRuntime {
    pub(crate) fn new(
        engine: Engine,
        linker: Linker<HostState>,
        modules_dir: PathBuf,
        fuel_per_tick: u64,
        extensions: Arc<ExtensionRegistry>,
        audit: Arc<ExtensionCallAudit>,
    ) -> Self {
        Self {
            engine,
            linker,
            modules_dir,
            fuel_per_tick,
            extensions,
            audit,
            validated_kinds: RefCell::new(HashSet::new()),
        }
    }

    fn wasm_path(&self, kind: &str) -> PathBuf {
        crate::manifest::module_wasm_path(&self.modules_dir, kind)
    }

    fn start_after_stable(&self, kind: &str, name: &str, config: &str) -> Result<ModuleState> {
        wait_wasm_stable(&self.wasm_path(kind));
        self.validated_kinds.borrow_mut().remove(kind);
        self.start(kind, name, config)
    }

    fn instantiate(
        &self,
        component: &Component,
        permissions: Arc<[extension_protocol::PermissionEntry]>,
    ) -> Result<(Store<HostState>, GuestModule)> {
        host::instantiate_component(
            &self.engine,
            &self.linker,
            component,
            Arc::clone(&self.extensions),
            permissions,
            Arc::clone(&self.audit),
            self.fuel_per_tick,
        )
    }

    pub(crate) fn start(&self, kind: &str, name: &str, config: &str) -> Result<ModuleState> {
        let manifest = crate::manifest::read_module_manifest(&self.modules_dir, kind)?;
        version::check_modules_api_compatible(
            kind,
            (manifest.modules_api.major, manifest.modules_api.minor, 0),
        )?;

        let unmet = unmet_extensions(&manifest.required_extensions, &self.extensions);
        if !unmet.is_empty() {
            let mut missing = Vec::new();
            let mut incompatible = Vec::new();
            for item in unmet {
                match item {
                    UnmetExtension::Missing(name) => missing.push(name),
                    UnmetExtension::Incompatible(name) => incompatible.push(name),
                }
            }
            let mut parts = Vec::new();
            if !missing.is_empty() {
                let list = missing.join(", ");
                parts.push(format!(
                    "missing extension(s) `{list}`; install extension(s) {list}"
                ));
            }
            if !incompatible.is_empty() {
                let list = incompatible.join(", ");
                parts.push(format!(
                    "incompatible extension(s) `{list}`; upgrade extension(s) {list}"
                ));
            }
            return Err(format!("module `{kind}` requires {}", parts.join("; ")).into());
        }

        let permissions = manifest.frozen_protocol_permissions()?;
        let component = Component::from_file(&self.engine, self.wasm_path(kind))?;
        let (mut store, module) = self.instantiate(&component, Arc::clone(&permissions))?;

        if self.validated_kinds.borrow_mut().insert(kind.to_string()) {
            let schema = module
                .smstatus_module_guest()
                .call_config_schema(&mut store)?;
            crate::schema::validate_schema(kind, name, &schema)?;
        }

        module
            .smstatus_module_guest()
            .call_init(&mut store, config)?;
        Ok(ModuleState {
            kind: kind.to_string(),
            name: name.to_string(),
            component,
            store,
            module,
            config: config.to_string(),
            permissions,
            last_output: String::new(),
            next_due: Instant::now(),
        })
    }

    pub(crate) fn tick(&self, state: &mut ModuleState, now: Instant) -> Result<()> {
        if state.next_due > now {
            return Ok(());
        }

        state.store.set_fuel(self.fuel_per_tick)?;
        state
            .store
            .data_mut()
            .set_caller_module_kind(Some(state.kind.clone()));

        let tick_result = match state
            .module
            .smstatus_module_guest()
            .call_update(&mut state.store)
        {
            Ok(output) => {
                state.last_output = output.text;
                state.next_due = now + Duration::from_millis(output.interval_ms as u64);
                Ok(())
            }
            Err(err) => {
                log::error!(
                    "module `{}` (kind `{}`) tick failed: {err}",
                    state.name,
                    state.kind
                );
                log::error!("re-instantiating module after trap");
                match self.instantiate(&state.component, Arc::clone(&state.permissions)) {
                    Ok((mut store, module)) => {
                        match module
                            .smstatus_module_guest()
                            .call_init(&mut store, &state.config)
                        {
                            Ok(()) => {
                                state.store = store;
                                state.module = module;
                            }
                            Err(err) => {
                                log::error!(
                                    "failed to re-init `{}`, keeping previous instance running: {err}",
                                    state.name
                                );
                            }
                        }
                    }
                    Err(err) => log::error!("failed to re-instantiate `{}`: {err}", state.name),
                }
                state.next_due = now + Duration::from_secs(1);
                Ok(())
            }
        };

        state.store.data_mut().set_caller_module_kind(None);
        tick_result
    }

    fn kind_forced(force_wasm_kinds: &[String], kind: &str) -> bool {
        force_wasm_kinds.iter().any(|k| k == kind)
    }

    pub(crate) fn reload(
        &self,
        old_modules: Vec<ModuleState>,
        new_config: &BarConfig,
        force_wasm_kinds: &[String],
    ) -> Vec<ModuleState> {
        let new_names = match new_config.module_names() {
            Ok(names) => names,
            Err(err) => {
                log::error!("reload aborted, bad `modules` list: {err}");
                return old_modules;
            }
        };

        let mut old_by_name: HashMap<String, Vec<ModuleState>> = HashMap::new();
        for module in old_modules {
            old_by_name
                .entry(module.name.clone())
                .or_default()
                .push(module);
        }

        let mut new_modules = Vec::with_capacity(new_names.len());
        for entry in new_names {
            let (kind, name) = BarConfig::split_module_entry(&entry);
            let config = new_config.module_config_json(name);
            let reused = old_by_name
                .get_mut(name)
                .filter(|v| !v.is_empty())
                .map(|v| v.remove(0));

            match reused {
                Some(existing)
                    if existing.config == config && !Self::kind_forced(force_wasm_kinds, kind) =>
                {
                    new_modules.push(existing);
                }
                Some(existing) if Self::kind_forced(force_wasm_kinds, kind) => {
                    match self.start_after_stable(kind, name, &config) {
                        Ok(state) => new_modules.push(state),
                        Err(err) => {
                            log::error!(
                                "failed to reload wasm for `{name}` (kind `{kind}`), keeping previous instance: {err}"
                            );
                            new_modules.push(existing);
                        }
                    }
                }
                Some(mut existing) => {
                    let reinit_result = existing
                        .store
                        .set_fuel(self.fuel_per_tick)
                        .map_err(|e| e.to_string())
                        .and_then(|()| {
                            existing
                                .module
                                .smstatus_module_guest()
                                .call_init(&mut existing.store, &config)
                                .map_err(|e| e.to_string())
                        });
                    match reinit_result {
                        Ok(()) => {
                            existing.config = config;
                            existing.next_due = Instant::now();
                        }
                        Err(err) => {
                            log::error!(
                                "failed to re-init `{name}` with new config, keeping old config running: {err}"
                            );
                        }
                    }
                    new_modules.push(existing);
                }
                None => match self.start(kind, name, &config) {
                    Ok(state) => new_modules.push(state),
                    Err(err) => log::error!("failed to start new module `{name}`: {err}"),
                },
            }
        }
        new_modules
    }

    pub(crate) fn reload_wasm(
        &self,
        modules: Vec<ModuleState>,
        kinds: &[String],
        config: &BarConfig,
    ) -> Vec<ModuleState> {
        let mut kept = Vec::with_capacity(modules.len());
        for existing in modules {
            if !kinds.iter().any(|k| k == &existing.kind) {
                kept.push(existing);
                continue;
            }

            let path = self.wasm_path(&existing.kind);
            if !path.exists() {
                log::error!(
                    "wasm for `{}` (kind `{}`) missing on disk; dropping instance",
                    existing.name,
                    existing.kind
                );
                continue;
            }

            match self.start_after_stable(&existing.kind, &existing.name, &existing.config) {
                Ok(restarted) => kept.push(restarted),
                Err(err) => {
                    log::error!(
                        "failed to reload wasm for `{}` (kind `{}`), keeping previous instance: {err}",
                        existing.name,
                        existing.kind
                    );
                    kept.push(existing);
                }
            }
        }

        let live_names: HashSet<String> = kept.iter().map(|m| m.name.clone()).collect();
        let Ok(entries) = config.module_names() else {
            return kept;
        };
        for entry in entries {
            let (kind, name) = BarConfig::split_module_entry(&entry);
            if !kinds.iter().any(|k| k == kind) || live_names.contains(name) {
                continue;
            }
            let path = self.wasm_path(kind);
            if !path.exists() {
                continue;
            }
            let module_config = config.module_config_json(name);
            match self.start_after_stable(kind, name, &module_config) {
                Ok(state) => kept.push(state),
                Err(err) => log::error!("failed to start module `{name}` after wasm create: {err}"),
            }
        }

        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ApiVersionReq;

    fn req(name: &str, major: u32, minor: u32) -> RequiredExtension {
        RequiredExtension {
            name: name.to_string(),
            version: ApiVersionReq { major, minor },
        }
    }

    fn registry_with_installed(entries: &[(&str, &str)]) -> ExtensionRegistry {
        let dir = crate::extension::test_temp_dir("module");
        let extensions_dir = dir.join("extensions");
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
        ExtensionRegistry::new(extensions_dir, dir.join("sockets"))
    }

    #[test]
    fn no_required_extensions_is_never_missing() {
        let registry = registry_with_installed(&[]);
        assert_eq!(
            unmet_extensions(&[], &registry),
            Vec::<UnmetExtension>::new()
        );
    }

    #[test]
    fn all_required_extensions_installed_is_not_missing() {
        let registry = registry_with_installed(&[("docker", "0.1.0")]);
        assert_eq!(
            unmet_extensions(&[req("docker", 0, 1)], &registry),
            Vec::<UnmetExtension>::new()
        );
    }

    #[test]
    fn one_missing_extension_is_reported() {
        let registry = registry_with_installed(&[]);
        assert_eq!(
            unmet_extensions(&[req("docker", 0, 1)], &registry),
            vec![UnmetExtension::Missing("docker".to_string())]
        );
    }

    #[test]
    fn several_missing_extensions_are_all_reported() {
        let registry = registry_with_installed(&[("docker", "0.1.0")]);
        assert_eq!(
            unmet_extensions(
                &[req("docker", 0, 1), req("dbus", 0, 1), req("network", 0, 1)],
                &registry
            ),
            vec![
                UnmetExtension::Missing("dbus".to_string()),
                UnmetExtension::Missing("network".to_string()),
            ]
        );
    }

    #[test]
    fn incompatible_extension_version_is_reported() {
        let registry = registry_with_installed(&[("fs", "0.1.0")]);
        assert_eq!(
            unmet_extensions(&[req("fs", 0, 2)], &registry),
            vec![UnmetExtension::Incompatible("fs".to_string())]
        );
        assert_eq!(
            unmet_extensions(&[req("fs", 1, 0)], &registry),
            vec![UnmetExtension::Incompatible("fs".to_string())]
        );
    }
}
