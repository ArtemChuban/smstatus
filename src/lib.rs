mod bar;
mod bindings;
mod cli;
mod config;
mod daemon;
mod error;
pub mod extension;
mod host;
mod lock;
mod logging;
mod meta;
mod module;
mod probe;
mod schema;
mod schema_probe;
mod sysinfo;
mod tui;
mod version;
mod watcher;
mod x11;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Commands, DAEMON_ENV_VAR};

pub fn run() -> ExitCode {
    if std::env::var_os(DAEMON_ENV_VAR).is_some() {
        return daemon::run_daemon();
    }

    match Cli::parse().command {
        Some(Commands::Start) => daemon::cmd_start(),
        Some(Commands::Stop) => daemon::cmd_stop(),
        Some(Commands::Run) => daemon::cmd_run(),
        None => tui::run(),
    }
}
