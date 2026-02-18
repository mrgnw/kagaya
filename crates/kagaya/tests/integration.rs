use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use kagaya::types::*;
use kagaya::supervisor::{Supervisor, SupervisorConfig};
use kagaya::logs;

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(name: &str) -> std::path::PathBuf {
	let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
	let dir = std::env::temp_dir().join(format!("kagaya-test-{}-{}", n, name));
	let _ = std::fs::create_dir_all(&dir);
	dir
}

fn test_supervisor(name: &str) -> (std::sync::Arc<Supervisor>, std::path::PathBuf) {
	let log_dir = temp_dir(name);
	let sup = Supervisor::new(SupervisorConfig {
		log_dir: log_dir.clone(),
		max_log_size: 1024 * 1024,
	});
	(sup, log_dir)
}

fn simple_proc(name: &str, command: &str) -> ProcessDef {
	ProcessDef {
		name: name.to_string(),
		command: command.to_string(),
		service_type: ServiceType::Service,
		restart: false,
		max_retries: 0,
		restart_delay_secs: 1,
		env: HashMap::new(),
		autostart: true,
	}
}

// --- Types ---

#[test]
fn process_state_is_running() {
	assert!(ProcessState::Running { pid: 1, uptime_secs: 0 }.is_running());
	assert!(!ProcessState::Stopped.is_running());
	assert!(!ProcessState::Crashed { exit_code: 1, retries: 0 }.is_running());
	assert!(!ProcessState::Failed { exit_code: 1 }.is_running());
}

#[test]
fn service_status_is_running() {
	let s = ServiceStatus {
		name: "test".into(),
		dir: "/tmp".into(),
		processes: vec![
			ProcessStatus {
				name: "web".into(),
				state: ProcessState::Running { pid: 1, uptime_secs: 5 },
				pid: Some(1),
				autostart: true,
				service_type: ServiceType::Service,
				ports: vec![],
			},
		],
	};
	assert!(s.is_running());

	let s2 = ServiceStatus {
		name: "test".into(),
		dir: "/tmp".into(),
		processes: vec![
			ProcessStatus {
				name: "web".into(),
				state: ProcessState::Stopped,
				pid: None,
				autostart: true,
				service_type: ServiceType::Service,
				ports: vec![],
			},
		],
	};
	assert!(!s2.is_running());
}

// --- Logs ---

#[test]
fn log_parse_date() {
	assert_eq!(logs::parse_log_date("web 26-0214.log"), Some((26, 2, 14)));
	assert_eq!(logs::parse_log_date("invalid"), None);
}

#[test]
fn log_secs_to_datetime() {
	let (y, m, d, h, min) = logs::secs_to_datetime(1771027200);
	assert_eq!((y, m, d, h, min), (2026, 2, 14, 0, 0));
}

#[test]
fn log_current_name_format() {
	let name = logs::current_log_name("web");
	assert!(name.starts_with("web "));
	assert!(name.ends_with(".log"));
}

#[test]
fn log_service_dir() {
	let base = std::path::Path::new("/tmp/logs");
	assert_eq!(logs::service_log_dir(base, "myapp"), base.join("myapp"));
}

// --- Supervisor: start/stop lifecycle ---

