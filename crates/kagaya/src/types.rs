use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Whether a process is a long-running service or a one-shot task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    #[default]
    Service,
    Task,
}

/// A named service: a directory containing one or more processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub dir: PathBuf,
    pub processes: Vec<ProcessDef>,
}

/// Definition of a process to supervise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDef {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub service_type: ServiceType,
    #[serde(default = "default_true")]
    pub restart: bool,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_restart_delay")]
    pub restart_delay_secs: u64,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub autostart: bool,
    pub pre_start: Option<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub ready: Option<String>,
    #[serde(default = "default_ready_timeout")]
    pub ready_timeout: u64,
}

fn default_true() -> bool {
    true
}
fn default_max_retries() -> u32 {
    3
}
fn default_restart_delay() -> u64 {
    1
}
fn default_ready_timeout() -> u64 {
    10
}

/// Runtime state of a supervised process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProcessState {
    Running { pid: u32, uptime_secs: u64 },
    Stopped,
    Crashed { exit_code: i32, retries: u32 },
    Failed { exit_code: i32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    On,
    Degraded,
    Err,
    Off,
}

impl ServiceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Degraded => "degraded",
            Self::Err => "err",
            Self::Off => "off",
        }
    }
}

impl ProcessState {
    pub fn is_running(&self) -> bool {
        matches!(self, ProcessState::Running { .. })
    }
}

/// Status snapshot of a service and its processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub dir: PathBuf,
    pub processes: Vec<ProcessStatus>,
}

impl ServiceStatus {
    pub fn is_running(&self) -> bool {
        self.processes.iter().any(|p| p.state.is_running())
    }

    pub fn aggregate_state(&self) -> ServiceState {
        let any_running = self.processes.iter().any(|p| p.state.is_running());
        let any_failed = self.processes.iter().any(|p| {
            matches!(
                p.state,
                ProcessState::Failed { .. } | ProcessState::Crashed { .. }
            )
        });
        match (any_running, any_failed) {
            (true, true) => ServiceState::Degraded,
            (false, true) => ServiceState::Err,
            (true, false) => ServiceState::On,
            (false, false) => ServiceState::Off,
        }
    }
}

/// Status snapshot of a single process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStatus {
    pub name: String,
    pub state: ProcessState,
    pub pid: Option<u32>,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default)]
    pub service_type: ServiceType,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub ports_expected: Vec<u16>,
    #[serde(default)]
    pub state_since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(name: &str, state: ProcessState) -> ProcessStatus {
        ProcessStatus {
            name: name.to_string(),
            state,
            pid: None,
            autostart: true,
            service_type: ServiceType::Service,
            ports: vec![],
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        }
    }

    #[test]
    fn aggregate_state_is_on_when_any_process_running() {
        let service = ServiceStatus {
            name: "svc".into(),
            dir: PathBuf::from("/tmp/svc"),
            processes: vec![proc(
                "web",
                ProcessState::Running {
                    pid: 1,
                    uptime_secs: 3,
                },
            )],
        };
        assert_eq!(service.aggregate_state(), ServiceState::On);
    }

    #[test]
    fn aggregate_state_is_degraded_when_running_and_failed_mix() {
        let service = ServiceStatus {
            name: "svc".into(),
            dir: PathBuf::from("/tmp/svc"),
            processes: vec![
                proc(
                    "web",
                    ProcessState::Running {
                        pid: 1,
                        uptime_secs: 3,
                    },
                ),
                proc("worker", ProcessState::Failed { exit_code: 1 }),
            ],
        };
        assert_eq!(service.aggregate_state(), ServiceState::Degraded);
    }

    #[test]
    fn aggregate_state_is_err_when_only_failed_processes_exist() {
        let service = ServiceStatus {
            name: "svc".into(),
            dir: PathBuf::from("/tmp/svc"),
            processes: vec![proc(
                "web",
                ProcessState::Crashed {
                    exit_code: 1,
                    retries: 2,
                },
            )],
        };
        assert_eq!(service.aggregate_state(), ServiceState::Err);
    }

    #[test]
    fn aggregate_state_is_off_when_everything_stopped() {
        let service = ServiceStatus {
            name: "svc".into(),
            dir: PathBuf::from("/tmp/svc"),
            processes: vec![proc("web", ProcessState::Stopped)],
        };
        assert_eq!(service.aggregate_state(), ServiceState::Off);
    }
}
