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
    about = "launchd made easy — manage services with start/stop/restart/status/logs",
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

    /// Watch status live; optional duration in seconds
    #[arg(
        long,
        short,
        global = true,
        value_name = "SECS",
        num_args = 0..=1,
        default_missing_value = "0"
    )]
    pub watch: Option<u64>,

    /// Skip the post-command status watch
    #[arg(long, short = 'W', global = true)]
    pub no_watch: bool,

    /// Watch refresh interval in seconds
    #[arg(long, global = true, hide = true, value_name = "SECS")]
    pub watch_interval: Option<u64>,

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
        /// Service names (service or service.process)
        names: Vec<String>,
        /// Show all services
        #[arg(long, short)]
        all: bool,
        /// Show per-process detail
        #[arg(long, short)]
        detailed: bool,
    },

    /// Start service(s); briefly shows live status after
    Start {
        /// Service names (use .. for chains: db..api starts db, waits, then api)
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
        /// Block until all started processes are ready
        #[arg(long)]
        wait: bool,
        /// Kill foreign processes holding configured ports before starting
        #[arg(long, short)]
        force: bool,
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
    },

    /// Restart service(s) or a single process
    Restart {
        /// Service or service.process targets
        names: Vec<String>,
        /// Restart all services
        #[arg(long, short)]
        all: bool,
        /// Show per-process detail
        #[arg(long, short)]
        detailed: bool,
        /// Stream live output after restarting
        #[arg(long, short = 'e')]
        echo: bool,
        /// Kill foreign processes holding configured ports before starting
        #[arg(long, short)]
        force: bool,
    },

    /// Show log file paths
    Logs {
        /// service or service.process
        target: Vec<String>,
    },

    /// Tail + stream live output from a service
    Echo {
        /// service or service.process
        target: Vec<String>,
        /// Number of trailing lines to print before streaming
        #[arg(long, short = 'n', default_value_t = 14, value_name = "LINES")]
        lines: usize,
    },

    /// Show service config or a single process command
    Show {
        /// service or service.process
        target: Vec<String>,
    },

    /// Manage cron jobs (via koku)
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Cron { args: Vec<String> },

    /// Reload projects.toml and re-sync plists
    #[command(visible_alias = "rc")]
    ReloadConfig,

    /// HTTP server for web UI
    Serve {
        #[command(subcommand)]
        action: Option<ServeAction>,
    },

    /// Register a service (project directory or standalone command)
    Add {
        args: Vec<String>,
        /// Register a standalone command instead of a project directory
        #[arg(long)]
        run: Option<String>,
    },

    /// Unregister a service
    #[command(
        visible_alias = "rm",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    Remove { args: Vec<String> },

    /// Create config files
    Init,

    /// Manage boot autostart (on/off/status)
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Autostart { args: Vec<String> },

    /// Self-management (update)
    #[command(name = "self", trailing_var_arg = true, allow_hyphen_values = true)]
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

#[derive(Subcommand)]
pub enum ServeAction {
    /// Stop the running server
    Stop,
    /// Restart the running server
    Restart,
    /// Show server status
    Status,
    /// Run as background daemon (default)
    Daemon,
    /// Run in foreground (blocks terminal)
    Foreground,
}
