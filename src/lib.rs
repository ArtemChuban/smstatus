mod bar;
mod bindings;
mod cli;
mod config;
mod control;
mod daemon;
mod error;
mod extension;
mod host;
mod install;
mod lock;
mod logging;
mod manifest;
mod meta;
mod module;
mod preset;
mod probe;
mod reload;
mod schema;
mod schema_probe;
mod tui;
mod version;
mod x11;

use std::process::ExitCode;

use cli::{Cli, Commands, DAEMON_ENV_VAR, ExtensionCommands, ModuleCommands, PresetCommands};

fn cli_ok_line(message: &str) -> ExitCode {
    logging::to_stdout(log::Level::Info, message);
    ExitCode::SUCCESS
}

fn cli_ok_lines(lines: impl IntoIterator<Item = String>) -> ExitCode {
    for line in lines {
        logging::to_stdout(log::Level::Info, &line);
    }
    ExitCode::SUCCESS
}

fn cli_err(err: impl std::fmt::Display) -> ExitCode {
    logging::to_stderr(log::Level::Error, &err.to_string());
    ExitCode::FAILURE
}

pub fn run() -> ExitCode {
    if std::env::var_os(DAEMON_ENV_VAR).is_some() {
        return daemon::run_daemon();
    }

    match Cli::parse_cli().command {
        Some(Commands::Start) => daemon::cmd_start(),
        Some(Commands::Stop) => daemon::cmd_stop(),
        Some(Commands::Run) => daemon::cmd_run(),
        Some(Commands::Reload {
            config,
            module,
            extension,
        }) => {
            let request = reload::reload_request_from_cli(config, module, extension);
            match control::notify_running(request) {
                Ok(control::NotifyOutcome::Delivered) => cli_ok_line("reload delivered"),
                Ok(control::NotifyOutcome::NotRunning) => cli_err("smstatus is not running"),
                Err(err) => cli_err(err),
            }
        }
        Some(Commands::Module { command }) => match command {
            ModuleCommands::Install {
                source,
                allow_insecure_http,
                sha256,
            } => {
                let options = install::InstallOptions {
                    allow_insecure_http,
                    expected_sha256: sha256,
                    ..Default::default()
                };
                match install::install_module(&source, &options) {
                    Ok(outcome) => cli_ok_line(&install::format_module_outcome(&outcome)),
                    Err(err) => cli_err(err),
                }
            }
            ModuleCommands::List => match install::list_modules() {
                Ok(lines) => cli_ok_lines(lines),
                Err(err) => cli_err(err),
            },
            ModuleCommands::Remove { name } => match install::remove_module(&name) {
                Ok(()) => cli_ok_line(&format!("removed module `{name}`")),
                Err(err) => cli_err(err),
            },
        },
        Some(Commands::Extension { command }) => match command {
            ExtensionCommands::Install {
                source,
                allow_insecure_http,
                sha256,
            } => {
                let options = install::InstallOptions {
                    allow_insecure_http,
                    expected_sha256: sha256,
                    ..Default::default()
                };
                match install::install_extension(&source, &options) {
                    Ok(outcome) => cli_ok_line(&install::format_extension_outcome(&outcome)),
                    Err(err) => cli_err(err),
                }
            }
            ExtensionCommands::List => match install::list_extensions() {
                Ok(lines) => cli_ok_lines(lines),
                Err(err) => cli_err(err),
            },
            ExtensionCommands::Remove { name } => match install::remove_extension(&name) {
                Ok(()) => cli_ok_line(&format!("removed extension `{name}`")),
                Err(err) => cli_err(err),
            },
        },
        Some(Commands::Preset { command }) => match command {
            PresetCommands::List => match preset::list_presets() {
                Ok(lines) => cli_ok_lines(lines),
                Err(err) => cli_err(err),
            },
            PresetCommands::Save { name } => match preset::save_preset(&name) {
                Ok(()) => cli_ok_line(&format!("saved preset `{name}`")),
                Err(err) => cli_err(err),
            },
            PresetCommands::Use { name, reload } => match preset::use_preset(&name, reload) {
                Ok(preset::UsePresetOutcome::ReloadDelivered) => cli_ok_line("reload delivered"),
                Ok(preset::UsePresetOutcome::ReloadNotRunning) => {
                    cli_ok_line(&format!("active preset set to `{name}`"))
                }
                Ok(preset::UsePresetOutcome::Switched) => {
                    cli_ok_line(&format!("active preset set to `{name}`"))
                }
                Err(err) => cli_err(err),
            },
            PresetCommands::Remove { name } => match preset::remove_preset(&name) {
                Ok(()) => cli_ok_line(&format!("removed preset `{name}`")),
                Err(err) => cli_err(err),
            },
        },
        None => tui::run(),
    }
}
