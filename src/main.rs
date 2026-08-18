mod bar;
mod bindings;
mod cli;
mod config;
mod daemon;
mod error;
mod host;
mod lock;
mod module;
mod sysinfo;
mod watcher;
mod x11;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Commands, DAEMON_ENV_VAR};

fn main() -> ExitCode {
    if std::env::var_os(DAEMON_ENV_VAR).is_some() {
        return daemon::run_daemon();
    }

    match Cli::parse().command {
        Commands::Start => daemon::cmd_start(),
        Commands::Stop => daemon::cmd_stop(),
        Commands::Run => daemon::cmd_run(),
    }
}