#[tokio::test]
async fn supervisor_start_and_stop() {
	let (sup, log_dir) = test_supervisor("start-stop");
	let dir = temp_dir("start-stop-workdir");

	let procs = vec![simple_proc("sleeper", "sleep 60")];
	let result = sup.start_service("test", &dir, &procs, true, &[]).await;
	assert!(result.is_ok());

	// Give it a moment to spawn
	tokio::time::sleep(std::time::Duration::from_millis(200)).await;

	let statuses = sup.status().await;
	assert_eq!(statuses.len(), 1);
	assert_eq!(statuses[0].name, "test");
	assert!(statuses[0].processes[0].state.is_running());

	let result = sup.stop_service("test").await;
	assert!(result.is_ok());

	let statuses = sup.status().await;
	assert!(statuses.is_empty());

	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn supervisor_already_running() {
	let (sup, log_dir) = test_supervisor("already-running");
	let dir = temp_dir("already-running-workdir");

	let procs = vec![simple_proc("sleeper", "sleep 60")];
	let _ = sup.start_service("test", &dir, &procs, true, &[]).await;
	tokio::time::sleep(std::time::Duration::from_millis(200)).await;

	let result = sup.start_service("test", &dir, &procs, true, &[]).await;
	assert!(result.unwrap().contains("already running"));

	let _ = sup.stop_service("test").await;
	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn supervisor_stop_not_running() {
	let (sup, log_dir) = test_supervisor("stop-notrunning");

	let result = sup.stop_service("nonexistent").await;
	assert!(result.is_err());

	let _ = std::fs::remove_dir_all(&log_dir);
}

#[tokio::test]
async fn supervisor_empty_processes() {
	let (sup, log_dir) = test_supervisor("empty-procs");
	let dir = temp_dir("empty-procs-workdir");

	let result = sup.start_service("test", &dir, &[], true, &[]).await;
	assert!(result.is_err());
	assert!(result.unwrap_err().contains("no processes defined"));

	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

// --- Process output capture ---

#[tokio::test]
async fn supervisor_captures_output() {
	let (sup, log_dir) = test_supervisor("output");
	let dir = temp_dir("output-workdir");

	let procs = vec![simple_proc("echo", "echo hello-kagaya")];
	let _ = sup.start_service("test", &dir, &procs, true, &[]).await;

	// Wait for process to run and output to be captured
	tokio::time::sleep(std::time::Duration::from_millis(500)).await;

	let output = sup.get_output("test", Some("echo")).await;
	assert!(output.is_ok());
	let snapshot = output.unwrap().snapshot().await;
	let text = String::from_utf8_lossy(&snapshot);
	assert!(text.contains("hello-kagaya"), "output was: {}", text);

	let _ = sup.stop_service("test").await;
	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

// --- Process exits cleanly ---

#[tokio::test]
async fn supervisor_process_exits_cleanly() {
	let (sup, log_dir) = test_supervisor("clean-exit");
	let dir = temp_dir("clean-exit-workdir");

	let procs = vec![simple_proc("fast", "echo done")];
	let _ = sup.start_service("test", &dir, &procs, true, &[]).await;

	// Wait for process to finish
	tokio::time::sleep(std::time::Duration::from_millis(500)).await;

	let statuses = sup.status().await;
	assert_eq!(statuses.len(), 1);
	let proc = &statuses[0].processes[0];
	assert_eq!(proc.state, ProcessState::Stopped);

	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

// --- Task type doesn't restart ---

#[tokio::test]
async fn task_does_not_restart_on_failure() {
	let (sup, log_dir) = test_supervisor("task-fail");
	let dir = temp_dir("task-fail-workdir");

	let procs = vec![ProcessDef {
		name: "task".to_string(),
		command: "exit 1".to_string(),
		service_type: ServiceType::Task,
		restart: true, // even with restart=true, tasks don't restart
		max_retries: 3,
		restart_delay_secs: 0,
		env: HashMap::new(),
		autostart: true,
	}];

	let _ = sup.start_service("test", &dir, &procs, true, &[]).await;
	tokio::time::sleep(std::time::Duration::from_millis(500)).await;

	let statuses = sup.status().await;
	let proc = &statuses[0].processes[0];
	assert!(matches!(proc.state, ProcessState::Failed { exit_code: 1 }));

	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

// --- Filter by process name ---

#[tokio::test]
async fn supervisor_filter_processes() {
	let (sup, log_dir) = test_supervisor("filter");
	let dir = temp_dir("filter-workdir");

	let procs = vec![
		simple_proc("web", "sleep 60"),
		simple_proc("worker", "sleep 60"),
	];

	// Only start "web"
	let filter = vec!["web".to_string()];
	let _ = sup.start_service("test", &dir, &procs, false, &filter).await;
	tokio::time::sleep(std::time::Duration::from_millis(200)).await;

	let statuses = sup.status().await;
	let web = statuses[0].processes.iter().find(|p| p.name == "web").unwrap();
	let worker = statuses[0].processes.iter().find(|p| p.name == "worker").unwrap();
	assert!(web.state.is_running());
	assert!(!worker.state.is_running());

	let _ = sup.stop_service("test").await;
	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

// --- Kill individual process ---

#[tokio::test]
async fn supervisor_kill_process() {
	let (sup, log_dir) = test_supervisor("kill");
	let dir = temp_dir("kill-workdir");

	let procs = vec![simple_proc("sleeper", "sleep 60")];
	let _ = sup.start_service("test", &dir, &procs, true, &[]).await;
	tokio::time::sleep(std::time::Duration::from_millis(200)).await;

	let result = sup.kill_process("test", "sleeper").await;
	assert!(result.is_ok());

	// Check it's stopped
	let statuses = sup.status().await;
	let proc = &statuses[0].processes[0];
	assert_eq!(proc.state, ProcessState::Stopped);

	let _ = sup.stop_service("test").await;
	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}

// --- Env vars ---

#[tokio::test]
async fn supervisor_passes_env_vars() {
	let (sup, log_dir) = test_supervisor("env");
	let dir = temp_dir("env-workdir");

	let mut env = HashMap::new();
	env.insert("KAGAYA_TEST_VAR".to_string(), "hello123".to_string());
	let procs = vec![ProcessDef {
		name: "env".to_string(),
		command: "echo $KAGAYA_TEST_VAR".to_string(),
		service_type: ServiceType::Service,
		restart: false,
		max_retries: 0,
		restart_delay_secs: 0,
		env,
		autostart: true,
	}];

	let _ = sup.start_service("test", &dir, &procs, true, &[]).await;
	tokio::time::sleep(std::time::Duration::from_millis(500)).await;

	let output = sup.get_output("test", Some("env")).await.unwrap();
	let snapshot = output.snapshot().await;
	let text = String::from_utf8_lossy(&snapshot);
	assert!(text.contains("hello123"), "output was: {}", text);

	let _ = std::fs::remove_dir_all(&log_dir);
	let _ = std::fs::remove_dir_all(&dir);
}
