mod bar;
mod bindings;
mod cli;
mod config;
mod daemon;
mod error;
mod extension;
mod host;
mod install;
mod lock;
mod logging;
mod meta;
mod module;
mod probe;
mod schema;
mod schema_probe;
mod tui;
mod version;
mod watcher;
mod x11;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Commands, DAEMON_ENV_VAR, ExtensionCommands, ModuleCommands};

pub fn run() -> ExitCode {
    if std::env::var_os(DAEMON_ENV_VAR).is_some() {
        return daemon::run_daemon();
    }

    match Cli::parse().command {
        Some(Commands::Start) => daemon::cmd_start(),
        Some(Commands::Stop) => daemon::cmd_stop(),
        Some(Commands::Run) => daemon::cmd_run(),
        Some(Commands::Module { command }) => match command {
            ModuleCommands::Install { source } => match install::install_module(&source) {
                Ok(outcome) => {
                    logging::to_stdout(log::Level::Info, &install::format_module_outcome(&outcome));
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    logging::to_stderr(log::Level::Error, &err.to_string());
                    ExitCode::FAILURE
                }
            },
        },
        Some(Commands::Extension { command }) => match command {
            ExtensionCommands::Install { source } => match install::install_extension(&source) {
                Ok(outcome) => {
                    logging::to_stdout(
                        log::Level::Info,
                        &install::format_extension_outcome(&outcome),
                    );
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    logging::to_stderr(log::Level::Error, &err.to_string());
                    ExitCode::FAILURE
                }
            },
        },
        None => tui::run(),
    }
}
