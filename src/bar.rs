use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::BarConfig;
use crate::control::ControlListener;
use crate::error::Result;
use crate::extension::ExtensionRegistry;
use crate::host;
use crate::lock;
use crate::logging;
use crate::module::{ModuleRuntime, ModuleState};
use crate::reload::ReloadBatch;
use crate::x11::X11Bar;

const FUEL_PER_TICK: u64 = 10_000_000;
const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run() -> Result<()> {
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

    let (engine, linker) = host::build_engine_and_linker()?;
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
        Arc::clone(&extensions),
    );

    let mut modules = start_modules(&runtime, &config)?;
    let mut listener = ControlListener::new().map_err(|err| err.to_string())?;
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

        if let Some(batch) = listener.wait_for_reload_or_timeout(sleep_for) {
            modules = apply_reload_batch(
                batch,
                &runtime,
                &extensions,
                &config_path,
                modules,
                &mut config,
                &mut separator,
            );
        }
    }
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
    extensions: &ExtensionRegistry,
    config_path: &Path,
    mut modules: Vec<ModuleState>,
    config: &mut BarConfig,
    separator: &mut String,
) -> Vec<ModuleState> {
    if batch.config {
        match BarConfig::load(config_path) {
            Ok(new_config) => {
                *separator = new_config.separator();
                logging::set_retain_days(new_config.log_days());
                modules = runtime.reload(modules, &new_config, &[]);
                *config = new_config;
            }
            Err(err) => {
                log::error!("config reload failed ({err}); keeping previous configuration running");
            }
        }
    }

    if !batch.wasm_kinds.is_empty() {
        modules = runtime.reload_wasm(modules, &batch.wasm_kinds, config);
    }

    if !batch.extension_names.is_empty() {
        extensions.drop_running(&batch.extension_names);
    }

    modules
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::sync::Arc;

    use super::*;
    use crate::host;
    use crate::reload::ReloadBatch;

    fn install_echo(extensions_dir: &Path) {
        let pkg = extensions_dir.join("echo");
        std::fs::create_dir_all(&pkg).unwrap();
        let echo_bin = std::env::var("CARGO_BIN_EXE_echo")
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .unwrap_or_else(|| {
                let mut dir = std::env::current_exe().unwrap();
                dir.pop();
                if dir.ends_with("deps") {
                    dir.pop();
                }
                dir.join("echo")
            });
        symlink(echo_bin, pkg.join("extension")).unwrap();
        std::fs::write(
            pkg.join("manifest.toml"),
            "name = \"echo\"\nversion = \"0.1.0\"\nauthor = \"test\"\nextensions-api = { major = 0, minor = 1 }\n",
        )
        .unwrap();
    }

    #[test]
    fn apply_reload_batch_drops_running_extensions() {
        let base = crate::extension::test_temp_dir("bar-reload-ext");
        let extensions_dir = base.join("extensions");
        let socket_dir = base.join("sockets");
        install_echo(&extensions_dir);

        let extensions = ExtensionRegistry::new(extensions_dir, socket_dir);
        assert_eq!(extensions.call("echo", "ping", "before").unwrap(), "before");

        let config_path = base.join("config.toml");
        std::fs::write(&config_path, "modules = []\n").unwrap();
        let mut config = BarConfig::load(&config_path).unwrap();
        let mut separator = config.separator();

        let (engine, linker) = host::build_engine_and_linker().unwrap();
        let extensions = Arc::new(extensions);
        let runtime = ModuleRuntime::new(
            engine,
            linker,
            base.join("modules"),
            FUEL_PER_TICK,
            Arc::clone(&extensions),
        );

        let batch = ReloadBatch {
            extension_names: vec!["echo".to_string()],
            ..ReloadBatch::default()
        };
        let modules = apply_reload_batch(
            batch,
            &runtime,
            extensions.as_ref(),
            &config_path,
            Vec::new(),
            &mut config,
            &mut separator,
        );
        assert!(modules.is_empty());
        assert_eq!(extensions.call("echo", "ping", "after").unwrap(), "after");
    }
}
