use kagaya::ServiceStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Start {
        names: Vec<String>,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        processes: Vec<String>,
    },
    Stop {
        names: Vec<String>,
        #[serde(default)]
        processes: Vec<String>,
    },
    Reload {
        names: Vec<String>,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        processes: Vec<String>,
    },
    Restart {
        service: String,
        process: String,
    },
    Kill {
        service: String,
        process: String,
    },
    ReloadConfig,
    Status,
    Logs {
        service: String,
        process: Option<String>,
        follow: bool,
        #[serde(default)]
        offset: u64,
    },
    Ping,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok {
        message: Option<String>,
    },
    Status {
        services: Vec<ServiceStatus>,
        http_port: Option<u16>,
    },
    Log {
        line: String,
        #[serde(default)]
        offset: u64,
    },
    Error {
        message: String,
    },
    Progress {
        service: String,
        message: String,
    },
    Pong,
}

fn daemon_paths() -> muzan::DaemonPaths {
    muzan::DaemonPaths::new("kagaya")
}

pub fn state_dir() -> std::path::PathBuf {
    daemon_paths().state_dir()
}

pub fn config_dir() -> std::path::PathBuf {
    daemon_paths().config_dir()
}
