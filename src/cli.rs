use clap::{Parser, Subcommand};

pub(crate) const DAEMON_ENV_VAR: &str = "SMSTATUS_DAEMON_CHILD";
pub(crate) const EXIT_ALREADY_RUNNING: u8 = 3;

#[derive(Parser)]
#[command(
    name = "smstatus",
    about = "suckmore status",
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true
)]
pub(crate) struct Cli {
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    Start,
    Stop,
    Run,
    Module {
        #[command(subcommand)]
        command: ModuleCommands,
    },
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },
}

#[derive(Subcommand)]
pub(crate) enum ModuleCommands {
    Install { source: String },
}

#[derive(Subcommand)]
pub(crate) enum ExtensionCommands {
    Install { source: String },
}
