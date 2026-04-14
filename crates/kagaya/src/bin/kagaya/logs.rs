use crate::utils::state_dir;
use std::path::PathBuf;

pub fn log_dir() -> PathBuf {
    state_dir().join("logs")
}
