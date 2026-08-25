use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wasmtime::component::Linker;
use wasmtime::{Config, Engine};

use crate::bindings::GuestModule;
use crate::config::BarConfig;
use crate::error::Result;
use crate::extension::ExtensionRegistry;
use crate::host::HostState;
use crate::lock;
use crate::logging;
use crate::module::{ModuleRuntime, ModuleState};
use crate::watcher::{ReloadBatch, ReloadWatcher};
use crate::x11::X11Bar;

const FUEL_PER_TICK: u64 = 10_000_000;
const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run() -> Result<()> {
    let engine = build_engine()?;
    let config_dir: PathBuf = crate::config::default_config_dir()?;
    let modules_dir = config_dir.join("modules");
    let config_path = config_dir.join("config.toml");
    let mut config = BarConfig::load(&config_path)?;
    let mut separator = config.separator();
    if let Err(err) = logging::init(config.log_days()) {
        logging::to_stderr(
            log::Level::Error,
            &format!("failed to initialize logging: {err}"),
        );
    }

    let linker = build_linker(&engine)?;
    let x11_bar = X11Bar::connect()?;
    let extensions = Arc::new(ExtensionRegistry::new(
        config_dir.join("extensions"),
        lock::lock_dir()?.join("extensions"),
    ));

    let runtime = ModuleRuntime::new(
        engine,
        linker,
        modules_dir.clone(),
        FUEL_PER_TICK,
        extensions,
    );

    let mut modules = start_modules(&runtime, &config)?;
    let mut watcher = ReloadWatcher::new(&config_dir, config_path.clone(), modules_dir)?;
    let mut last_logged = String::new();

    loop {
        let now = Instant::now();
        for state in &mut modules {
            runtime.tick(state, now)?;
        }

        let combined = combined_output(&modules, &separator);
        x11_bar.set_status(&combined)?;

        if combined != last_logged {
            log::info!("root name set to: {combined}");
            last_logged = combined;
        }

        let sleep_for = next_sleep_duration(&modules);

        if let Some(batch) = watcher.wait_for_reload_or_timeout(sleep_for) {
            modules = apply_reload_batch(
                batch,
                &runtime,
                &config_path,
                modules,
                &mut config,
                &mut separator,
            );
        }
    }
}

fn build_engine() -> Result<Engine> {
    let mut wasm_config = Config::new();
    wasm_config.wasm_component_model(true);
    wasm_config.consume_fuel(true);
    Ok(Engine::new(&wasm_config)?)
}

fn build_linker(engine: &Engine) -> Result<Linker<HostState>> {
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    GuestModule::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;
    Ok(linker)
}

fn start_modules(runtime: &ModuleRuntime, config: &BarConfig) -> Result<Vec<ModuleState>> {
    let mut modules = Vec::new();
    for entry in config.module_names()? {
        let (kind, name) = BarConfig::split_module_entry(&entry);
        let module_config = config.module_config_json(name);
        match runtime.start(kind, name, &module_config) {
            Ok(state) => modules.push(state),
            Err(err) => log::error!("failed to start module `{name}`: {err}"),
        }
    }
    Ok(modules)
}

fn combined_output(modules: &[ModuleState], separator: &str) -> String {
    modules
        .iter()
        .map(ModuleState::last_output)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

fn next_sleep_duration(modules: &[ModuleState]) -> Duration {
    modules
        .iter()
        .map(|s| s.next_due().saturating_duration_since(Instant::now()))
        .min()
        .unwrap_or(DEFAULT_TICK_INTERVAL)
}

fn apply_reload_batch(
    batch: ReloadBatch,
    runtime: &ModuleRuntime,
    config_path: &Path,
    modules: Vec<ModuleState>,
    config: &mut BarConfig,
    separator: &mut String,
) -> Vec<ModuleState> {
    if batch.config {
        match BarConfig::load(config_path) {
            Ok(new_config) => {
                *separator = new_config.separator();
                logging::set_retain_days(new_config.log_days());
                let modules = runtime.reload(modules, &new_config, &batch.wasm_kinds);
                *config = new_config;
                modules
            }
            Err(err) => {
                log::error!("config reload failed ({err}); keeping previous configuration running");
                modules
            }
        }
    } else if !batch.wasm_kinds.is_empty() {
        runtime.reload_wasm(modules, &batch.wasm_kinds, config)
    } else {
        modules
    }
}
