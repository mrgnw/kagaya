use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use kagaya::logs;
use kagaya::supervisor::{Supervisor, SupervisorConfig};
use kagaya::types::*;
use serde_json::Value;

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(name: &str) -> std::path::PathBuf {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("kagaya-test-{}-{}", n, name));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn cli_temp_dir(name: &str) -> PathBuf {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = PathBuf::from("/tmp").join(format!("kagaya-cli-test-{}-{}", n, name));
    let _ = std::fs::remove_dir_all(&dir);
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
        pre_start: None,
        ports: vec![],
        depends_on: vec![],
        ready: None,
        ready_timeout: 10,
    }
}

struct CliTestEnv {
    root: PathBuf,
    home: PathBuf,
    xdg_config_home: PathBuf,
    xdg_state_home: PathBuf,
    project_dir: PathBuf,
}

impl CliTestEnv {
    fn new(name: &str) -> Self {
        let root = cli_temp_dir(name);
        let home = root.join("home");
        let xdg_config_home = root.join("xdg-config");
        let xdg_state_home = root.join("xdg-state");
        let project_dir = root.join("project");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&xdg_config_home).unwrap();
        std::fs::create_dir_all(&xdg_state_home).unwrap();
        std::fs::create_dir_all(&project_dir).unwrap();
        Self {
            root,
            home,
            xdg_config_home,
            xdg_state_home,
            project_dir,
        }
    }

    fn daemon_pid_path(&self) -> PathBuf {
        self.xdg_state_home.join("kagaya").join("daemon.pid")
    }

    fn setup_project(&self) {
        std::fs::create_dir_all(self.xdg_config_home.join("kagaya")).unwrap();
        std::fs::write(
            self.project_dir.join("services.toml"),
            "web = \"sleep 60\"\n",
        )
        .unwrap();
        std::fs::write(
            self.xdg_config_home.join("kagaya").join("projects.toml"),
            format!("[app]\ndir = \"{}\"\n", self.project_dir.display()),
        )
        .unwrap();
    }

    fn command(&self, binary: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(binary);
        cmd.args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg_config_home)
            .env("XDG_STATE_HOME", &self.xdg_state_home);
        cmd
    }

    fn run_json(&self, binary: &Path, args: &[&str]) -> Value {
        let output = self.command(binary, args).output().unwrap();
        assert!(
            output.status.success(),
            "command failed: {}\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn wait_for_service_running(&self, binary: &Path, service: &str) {
        for _ in 0..50 {
            let status = self.run_json(binary, &["--json", "status", service]);
            let running = status["services"]
                .as_array()
                .and_then(|services| services.iter().find(|entry| entry["name"] == service))
                .and_then(|service| service["processes"].as_array())
                .map(|processes| {
                    processes
                        .iter()
                        .any(|process| process["state"].get("Running").is_some())
                })
                .unwrap_or(false);
            if running {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("service '{service}' did not become running in time");
    }

    fn wait_for_daemon_pid_change(&self, binary: &Path, previous_pid: u64) -> u64 {
        for _ in 0..50 {
            let status = self.run_json(binary, &["--json", "daemon", "status"]);
            let running = status["running"].as_bool().unwrap_or(false);
            let pid = status["pid"].as_u64().unwrap_or(0);
            if running && pid != 0 && pid != previous_pid {
                return pid;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("daemon pid did not change after restart");
    }
}

impl Drop for CliTestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// --- Types ---

#[test]
fn process_state_is_running() {
    assert!(ProcessState::Running {
        pid: 1,
        uptime_secs: 0
    }
    .is_running());
    assert!(!ProcessState::Stopped.is_running());
    assert!(!ProcessState::Crashed {
        exit_code: 1,
        retries: 0
    }
    .is_running());
    assert!(!ProcessState::Failed { exit_code: 1 }.is_running());
}

#[test]
fn service_status_is_running() {
    let s = ServiceStatus {
        name: "test".into(),
        dir: "/tmp".into(),
        processes: vec![ProcessStatus {
            name: "web".into(),
            state: ProcessState::Running {
                pid: 1,
                uptime_secs: 5,
            },
            pid: Some(1),
            autostart: true,
            service_type: ServiceType::Service,
            ports: vec![],
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        }],
    };
    assert!(s.is_running());

    let s2 = ServiceStatus {
        name: "test".into(),
        dir: "/tmp".into(),
        processes: vec![ProcessStatus {
            name: "web".into(),
            state: ProcessState::Stopped,
            pid: None,
            autostart: true,
            service_type: ServiceType::Service,
            ports: vec![],
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        }],
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
    assert_eq!(result.unwrap(), "nonexistent: not running");

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
        pre_start: None,
        ports: vec![],
        depends_on: vec![],
        ready: None,
        ready_timeout: 10,
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
    let _ = sup
        .start_service("test", &dir, &procs, false, &filter)
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let statuses = sup.status().await;
    let web = statuses[0]
        .processes
        .iter()
        .find(|p| p.name == "web")
        .unwrap();
    let worker = statuses[0]
        .processes
        .iter()
        .find(|p| p.name == "worker")
        .unwrap();
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
        pre_start: None,
        ports: vec![],
        depends_on: vec![],
        ready: None,
        ready_timeout: 10,
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

// --- Topological sort ---

#[test]
fn toposort_no_deps() {
    let procs = vec![
        simple_proc("a", "echo a"),
        simple_proc("b", "echo b"),
        simple_proc("c", "echo c"),
    ];
    let order = kagaya::toposort_processes(&procs, &["a", "b", "c"]).unwrap();
    assert_eq!(order.len(), 3);
    assert!(order.contains(&"a".to_string()));
    assert!(order.contains(&"b".to_string()));
    assert!(order.contains(&"c".to_string()));
}

#[test]
fn toposort_linear_chain() {
    let mut b = simple_proc("b", "echo b");
    b.depends_on = vec!["a".to_string()];
    let mut c = simple_proc("c", "echo c");
    c.depends_on = vec!["b".to_string()];
    let procs = vec![simple_proc("a", "echo a"), b, c];
    let order = kagaya::toposort_processes(&procs, &["c"]).unwrap();
    assert_eq!(order.len(), 3);
    let pos_a = order.iter().position(|x| x == "a").unwrap();
    let pos_b = order.iter().position(|x| x == "b").unwrap();
    let pos_c = order.iter().position(|x| x == "c").unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_b < pos_c);
}

#[test]
fn toposort_pulls_transitive_deps() {
    let mut b = simple_proc("b", "echo b");
    b.depends_on = vec!["a".to_string()];
    let mut c = simple_proc("c", "echo c");
    c.depends_on = vec!["b".to_string()];
    let procs = vec![simple_proc("a", "echo a"), b, c];
    let order = kagaya::toposort_processes(&procs, &["c"]).unwrap();
    assert_eq!(order.len(), 3);
    assert!(order.contains(&"a".to_string()));
    assert!(order.contains(&"b".to_string()));
}

#[test]
fn toposort_detects_cycle() {
    let mut a = simple_proc("a", "echo a");
    a.depends_on = vec!["b".to_string()];
    let mut b = simple_proc("b", "echo b");
    b.depends_on = vec!["a".to_string()];
    let procs = vec![a, b];
    let result = kagaya::toposort_processes(&procs, &["a", "b"]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("circular dependency"));
}

#[test]
fn toposort_diamond_dependency() {
    let mut b = simple_proc("b", "echo b");
    b.depends_on = vec!["a".to_string()];
    let mut c = simple_proc("c", "echo c");
    c.depends_on = vec!["a".to_string()];
    let mut d = simple_proc("d", "echo d");
    d.depends_on = vec!["b".to_string(), "c".to_string()];
    let procs = vec![simple_proc("a", "echo a"), b, c, d];
    let order = kagaya::toposort_processes(&procs, &["d"]).unwrap();
    assert_eq!(order.len(), 4);
    let pos_a = order.iter().position(|x| x == "a").unwrap();
    let pos_b = order.iter().position(|x| x == "b").unwrap();
    let pos_c = order.iter().position(|x| x == "c").unwrap();
    let pos_d = order.iter().position(|x| x == "d").unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_a < pos_c);
    assert!(pos_b < pos_d);
    assert!(pos_c < pos_d);
}

// --- depends_on integration ---

#[tokio::test]
async fn depends_on_starts_dependency_first() {
    let (sup, log_dir) = test_supervisor("deps");
    let dir = temp_dir("deps-workdir");

    // "marker" is a task that creates a file; "checker" depends on it
    let marker_file = dir.join("marker.txt");
    let marker = ProcessDef {
        name: "marker".to_string(),
        command: format!("echo ready > {}", marker_file.display()),
        service_type: ServiceType::Task,
        restart: false,
        max_retries: 0,
        restart_delay_secs: 0,
        env: HashMap::new(),
        autostart: true,
        pre_start: None,
        ports: vec![],
        depends_on: vec![],
        ready: None,
        ready_timeout: 10,
    };
    let checker = ProcessDef {
        name: "checker".to_string(),
        command: format!("cat {}", marker_file.display()),
        service_type: ServiceType::Task,
        restart: false,
        max_retries: 0,
        restart_delay_secs: 0,
        env: HashMap::new(),
        autostart: true,
        pre_start: None,
        ports: vec![],
        depends_on: vec!["marker".to_string()],
        ready: None,
        ready_timeout: 10,
    };

    let procs = vec![marker, checker];
    let _ = sup.start_service("test", &dir, &procs, true, &[]).await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // checker should have been able to read the marker file
    let output = sup.get_output("test", Some("checker")).await.unwrap();
    let snapshot = output.snapshot().await;
    let text = String::from_utf8_lossy(&snapshot);
    assert!(text.contains("ready"), "checker output was: {}", text);

    let _ = std::fs::remove_dir_all(&log_dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_daemon_restart_restores_running_services_with_new_binary_path() {
    let env = CliTestEnv::new("daemon-restart");
    env.setup_project();

    let binary = PathBuf::from(env!("CARGO_BIN_EXE_ky"));
    let binary_copy = env.root.join("ky-new");
    std::fs::copy(&binary, &binary_copy).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&binary_copy).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary_copy, perms).unwrap();
    }

    env.run_json(&binary, &["--json", "start", "app"]);
    env.wait_for_service_running(&binary, "app");

    let before = env.run_json(&binary, &["--json", "daemon", "status"]);
    let old_pid = before["pid"].as_u64().unwrap();
    assert!(before["running"].as_bool().unwrap());
    assert!(env.daemon_pid_path().exists());

    let restart = env.run_json(&binary_copy, &["--json", "daemon", "restart"]);
    assert_eq!(restart["ok"].as_bool(), Some(true));
    let new_pid = env.wait_for_daemon_pid_change(&binary_copy, old_pid);
    assert_ne!(old_pid, new_pid);
    assert_eq!(
        std::fs::read_to_string(env.daemon_pid_path())
            .unwrap()
            .trim(),
        new_pid.to_string()
    );

    env.wait_for_service_running(&binary_copy, "app");
    let final_status = env.run_json(&binary_copy, &["--json", "status", "app"]);
    let service = final_status["services"]
        .as_array()
        .unwrap()
        .first()
        .unwrap();
    assert_eq!(service["name"], "app");
    assert!(service["processes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|process| process["state"].get("Running").is_some()));

    let _ = env.run_json(&binary_copy, &["--json", "daemon", "stop"]);
}
