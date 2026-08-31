use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant};

use crate::config::{
    BarConfig, BarConfigLoad, DEFAULT_LOG_DAYS, IDLE_STATUS_MESSAGE, load_bar_config,
};
use crate::control::{ControlListener, WaitResult};
use crate::error::Result;
use crate::extension::{
    ExtensionCallAudit, ExtensionEventBus, ExtensionRegistry, encode_status_snapshot,
};
use crate::host;
use crate::lock;
use crate::logging;
use crate::module::{ModuleRuntime, ModuleState};
use crate::reload::ReloadBatch;
use crate::x11::X11Bar;

const FUEL_PER_TICK: u64 = 10_000_000;
const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(100);

enum InitialBarMode {
    Idle,
    Active {
        modules: Vec<ModuleState>,
        separator: String,
    },
}

fn activate_config(
    loaded: BarConfig,
    runtime: &ModuleRuntime,
    config: &Arc<RwLock<BarConfig>>,
    init_logging: bool,
) -> Option<(Vec<ModuleState>, String)> {
    let separator = loaded.separator();
    let log_days = loaded.log_days();

    if init_logging {
        if let Err(err) = logging::init(log_days) {
            logging::to_stderr(
                log::Level::Error,
                &format!("failed to initialize logging: {err}"),
            );
        }
    } else {
        logging::set_retain_days(log_days);
    }

    *config.write().unwrap_or_else(PoisonError::into_inner) = loaded;
    let modules = start_modules(runtime, config).ok()?;
    if modules.is_empty() {
        log::warn!("configured modules failed to start; staying idle ({IDLE_STATUS_MESSAGE})");
        *config.write().unwrap_or_else(PoisonError::into_inner) = BarConfig::empty();
        return None;
    }
    Some((modules, separator))
}

fn resolve_initial_bar_mode(
    config_dir: &Path,
    runtime: &ModuleRuntime,
    config: &Arc<RwLock<BarConfig>>,
) -> InitialBarMode {
    match load_bar_config(config_dir) {
        BarConfigLoad::Ready { config: loaded } => {
            match activate_config(loaded, runtime, config, true) {
                Some((modules, separator)) => InitialBarMode::Active { modules, separator },
                None => InitialBarMode::Idle,
            }
        }
        BarConfigLoad::Idle => {
            if let Err(err) = logging::init(DEFAULT_LOG_DAYS) {
                logging::to_stderr(
                    log::Level::Error,
                    &format!("failed to initialize logging: {err}"),
                );
            }
            InitialBarMode::Idle
        }
    }
}

fn try_leave_idle(
    config_dir: &Path,
    config: &Arc<RwLock<BarConfig>>,
    runtime: &ModuleRuntime,
) -> Option<(Vec<ModuleState>, String)> {
    match load_bar_config(config_dir) {
        BarConfigLoad::Ready { config: loaded } => activate_config(loaded, runtime, config, false),
        BarConfigLoad::Idle => None,
    }
}

