use koku::{Request, Response, JobStatus, JobState};
use muzan::{DaemonClient, DaemonPaths};

fn koku_paths() -> DaemonPaths {
	DaemonPaths::new("koku")
}

fn connect() -> Option<DaemonClient<Request, Response>> {
	DaemonClient::connect(&koku_paths()).ok()
}

pub fn fetch_status() -> Option<Vec<JobStatus>> {
	let mut client = connect()?;
	match client.send(&Request::Status) {
		Ok(Response::Status(statuses)) => Some(statuses),
		_ => None,
	}
}

pub fn run_job(name: &str) -> Result<String, String> {
	let mut client = connect().ok_or_else(|| "koku daemon not running".to_string())?;
	match client.send(&Request::Run(name.to_string())) {
		Ok(Response::Ok(msg)) => Ok(msg),
		Ok(Response::Error(msg)) => Err(msg),
		Ok(_) => Err("unexpected response".to_string()),
		Err(e) => Err(format!("{}", e)),
	}
}

pub fn pause_job(name: &str) -> Result<String, String> {
	let mut client = connect().ok_or_else(|| "koku daemon not running".to_string())?;
	match client.send(&Request::Pause(name.to_string())) {
		Ok(Response::Ok(msg)) => Ok(msg),
		Ok(Response::Error(msg)) => Err(msg),
		Ok(_) => Err("unexpected response".to_string()),
		Err(e) => Err(format!("{}", e)),
	}
}

pub fn resume_job(name: &str) -> Result<String, String> {
	let mut client = connect().ok_or_else(|| "koku daemon not running".to_string())?;
	match client.send(&Request::Resume(name.to_string())) {
		Ok(Response::Ok(msg)) => Ok(msg),
		Ok(Response::Error(msg)) => Err(msg),
		Ok(_) => Err("unexpected response".to_string()),
		Err(e) => Err(format!("{}", e)),
	}
}

pub fn reload() -> Result<String, String> {
	let mut client = connect().ok_or_else(|| "koku daemon not running".to_string())?;
	match client.send(&Request::Reload) {
		Ok(Response::Ok(msg)) => Ok(msg),
		Ok(Response::Error(msg)) => Err(msg),
		Ok(_) => Err("unexpected response".to_string()),
		Err(e) => Err(format!("{}", e)),
	}
}

#[allow(dead_code)]
pub fn is_running() -> bool {
	connect().is_some()
}

pub fn state_symbol(state: &JobState) -> &'static str {
	match state {
		JobState::Idle => "○",
		JobState::Running => "●",
		JobState::Paused => "⏸",
		JobState::Failing => "⚠",
		JobState::Stopped => "✖",
	}
}
