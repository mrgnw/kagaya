use clap::{Parser, Subcommand};
use std::sync::atomic::{AtomicU8, Ordering};

static OUTPUT_FORMAT: AtomicU8 = AtomicU8::new(0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Human = 0,
    Json = 1,
    Tsv = 2,
}

impl OutputFormat {
    pub fn is_plain(&self) -> bool {
        !matches!(self, OutputFormat::Human)
    }
}

pub fn set_output_format(fmt: OutputFormat) {
    OUTPUT_FORMAT.store(fmt as u8, Ordering::Relaxed);
}

pub fn output_format() -> OutputFormat {
    match OUTPUT_FORMAT.load(Ordering::Relaxed) {
        1 => OutputFormat::Json,
        2 => OutputFormat::Tsv,
        _ => OutputFormat::Human,
    }
}

#[derive(Parser)]
#[command(
    name = "ky",
    version,
    about = "process daemon manager",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true,
    allow_external_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Cmd>,

    /// Output as JSON
    #[arg(long, global = true, conflicts_with = "tsv")]
    pub json: bool,

    /// Output as TSV
    #[arg(long, global = true, conflicts_with = "json")]
    pub tsv: bool,

    /// Watch mode (live refresh)
    #[arg(long, short, global = true)]
    pub watch: bool,

    /// Show help
    #[arg(long, short = 'h', global = true)]
    pub help: bool,

    /// Show version
    #[arg(long, short = 'V', global = true)]
    pub version: bool,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Show status
    #[command(alias = "st")]
    Status {
        /// Service names or --all
        names: Vec<String>,
        #[arg(long, short)]
        all: bool,
        #[arg(long, short)]
        detailed: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Start service(s)
    Start {
        names: Vec<String>,
        #[arg(long, short)]
        all: bool,
        #[arg(long)]
        autostart: bool,
        #[arg(long, short)]
        detailed: bool,
        #[arg(long, short = 'e')]
        echo: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Stop service(s)
    Stop {
        names: Vec<String>,
        #[arg(long, short)]
        all: bool,
        #[arg(long, short)]
        detailed: bool,
        #[arg(long, short = 'e')]
        echo: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Restart service(s) or a single process
    Restart {
        target: Vec<String>,
        #[arg(long, short)]
        all: bool,
        #[arg(long, short)]
        detailed: bool,
        #[arg(long, short = 'e')]
        echo: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Show last 100 lines of log file
    Logs { args: Vec<String> },

    /// Deprecated: use echo instead
    #[command(hide = true)]
    Tail { args: Vec<String> },

    /// Live output stream from daemon
    Echo { args: Vec<String> },

    /// Show services.toml or process command
    Show { args: Vec<String> },

    /// Manage cron jobs (via koku)
    Cron { args: Vec<String> },

    /// Manage the daemon
    Daemon { args: Vec<String> },

    /// HTTP server for web UI
    Serve { args: Vec<String> },

    /// Register a project
    Add { args: Vec<String> },

    /// Unregister a project
    #[command(alias = "rm")]
    Remove { args: Vec<String> },

    /// Create config files
    Init,

    /// Migrate ubermind Procfiles to kagaya TOML
    Migrate {
        #[arg(long, short)]
        force: bool,
    },

    /// Start services on login
    Autostart { args: Vec<String> },

    /// macOS launchd agents
    #[command(alias = "launch", alias = "lctl")]
    Launchd { args: Vec<String> },

    /// Self-management commands
    #[command(name = "self")]
    SelfCmd { args: Vec<String> },

    /// Show status for all services
    All,

    /// Show help
    Help,

    /// Show version
    Version,

    /// Catch-all for service-first syntax (e.g. ky myapp start)
    #[command(external_subcommand)]
    External(Vec<String>),
}
