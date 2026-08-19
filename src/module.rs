use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};
use x11rb::rust_connection::RustConnection;

use crate::bindings::GuestModule;
use crate::config::BarConfig;
use crate::error::Result;
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
    connection: Arc<RustConnection>,
    http_agent: ureq::Agent,
}

impl ModuleRuntime {
    pub(crate) fn new(
        engine: Engine,
        linker: Linker<HostState>,
        modules_dir: PathBuf,
        fuel_per_tick: u64,
        connection: Arc<RustConnection>,
        http_agent: ureq::Agent,
    ) -> Self {
        Self {
            engine,
            linker,
            modules_dir,
            fuel_per_tick,
            connection,
            http_agent,
        }
    }

    fn instantiate(&self, component: &Component) -> Result<(Store<HostState>, GuestModule)> {
        let state = HostState::new(Arc::clone(&self.connection), self.http_agent.clone());
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| state.limits());
        store.set_fuel(self.fuel_per_tick)?;
        let module = GuestModule::instantiate(&mut store, component, &self.linker)?;
        Ok((store, module))
    }

    pub(crate) fn start(&self, kind: &str, name: &str, config: &str) -> Result<ModuleState> {
        let component =
            Component::from_file(&self.engine, self.modules_dir.join(format!("{kind}.wasm")))?;
        let (mut store, module) = self.instantiate(&component)?;

        let required = module
            .smstatus_module_guest()
            .call_required_host_api_version(&mut store)?;
        version::check_compatible(kind, (required.major, required.minor, required.patch))?;

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
                eprintln!(
                    "module `{}` (kind `{}`) tick failed: {err}",
                    state.name, state.kind
                );
                eprintln!("re-instantiating module after trap");
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
                                eprintln!(
                                    "failed to re-init `{}`, keeping previous instance running: {err}",
                                    state.name
                                );
                            }
                        }
                    }
                    Err(err) => eprintln!("failed to re-instantiate `{}`: {err}", state.name),
                }
                state.next_due = now + Duration::from_secs(1);
            }
        }
        Ok(())
    }

    pub(crate) fn reload(
        &self,
        old_modules: Vec<ModuleState>,
        new_config: &BarConfig,
    ) -> Vec<ModuleState> {
        let new_names = match new_config.module_names() {
            Ok(names) => names,
            Err(err) => {
                eprintln!("reload aborted, bad `modules` list: {err}");
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
                Some(existing) if existing.config == config => {
                    new_modules.push(existing);
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
                            eprintln!(
                                "failed to re-init `{name}` with new config, keeping old config running: {err}"
                            );
                        }
                    }
                    new_modules.push(existing);
                }
                None => match self.start(kind, name, &config) {
                    Ok(state) => new_modules.push(state),
                    Err(err) => eprintln!("failed to start new module `{name}`: {err}"),
                },
            }
        }
        new_modules
    }
}