pub(crate) fn run() -> Result<()> {
    let config_dir: PathBuf = crate::config::default_config_dir()?;
    let modules_dir = config_dir.join("modules");

    let (engine, linker) = host::build_engine_and_linker()?;
    let x11_bar = X11Bar::connect()?;
    let extensions = Arc::new(ExtensionRegistry::with_bus(
        config_dir.join("extensions"),
        lock::lock_dir()?.join("extensions"),
        Arc::new(ExtensionEventBus::new()),
    ));
    let audit = Arc::new(ExtensionCallAudit::new());
    let extensions_dir = config_dir.join("extensions");

    let (event_wake_tx, event_wake_rx) = std::sync::mpsc::channel();
    let runtime = ModuleRuntime::new(
        engine,
        linker,
        modules_dir.clone(),
        FUEL_PER_TICK,
        Arc::clone(&extensions),
        Arc::clone(&audit),
        Some(event_wake_tx),
    );

    let config = Arc::new(RwLock::new(BarConfig::empty()));
    let mut idle = true;
    let mut modules = Vec::new();
    let mut separator = " | ".to_string();

    match resolve_initial_bar_mode(&config_dir, &runtime, &config) {
        InitialBarMode::Active {
            modules: started,
            separator: loaded_separator,
        } => {
            modules = started;
            separator = loaded_separator;
            idle = false;
        }
        InitialBarMode::Idle => {}
    }

    let status_registry = Arc::clone(&extensions);
    let status_audit = Arc::clone(&audit);
    let status_config = Arc::clone(&config);
    let status_modules_dir = modules_dir.clone();
    let status_extensions_dir = extensions_dir.clone();
    let status_provider = Some(Box::new(move || {
        let config = status_config.read().unwrap_or_else(PoisonError::into_inner);
        encode_status_snapshot(
            status_registry.as_ref(),
            status_audit.as_ref(),
            &status_extensions_dir,
            &status_modules_dir,
            &config,
            Some(std::process::id() as i32),
            ExtensionCallAudit::MAX_RECORDS,
        )
    }) as Box<dyn Fn() -> String + Send>);
    let mut listener =
        ControlListener::new(status_provider, event_wake_rx).map_err(|err| err.to_string())?;
    let mut last_logged = String::new();

    loop {
        let sleep_for = if idle {
            x11_bar.set_status(IDLE_STATUS_MESSAGE)?;

            if last_logged != IDLE_STATUS_MESSAGE {
                log::info!("root name set to: {IDLE_STATUS_MESSAGE}");
                last_logged = IDLE_STATUS_MESSAGE.to_string();
            }

            DEFAULT_TICK_INTERVAL
        } else {
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

            next_sleep_duration(&modules)
        };

        match listener.wait_for_reload_or_timeout(sleep_for) {
            Some(WaitResult::ExtensionWake(names)) => {
                if !idle {
                    mark_modules_due(&mut modules, &names);
                }
            }
            Some(WaitResult::Reload(batch)) => {
                if idle {
                    if batch.config
                        && let Some((new_modules, new_separator)) =
                            try_leave_idle(&config_dir, &config, &runtime)
                    {
                        modules = new_modules;
                        separator = new_separator;
                        idle = false;
                        last_logged.clear();
                    }
                } else {
                    let (updated_modules, enter_idle) = apply_reload_batch(
                        batch,
                        &runtime,
                        &extensions,
                        &config_dir,
                        modules,
                        &config,
                        &mut separator,
                    );
                    modules = updated_modules;
                    if enter_idle {
                        modules.clear();
                        runtime.clear_extension_event_state();
                        *config.write().unwrap_or_else(PoisonError::into_inner) =
                            BarConfig::empty();
                        idle = true;
                        log::warn!("{IDLE_STATUS_MESSAGE}");
                        last_logged.clear();
                    }
                }
            }
            None => {}
        }
    }
}

