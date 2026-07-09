//! # kagaya
//!
//! Shared types for the `ky` CLI — a launchd-backed service manager for macOS.

pub mod toposort;
pub mod types;

pub use toposort::toposort_processes;
pub use types::*;
