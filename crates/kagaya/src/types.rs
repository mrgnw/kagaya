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

/// Runtime state of a supervised process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProcessState {
	Running { pid: u32, uptime_secs: u64 },
	Stopped,
	Crashed { exit_code: i32, retries: u32 },
	Failed { exit_code: i32 },
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
}
