use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wasmtime::component::Linker;
use wasmtime::{Config, Engine};

use crate::bindings::GuestModule;
use crate::config::BarConfig;
use crate::error::Result;
use crate::host::HostState;
use crate::module::{ModuleRuntime, ModuleState};
use crate::watcher::ConfigWatcher;
use crate::x11::X11Bar;

const FUEL_PER_TICK: u64 = 10_000_000;
const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run() -> Result<()> {
    let mut wasm_config = Config::new();
    wasm_config.wasm_component_model(true);
    wasm_config.consume_fuel(true);
    let engine = Engine::new(&wasm_config)?;

    let config_dir: PathBuf = dirs::config_dir()
        .ok_or("could not determine config directory")?
        .join("smstatus");
    let modules_dir = config_dir.join("modules");
    let config_path = config_dir.join("config.toml");
    let config = BarConfig::load(&config_path)?;
    let mut separator = config.separator();

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    GuestModule::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |state: &mut HostState| state,
    )?;

    let x11_bar = X11Bar::connect()?;

    let http_agent = {
        let agent_config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build();
        ureq::Agent::new_with_config(agent_config)
    };

    let runtime = ModuleRuntime::new(
        engine,
        linker,
        modules_dir,
        FUEL_PER_TICK,
        Arc::clone(x11_bar.connection()),
        http_agent,
    );

    let mut modules = Vec::new();
    for name in config.module_names()? {
        let module_config = config.module_config_json(&name);
        match runtime.start(&name, &module_config) {
            Ok(state) => modules.push(state),
            Err(err) => eprintln!("failed to start module `{name}`: {err}"),
        }
    }

    let mut watcher = ConfigWatcher::new(&config_dir, config_path.clone())?;

    loop {
        let now = Instant::now();

        for state in &mut modules {
            runtime.tick(state, now)?;
        }

        let combined = modules
            .iter()
            .map(ModuleState::last_output)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(&separator);

        x11_bar.set_status(&combined)?;

        println!("root name set to: {combined}");

        let sleep_for = modules
            .iter()
            .map(|s| s.next_due().saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(DEFAULT_TICK_INTERVAL);

        if watcher.wait_for_reload_or_timeout(sleep_for) {
            match BarConfig::load(&config_path) {
                Ok(new_config) => {
                    separator = new_config.separator();
                    modules = runtime.reload(modules, &new_config);
                }
                Err(err) => {
                    eprintln!(
                        "config reload failed ({err}); keeping previous configuration running"
                    )
                }
            }
        }
    }
}
