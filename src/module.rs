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
use crate::extension::ExtensionRegistry;
use crate::host::HostState;
use crate::version;

pub(crate) struct ModuleState {
    kind: String,
    name: String,
    component: Component,
    store: Store<HostState>,
    module: GuestModule,
    config: String,
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
    validated_kinds: RefCell<HashSet<String>>,
}

fn missing_extensions(required: &[String], registry: &ExtensionRegistry) -> Vec<String> {
    required
        .iter()
        .filter(|name| !registry.is_installed(name))
        .cloned()
        .collect()
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
    ) -> Self {
        Self {
            engine,
            linker,
            modules_dir,
            fuel_per_tick,
            extensions,
            validated_kinds: RefCell::new(HashSet::new()),
        }
    }

    fn wasm_path(&self, kind: &str) -> PathBuf {
        self.modules_dir.join(format!("{kind}.wasm"))
    }

    fn start_after_stable(&self, kind: &str, name: &str, config: &str) -> Result<ModuleState> {
        wait_wasm_stable(&self.wasm_path(kind));
        self.validated_kinds.borrow_mut().remove(kind);
        self.start(kind, name, config)
    }

    fn instantiate(&self, component: &Component) -> Result<(Store<HostState>, GuestModule)> {
        let state = HostState::new(Arc::clone(&self.extensions));
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| state.limits());
        store.set_fuel(self.fuel_per_tick)?;
        let module = GuestModule::instantiate(&mut store, component, &self.linker)?;
        Ok((store, module))
    }

    pub(crate) fn start(&self, kind: &str, name: &str, config: &str) -> Result<ModuleState> {
        let component = Component::from_file(&self.engine, self.wasm_path(kind))?;
        let (mut store, module) = self.instantiate(&component)?;

        let required = module
            .smstatus_module_guest()
            .call_required_host_api_version(&mut store)?;
        version::check_compatible(kind, (required.major, required.minor, required.patch))?;

        let required_extensions = module
            .smstatus_module_guest()
            .call_required_extensions(&mut store)?;
        let missing = missing_extensions(&required_extensions, &self.extensions);
        if !missing.is_empty() {
            let list = missing.join(", ");
            return Err(format!(
                "module `{kind}` requires extension(s) `{list}`; install extension(s) {list}"
            )
            .into());
        }

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
            last_output: String::new(),
            next_due: Instant::now(),
        })
    }

    pub(crate) fn tick(&self, state: &mut ModuleState, now: Instant) -> Result<()> {
        if state.next_due > now {
            return Ok(());
        }

        state.store.set_fuel(self.fuel_per_tick)?;

        match state
            .module
            .smstatus_module_guest()
            .call_update(&mut state.store)
        {
            Ok(output) => {
                state.last_output = output.text;
                state.next_due = now + Duration::from_millis(output.interval_ms as u64);
            }
            Err(err) => {
                log::error!(
                    "module `{}` (kind `{}`) tick failed: {err}",
                    state.name,
                    state.kind
                );
                log::error!("re-instantiating module after trap");
                match self.instantiate(&state.component) {
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
            }
        }
        Ok(())
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

    fn registry_with_installed(names: &[&str]) -> ExtensionRegistry {
        let dir = crate::extension::test_temp_dir("module");
        let extensions_dir = dir.join("extensions");
        std::fs::create_dir_all(&extensions_dir).unwrap();
        for name in names {
            std::fs::write(extensions_dir.join(name), "").unwrap();
        }
        ExtensionRegistry::new(extensions_dir, dir.join("sockets"))
    }

    #[test]
    fn no_required_extensions_is_never_missing() {
        let registry = registry_with_installed(&[]);
        assert_eq!(missing_extensions(&[], &registry), Vec::<String>::new());
    }

    #[test]
    fn all_required_extensions_installed_is_not_missing() {
        let registry = registry_with_installed(&["docker"]);
        assert_eq!(
            missing_extensions(&["docker".to_string()], &registry),
            Vec::<String>::new()
        );
    }

    #[test]
    fn one_missing_extension_is_reported() {
        let registry = registry_with_installed(&[]);
        assert_eq!(
            missing_extensions(&["docker".to_string()], &registry),
            vec!["docker".to_string()]
        );
    }

    #[test]
    fn several_missing_extensions_are_all_reported() {
        let registry = registry_with_installed(&["docker"]);
        assert_eq!(
            missing_extensions(
                &[
                    "docker".to_string(),
                    "dbus".to_string(),
                    "network".to_string(),
                ],
                &registry
            ),
            vec!["dbus".to_string(), "network".to_string()]
        );
    }
}
