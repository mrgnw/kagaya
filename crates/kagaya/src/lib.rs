//! # kagaya
//!
//! Process supervisor toolkit for Rust CLIs.
//!
//! Spawn, monitor, restart, and capture output from child processes.
//! Pairs with [`muzan`](https://crates.io/crates/muzan) for daemon lifecycle.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use kagaya::{Supervisor, SupervisorConfig, ProcessDef, ServiceType};
//! use std::collections::HashMap;
//!
//! # #[tokio::main]
//! # async fn main() {
//! let sup = Supervisor::new(SupervisorConfig {
//!     log_dir: "/tmp/myapp/logs".into(),
//!     max_log_size: 10 * 1024 * 1024,
//! });
//!
//! let procs = vec![ProcessDef {
//!     name: "web".into(),
//!     command: "echo hello".into(),
//!     service_type: ServiceType::Service,
//!     restart: false,
//!     max_retries: 0,
//!     restart_delay_secs: 0,
//!     env: HashMap::new(),
//!     autostart: true,
//! }];
//!
//! sup.start_service("myapp", "/tmp".as_ref(), &procs, true, &[])
//!     .await
//!     .unwrap();
//! # }
//! ```

pub mod types;
pub mod logs;
pub mod output;
pub mod supervisor;

pub use types::*;
pub use output::OutputCapture;
pub use supervisor::{Supervisor, SupervisorConfig, ManagedService, ManagedProcess};
