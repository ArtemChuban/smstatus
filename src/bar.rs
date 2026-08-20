use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::mem::ManuallyDrop;
use std::os::fd::{AsRawFd, FromRawFd};
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
use crate::watcher::ReloadWatcher;
use crate::x11::X11Bar;

const FUEL_PER_TICK: u64 = 10_000_000;
const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(100);
const MAX_LOG_FILE_BYTES: u64 = 5 * 1024 * 1024;

pub(crate) fn run() -> Result<()> {
    let mut wasm_config = Config::new();
    wasm_config.wasm_component_model(true);
    wasm_config.consume_fuel(true);
    let engine = Engine::new(&wasm_config)?;

    let config_dir: PathBuf = crate::config::default_config_dir()?;
    let modules_dir = config_dir.join("modules");
    let config_path = config_dir.join("config.toml");
    let mut config = BarConfig::load(&config_path)?;
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
        modules_dir.clone(),
        FUEL_PER_TICK,
        Arc::clone(x11_bar.connection()),
        http_agent,
    );

    let mut modules = Vec::new();
    for entry in config.module_names()? {
        let (kind, name) = BarConfig::split_module_entry(&entry);
        let module_config = config.module_config_json(name);
        match runtime.start(kind, name, &module_config) {
            Ok(state) => modules.push(state),
            Err(err) => eprintln!("failed to start module `{name}`: {err}"),
        }
    }

    let mut watcher = ReloadWatcher::new(&config_dir, config_path.clone(), modules_dir)?;
    let mut last_logged = String::new();

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

        if combined != last_logged {
            println!("root name set to: {combined}");
            rotate_stdout_log_if_large(MAX_LOG_FILE_BYTES);
            last_logged = combined;
        }

        let sleep_for = modules
            .iter()
            .map(|s| s.next_due().saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(DEFAULT_TICK_INTERVAL);

        if let Some(batch) = watcher.wait_for_reload_or_timeout(sleep_for) {
            if batch.config {
                match BarConfig::load(&config_path) {
                    Ok(new_config) => {
                        separator = new_config.separator();
                        modules = runtime.reload(modules, &new_config, &batch.wasm_kinds);
                        config = new_config;
                    }
                    Err(err) => {
                        eprintln!(
                            "config reload failed ({err}); keeping previous configuration running"
                        )
                    }
                }
            } else if !batch.wasm_kinds.is_empty() {
                modules = runtime.reload_wasm(modules, &batch.wasm_kinds, &config);
            }
        }
    }
}

fn rotate_stdout_log_if_large(max_bytes: u64) {
    // SAFETY: borrows the existing stdout fd without taking ownership of it;
    // wrapping in `ManuallyDrop` ensures it is never closed on drop.
    let mut stdout_file =
        ManuallyDrop::new(unsafe { File::from_raw_fd(std::io::stdout().as_raw_fd()) });
    let too_large = match stdout_file.metadata() {
        Ok(meta) => meta.len() > max_bytes,
        Err(err) => {
            eprintln!("failed to stat stdout log: {err}");
            false
        }
    };
    if too_large
        && let Err(err) = stdout_file
            .set_len(0)
            .and_then(|()| stdout_file.seek(SeekFrom::Start(0)).map(|_| ()))
    {
        eprintln!("failed to rotate stdout log: {err}");
    }
}
