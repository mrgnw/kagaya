use crate::protocol;
use std::path::PathBuf;

pub fn log_dir() -> PathBuf {
	protocol::state_dir().join("logs")
}

pub fn service_log_dir(service: &str) -> PathBuf {
	kagaya::logs::service_log_dir(&log_dir(), service)
}