fn start_modules(
    runtime: &ModuleRuntime,
    config: &Arc<RwLock<BarConfig>>,
) -> Result<Vec<ModuleState>> {
    let config = config.read().unwrap_or_else(PoisonError::into_inner);
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

fn mark_modules_due(modules: &mut [ModuleState], names: &[String]) {
    let now = Instant::now();
    for state in modules {
        if names.iter().any(|name| name == state.name()) {
            state.force_due(now);
        }
    }
}

fn apply_reload_batch(
    batch: ReloadBatch,
    runtime: &ModuleRuntime,
    extensions: &ExtensionRegistry,
    config_dir: &Path,
    mut modules: Vec<ModuleState>,
    config: &Arc<RwLock<BarConfig>>,
    separator: &mut String,
) -> (Vec<ModuleState>, bool) {
    let mut enter_idle = false;

    if batch.config {
        match load_bar_config(config_dir) {
            BarConfigLoad::Ready { config: new_config } => {
                *separator = new_config.separator();
                logging::set_retain_days(new_config.log_days());
                modules = runtime.reload(modules, &new_config, &[]);
                *config.write().unwrap_or_else(PoisonError::into_inner) = new_config;
            }
            BarConfigLoad::Idle => {
                enter_idle = true;
                modules.clear();
            }
        }
    }

    let config_guard = config.read().unwrap_or_else(PoisonError::into_inner);

    if !batch.wasm_kinds.is_empty() {
        modules = runtime.reload_wasm(modules, &batch.wasm_kinds, &config_guard);
    }

    if !batch.extension_names.is_empty() {
        extensions.drop_running(&batch.extension_names);
    }

    (modules, enter_idle)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::{Arc, OnceLock};

    use super::*;
    use crate::config::test_fixtures::{self, unique_config_dir};
    use crate::host;
    use crate::reload::ReloadBatch;

    fn test_runtime(base: &Path) -> ModuleRuntime {
        let (engine, linker) = host::build_engine_and_linker().unwrap();
        let extensions = Arc::new(ExtensionRegistry::new(
            base.join("extensions"),
            base.join("sockets"),
        ));
        let audit = Arc::new(ExtensionCallAudit::new());
        ModuleRuntime::new(
            engine,
            linker,
            base.join("modules"),
            FUEL_PER_TICK,
            extensions,
            audit,
            None,
        )
    }

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

    fn find_or_build_extension(name: &str) -> PathBuf {
        static TIME: OnceLock<PathBuf> = OnceLock::new();

        let (cache, package) = match name {
            "time" => (&TIME, "smstatus-time"),
            other => panic!("unsupported extension fixture `{other}`"),
        };

        cache
            .get_or_init(|| {
                if let Ok(path) = std::env::var(format!("CARGO_BIN_EXE_{name}")) {
                    let path = PathBuf::from(path);
                    if path.exists() {
                        return path;
                    }
                }

                let mut dir = std::env::current_exe().unwrap();
                dir.pop();
                if dir.ends_with("deps") {
                    dir.pop();
                }
                let bin = dir.join(name);
                if bin.exists() {
                    return bin;
                }

                let target_dir = dir.parent().expect("debug dir has a target-dir parent");
                let status = std::process::Command::new(env!("CARGO"))
                    .current_dir(env!("CARGO_MANIFEST_DIR"))
                    .args(["build", "-p", package, "--target-dir"])
                    .arg(target_dir)
                    .status()
                    .unwrap_or_else(|e| panic!("failed to spawn cargo build -p {package}: {e}"));
                assert!(
                    status.success() && bin.exists(),
                    "extension fixture missing at {}; build with `cargo build -p {package} --target-dir {}`",
                    bin.display(),
                    target_dir.display()
                );
                bin
            })
            .clone()
    }

    fn find_or_build_datetime_wasm() -> PathBuf {
        static DATETIME: OnceLock<PathBuf> = OnceLock::new();
        DATETIME
            .get_or_init(|| {
                let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let wasm = manifest_dir
                    .join("target/wasm32-wasip2/debug")
                    .join("datetime.wasm");
                if wasm.exists() {
                    return wasm;
                }

                let status = std::process::Command::new(env!("CARGO"))
                    .current_dir(&manifest_dir)
                    .args([
                        "build",
                        "-p",
                        "datetime",
                        "--target",
                        "wasm32-wasip2",
                    ])
                    .status()
                    .unwrap_or_else(|e| panic!("failed to spawn cargo build -p datetime: {e}"));
                assert!(
                    status.success() && wasm.exists(),
                    "datetime wasm missing at {}; build with `cargo build -p datetime --target wasm32-wasip2`",
                    wasm.display()
                );
                wasm
            })
            .clone()
    }

    fn install_time_extension(extensions_dir: &Path) {
        let pkg = extensions_dir.join("time");
        std::fs::create_dir_all(&pkg).unwrap();
        symlink(find_or_build_extension("time"), pkg.join("extension")).unwrap();
        std::fs::write(
            pkg.join("manifest.toml"),
            "name = \"time\"\nversion = \"0.1.0\"\nauthor = \"test\"\nextensions-api = { major = 0, minor = 1 }\n",
        )
        .unwrap();
    }

    fn install_datetime_module(modules_dir: &Path) {
        let pkg = modules_dir.join("datetime");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::copy(find_or_build_datetime_wasm(), pkg.join("module.wasm")).unwrap();
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("modules/datetime/manifest.toml"),
            pkg.join("manifest.toml"),
        )
        .unwrap();
    }

    #[test]
    fn missing_config_enters_idle() {
        let dir = unique_config_dir("bar-idle-missing");
        let config = Arc::new(RwLock::new(BarConfig::empty()));
        let runtime = test_runtime(&dir);

        assert!(matches!(
            resolve_initial_bar_mode(&dir, &runtime, &config),
            InitialBarMode::Idle
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_modules_enters_idle() {
        let dir = unique_config_dir("bar-idle-empty-modules");
        test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
        test_fixtures::write_preset(&dir, "default", "modules = []\n");

        let config = Arc::new(RwLock::new(BarConfig::empty()));
        let runtime = test_runtime(&dir);

        assert!(matches!(
            resolve_initial_bar_mode(&dir, &runtime, &config),
            InitialBarMode::Idle
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn idle_stays_idle_when_modules_not_installed() {
        let dir = unique_config_dir("bar-idle-promote");
        test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
        test_fixtures::write_preset(&dir, "default", "modules = []\n");

        let config = Arc::new(RwLock::new(BarConfig::empty()));
        let runtime = test_runtime(&dir);

        assert!(matches!(
            resolve_initial_bar_mode(&dir, &runtime, &config),
            InitialBarMode::Idle
        ));

        test_fixtures::write_preset(&dir, "default", "modules = [\"cpu\"]\n");
        assert!(matches!(load_bar_config(&dir), BarConfigLoad::Ready { .. }));

        assert!(try_leave_idle(&dir, &config, &runtime).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn idle_promotes_to_active_when_modules_installed() {
        let dir = unique_config_dir("bar-idle-promote-ok");
        std::fs::create_dir_all(dir.join("modules")).unwrap();
        std::fs::create_dir_all(dir.join("extensions")).unwrap();
        test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
        test_fixtures::write_preset(&dir, "default", "modules = []\n");

        let config = Arc::new(RwLock::new(BarConfig::empty()));
        let runtime = test_runtime(&dir);

        assert!(matches!(
            resolve_initial_bar_mode(&dir, &runtime, &config),
            InitialBarMode::Idle
        ));

        install_time_extension(&dir.join("extensions"));
        install_datetime_module(&dir.join("modules"));
        test_fixtures::write_preset(&dir, "default", "modules = [\"datetime\"]\n");

        let result = try_leave_idle(&dir, &config, &runtime);
        assert!(result.is_some());
        let (modules, separator) = result.unwrap();
        assert!(!modules.is_empty());
        assert_eq!(
            config.read().unwrap().module_names().unwrap(),
            vec!["datetime".to_string()]
        );
        assert_eq!(separator, " | ");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_reload_batch_enters_idle_when_modules_cleared() {
        let dir = unique_config_dir("bar-active-to-idle");
        test_fixtures::write_program_config(&dir, "[presets]\nactive = \"default\"\n");
        test_fixtures::write_preset(&dir, "default", "modules = [\"cpu\"]\n");

        let preset_path = crate::config::preset_file(&dir, "default").unwrap();
        let config = Arc::new(RwLock::new(BarConfig::load(&preset_path).unwrap()));
        let mut separator = config.read().unwrap().separator();

        let runtime = test_runtime(&dir);
        let extensions = Arc::new(ExtensionRegistry::new(
            dir.join("extensions"),
            dir.join("sockets"),
        ));

        test_fixtures::write_preset(&dir, "default", "modules = []\n");

        let batch = ReloadBatch {
            config: true,
            ..ReloadBatch::default()
        };
        let (modules, enter_idle) = apply_reload_batch(
            batch,
            &runtime,
            extensions.as_ref(),
            &dir,
            Vec::new(),
            &config,
            &mut separator,
        );
        assert!(enter_idle);
        assert!(modules.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
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
        std::fs::write(&config_path, "[presets]\nactive = \"default\"\n").unwrap();
        let preset_path = base.join("presets").join("default.toml");
        std::fs::create_dir_all(preset_path.parent().unwrap()).unwrap();
        std::fs::write(&preset_path, "modules = []\n").unwrap();
        let mut config = Arc::new(RwLock::new(BarConfig::load(&preset_path).unwrap()));
        let mut separator = config.read().unwrap().separator();

        let (engine, linker) = host::build_engine_and_linker().unwrap();
        let extensions = Arc::new(extensions);
        let audit = Arc::new(ExtensionCallAudit::new());
        let runtime = ModuleRuntime::new(
            engine,
            linker,
            base.join("modules"),
            FUEL_PER_TICK,
            Arc::clone(&extensions),
            audit,
            None,
        );

        let batch = ReloadBatch {
            extension_names: vec!["echo".to_string()],
            ..ReloadBatch::default()
        };
        let (modules, enter_idle) = apply_reload_batch(
            batch,
            &runtime,
            extensions.as_ref(),
            &base,
            Vec::new(),
            &mut config,
            &mut separator,
        );
        assert!(modules.is_empty());
        assert!(!enter_idle);
        assert_eq!(extensions.call("echo", "ping", "after").unwrap(), "after");
    }
}
