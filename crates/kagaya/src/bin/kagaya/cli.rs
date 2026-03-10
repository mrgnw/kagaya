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
    /// Show service status (default command)
    #[command(visible_alias = "st")]
    Status {
        /// Service names
        names: Vec<String>,
        /// Show all services
        #[arg(long, short)]
        all: bool,
        /// Show per-process detail
        #[arg(long, short)]
        detailed: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Start service(s); briefly shows live status after
    Start {
        /// Service names
        names: Vec<String>,
        /// Start all services
        #[arg(long, short)]
        all: bool,
        /// Start only autostart-enabled services
        #[arg(long)]
        autostart: bool,
        /// Show per-process detail
        #[arg(long, short)]
        detailed: bool,
        /// Stream live output after starting
        #[arg(long, short = 'e')]
        echo: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        /// Skip post-command status watch
        #[arg(long, short = 'W')]
        no_watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Stop service(s)
    Stop {
        /// Service names
        names: Vec<String>,
        /// Stop all services
        #[arg(long, short)]
        all: bool,
        /// Show per-process detail
        #[arg(long, short)]
        detailed: bool,
        /// Stream live output after stopping
        #[arg(long, short = 'e')]
        echo: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        /// Skip post-command status watch
        #[arg(long, short = 'W')]
        no_watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Restart service(s) or a single process
    Restart {
        /// Service or service.process targets
        target: Vec<String>,
        /// Restart all services
        #[arg(long, short)]
        all: bool,
        /// Show per-process detail
        #[arg(long, short)]
        detailed: bool,
        /// Stream live output after restarting
        #[arg(long, short = 'e')]
        echo: bool,
        #[arg(long, short, hide = true)]
        watch: bool,
        /// Skip post-command status watch
        #[arg(long, short = 'W')]
        no_watch: bool,
        #[arg(long, hide = true)]
        watch_interval: Option<u64>,
    },

    /// Show log file paths
    Logs { args: Vec<String> },

    /// Deprecated: use echo instead
    #[command(hide = true)]
    Tail { args: Vec<String> },

    /// Tail + stream live output from a service
    Echo { args: Vec<String> },

    /// Show service config or a single process command
    Show { args: Vec<String> },

    /// Manage cron jobs (via koku)
    Cron { args: Vec<String> },

    /// Manage the daemon process
    Daemon { args: Vec<String> },

    /// Reload projects.toml without restarting services
    #[command(visible_alias = "rc")]
    ReloadConfig,

    /// HTTP server for web UI
    Serve { args: Vec<String> },

    /// Register a project or standalone command
    Add {
        args: Vec<String>,
        /// Register a standalone command instead of a project directory
        #[arg(long)]
        run: Option<String>,
    },

    /// Unregister a project
    #[command(visible_alias = "rm")]
    Remove { args: Vec<String> },

    /// Create config files
    Init,

    /// Migrate Procfiles to kagaya TOML
    Migrate {
        #[arg(long, short)]
        force: bool,
    },

    /// Manage boot autostart (on/off/status)
    Autostart { args: Vec<String> },

    /// macOS launchd agents (run `ky launchd help` for details)
    #[command(visible_alias = "lctl")]
    Launchd { args: Vec<String> },

    /// Self-management (update)
    #[command(name = "self")]
    SelfCmd { args: Vec<String> },

    /// Show status for all services
    #[command(hide = true)]
    All,

    /// Show help
    #[command(hide = true)]
    Help,

    /// Show version
    #[command(hide = true)]
    Version,

    /// Catch-all for service-first syntax (e.g. ky myapp start)
    #[command(external_subcommand)]
    External(Vec<String>),
}
