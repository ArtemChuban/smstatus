use clap::{Parser, Subcommand};

pub(crate) const DAEMON_ENV_VAR: &str = "SMSTATUS_DAEMON_CHILD";
pub(crate) const EXIT_ALREADY_RUNNING: u8 = 3;

#[derive(Parser)]
#[command(
    name = "smstatus",
    about = "suckmore status",
    disable_version_flag = true
)]
pub(crate) struct Cli {
    #[arg(short = 'v', long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

impl Cli {
    pub(crate) fn parse_cli() -> Self {
        let cli = Self::parse();
        if cli.version {
            println!("smstatus {}", crate::version::cli_version_info());
            std::process::exit(0);
        }
        cli
    }
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    Start,
    Stop,
    Run,
    Reload {
        /// Reload active preset (module list, separator, log retention, param re-init)
        #[arg(long)]
        config: bool,
        /// Reload wasm for a module kind (repeatable)
        #[arg(long)]
        module: Vec<String>,
        /// Stop and respawn an extension from disk (repeatable; explicit trust event)
        #[arg(long)]
        extension: Vec<String>,
    },
    Module {
        #[command(subcommand)]
        command: ModuleCommands,
    },
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },
    Preset {
        #[command(subcommand)]
        command: PresetCommands,
    },
}

#[derive(Subcommand)]
pub(crate) enum PresetCommands {
    List,
    Save {
        name: String,
    },
    Use {
        name: String,
        /// Reload a running daemon after switching (without this, the bar keeps the previous preset until `smstatus reload` or restart)
        #[arg(long)]
        reload: bool,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ModuleCommands {
    Install {
        source: String,
        #[arg(long)]
        allow_insecure_http: bool,
        #[arg(long)]
        sha256: Option<String>,
    },
    List,
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ExtensionCommands {
    Install {
        source: String,
        #[arg(long)]
        allow_insecure_http: bool,
        #[arg(long)]
        sha256: Option<String>,
        #[arg(long)]
        force: bool,
    },
    List,
    Remove {
        name: String,
    },
}
