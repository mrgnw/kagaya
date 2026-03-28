pub mod api;
pub mod supervisor;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::config;
use crate::protocol::{Request, Response};
use serde::{Deserialize, Serialize};

const RESUME_SNAPSHOT_FILE: &str = "resume.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResumeSnapshot {
    services: Vec<ResumeService>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResumeService {
    name: String,
    processes: Vec<String>,
}

pub async fn run(args: &[String]) {
    tracing_subscriber::fmt().init();

    let _foreground = args.iter().any(|a| a == "--foreground" || a == "-f");
    let disable_http = args.iter().any(|a| a == "--no-http");
    let port_override = args
        .windows(2)
        .find(|w| w[0] == "--port" || w[0] == "-p")
        .and_then(|w| w[1].parse::<u16>().ok());

    let global_config = config::load_global_config();

    let port = port_override.unwrap_or(global_config.daemon.port);

    let http_port = if disable_http { None } else { Some(port) };
    let supervisor = supervisor::Supervisor::new(global_config.clone(), http_port);

    let paths = muzan::DaemonPaths::new("kagaya");

    let state_dir = paths.state_dir();
    let _ = std::fs::create_dir_all(&state_dir);

    let pid_path = paths.pid_path();
    let _ = std::fs::write(&pid_path, std::process::id().to_string());

    let socket_path = paths.socket_path();
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }

    let log_dir = crate::logs::log_dir();
    kagaya::logs::expire_logs(
        &log_dir,
        global_config.logs.max_age_days,
        global_config.logs.max_files,
    );

    {
        let config = global_config.clone();
        let log_dir = log_dir.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                kagaya::logs::expire_logs(
                    &log_dir,
                    config.logs.max_age_days,
                    config.logs.max_files,
                );
            }
        });
    }

    let shutdown = Arc::new(tokio::sync::Notify::new());
    let preserve_state = Arc::new(AtomicBool::new(false));

    restore_resume_snapshot(&paths, &supervisor).await;

    let sup_socket = Arc::clone(&supervisor);
    let shutdown_socket = Arc::clone(&shutdown);
    let preserve_socket = Arc::clone(&preserve_state);
    let paths_socket = paths.clone();
    let socket_handle = tokio::spawn(async move {
        muzan::server::run_socket_server_with_error(
            &paths_socket,
            move |req: Request| {
                let sup = Arc::clone(&sup_socket);
                let shutdown = Arc::clone(&shutdown_socket);
                let preserve = Arc::clone(&preserve_socket);
                async move { handle_request(&sup, req, &shutdown, &preserve).await }
            },
            Some(|msg: String| Response::Error { message: msg }),
        )
        .await;
    });

    if http_port.is_some() {
        let sup_http = Arc::clone(&supervisor);
        tokio::spawn(async move {
            run_http_server(sup_http, port).await;
        });
    }

    if http_port.is_some() {
        let ui_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
        if ui_dir.join("package.json").exists() {
            tracing::info!("spawning vite dev server in {}", ui_dir.display());
            tokio::spawn(async move {
                let mut child = tokio::process::Command::new("pnpm")
                    .args(["dev"])
                    .current_dir(&ui_dir)
                    .spawn()
                    .expect("failed to start vite dev server");
                child.wait().await.ok();
            });
        }
    }

    tracing::info!("daemon started (pid {})", std::process::id());
    if http_port.is_some() {
        tracing::info!("HTTP server on port {}", port);
    }

    tokio::select! {
        _ = socket_handle => {},
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutting down");
        }
        _ = shutdown.notified() => {
            tracing::info!("shutdown requested");
        }
    }

    if preserve_state.load(Ordering::SeqCst) {
        write_resume_snapshot(&paths, &supervisor).await;
    } else {
        clear_resume_snapshot(&paths);
    }

    // Gracefully stop all supervised processes
    let services: Vec<String> = supervisor
        .inner
        .services
        .read()
        .await
        .keys()
        .cloned()
        .collect();
    for name in &services {
        let _ = supervisor.stop_service(name).await;
    }

    let _ = std::fs::remove_file(paths.socket_path());
    let _ = std::fs::remove_file(paths.pid_path());
}

async fn handle_request(
    supervisor: &Arc<supervisor::Supervisor>,
    request: Request,
    shutdown: &Arc<tokio::sync::Notify>,
    preserve_state: &Arc<AtomicBool>,
) -> Response {
    match request {
        Request::Ping => Response::Pong,
        Request::Status => {
            let services = supervisor.status().await;
            Response::Status {
                services,
                http_port: supervisor.http_port,
            }
        }
        Request::Start {
            names,
            all,
            processes,
            chains,
            wait,
        } => {
            // Build cross-project dependency map from chains
            let mut project_deps: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for chain in &chains {
                for i in 1..chain.len() {
                    // chain[i] depends on chain[i-1] (if both are project names)
                    if names.contains(&chain[i]) && names.contains(&chain[i - 1]) {
                        project_deps
                            .entry(chain[i].clone())
                            .or_default()
                            .push(chain[i - 1].clone());
                    }
                }
            }

            if project_deps.is_empty() {
                // No cross-project deps — start all in parallel as before
                let mut messages = Vec::new();
                for name in &names {
                    // Filter chains to only intra-project ones
                    let intra_chains: Vec<Vec<String>> = chains
                        .iter()
                        .filter(|c| !c.iter().any(|n| names.contains(n) && n != name))
                        .cloned()
                        .collect();
                    match supervisor
                        .start_service_filtered(name, all, &processes, &intra_chains)
                        .await
                    {
                        Ok(msg) => messages.push(msg),
                        Err(e) => return Response::Error { message: e },
                    }
                }
                if wait {
                    for name in &names {
                        supervisor.wait_for_ready(name).await;
                    }
                }
                Response::Ok {
                    message: Some(messages.join("\n")),
                }
            } else {
                // Start projects respecting cross-project dependency order
                let mut started: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut messages = Vec::new();
                let mut remaining: Vec<String> = names.clone();

                while !remaining.is_empty() {
                    // Find projects whose deps are all started and ready
                    let ready_to_start: Vec<String> = remaining
                        .iter()
                        .filter(|name| {
                            project_deps
                                .get(*name)
                                .map(|deps| deps.iter().all(|d| started.contains(d)))
                                .unwrap_or(true)
                        })
                        .cloned()
                        .collect();

                    if ready_to_start.is_empty() {
                        // Shouldn't happen if deps are valid, but prevent infinite loop
                        for name in &remaining {
                            messages.push(format!("{}: skipped (unresolvable dependency)", name));
                        }
                        break;
                    }

                    for name in &ready_to_start {
                        match supervisor
                            .start_service_filtered(name, all, &processes, &[])
                            .await
                        {
                            Ok(msg) => messages.push(msg),
                            Err(e) => messages.push(format!("{}: error: {}", name, e)),
                        }
                    }

                    // Wait for this wave to be ready before starting dependents
                    let has_dependents = remaining.iter().any(|n| !ready_to_start.contains(n));
                    if has_dependents {
                        for name in &ready_to_start {
                            supervisor.wait_for_ready(name).await;
                        }
                    }

                    for name in &ready_to_start {
                        started.insert(name.clone());
                    }
                    remaining.retain(|n| !ready_to_start.contains(n));
                }

                if wait {
                    for name in &names {
                        supervisor.wait_for_ready(name).await;
                    }
                }
                Response::Ok {
                    message: Some(messages.join("\n")),
                }
            }
        }
        Request::Stop { names, processes } => {
            let mut messages = Vec::new();
            for name in &names {
                match supervisor.stop_service_filtered(name, &processes).await {
                    Ok(msg) => messages.push(msg),
                    Err(e) => return Response::Error { message: e },
                }
            }
            Response::Ok {
                message: Some(messages.join("\n")),
            }
        }
        Request::Reload {
            names,
            all,
            processes,
        } => {
            let mut messages = Vec::new();
            for name in &names {
                match supervisor
                    .reload_service_filtered(name, all, &processes)
                    .await
                {
                    Ok(msg) => messages.push(msg),
                    Err(e) => return Response::Error { message: e },
                }
            }
            Response::Ok {
                message: Some(messages.join("\n")),
            }
        }
        Request::Restart { service, process } => {
            match supervisor.restart_process(&service, &process).await {
                Ok(msg) => Response::Ok { message: Some(msg) },
                Err(e) => Response::Error { message: e },
            }
        }
        Request::Kill { service, process } => {
            match supervisor.kill_process(&service, &process).await {
                Ok(msg) => Response::Ok { message: Some(msg) },
                Err(e) => Response::Error { message: e },
            }
        }
        Request::ReloadConfig => {
            // Config is already loaded fresh on each operation, so we just verify it's valid
            match std::panic::catch_unwind(|| config::load_service_entries()) {
                Ok(_) => Response::Ok {
                    message: Some("projects.toml reloaded successfully".to_string()),
                },
                Err(_) => Response::Error {
                    message: "failed to parse projects.toml".to_string(),
                },
            }
        }
        Request::Logs {
            service,
            process,
            follow: _,
            offset,
        } => match supervisor.get_output(&service, process.as_deref()).await {
            Ok(capture) => {
                let (data, new_offset) = capture.snapshot_from(offset).await;
                Response::Log {
                    line: String::from_utf8_lossy(&data).to_string(),
                    offset: new_offset,
                }
            }
            Err(e) => Response::Error { message: e },
        },
        Request::Shutdown {
            preserve_state: should_preserve,
        } => {
            preserve_state.store(should_preserve, Ordering::SeqCst);
            shutdown.notify_one();
            Response::Ok {
                message: Some("shutting down".to_string()),
            }
        }
    }
}

fn resume_snapshot_path(paths: &muzan::DaemonPaths) -> PathBuf {
    paths.state_dir().join(RESUME_SNAPSHOT_FILE)
}

fn clear_resume_snapshot(paths: &muzan::DaemonPaths) {
    let _ = std::fs::remove_file(resume_snapshot_path(paths));
}

async fn write_resume_snapshot(
    paths: &muzan::DaemonPaths,
    supervisor: &Arc<supervisor::Supervisor>,
) {
    let services = supervisor.inner.services.read().await;
    let snapshot = ResumeSnapshot {
        services: services
            .iter()
            .filter_map(|(name, managed)| {
                let processes: Vec<String> = managed
                    .processes
                    .iter()
                    .filter(|(_, process)| process.state.is_running())
                    .map(|(process_name, _)| process_name.clone())
                    .collect();
                if processes.is_empty() {
                    None
                } else {
                    Some(ResumeService {
                        name: name.clone(),
                        processes,
                    })
                }
            })
            .collect(),
    };
    drop(services);

    if snapshot.services.is_empty() {
        clear_resume_snapshot(paths);
        return;
    }

    match serde_json::to_vec_pretty(&snapshot) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(resume_snapshot_path(paths), bytes) {
                tracing::warn!("failed to write resume snapshot: {}", error);
            }
        }
        Err(error) => tracing::warn!("failed to serialize resume snapshot: {}", error),
    }
}

async fn restore_resume_snapshot(
    paths: &muzan::DaemonPaths,
    supervisor: &Arc<supervisor::Supervisor>,
) {
    let path = resume_snapshot_path(paths);
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let snapshot: ResumeSnapshot = match serde_json::from_slice(&bytes) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!(
                "failed to parse resume snapshot {}: {}",
                path.display(),
                error
            );
            let _ = std::fs::remove_file(&path);
            return;
        }
    };
    let _ = std::fs::remove_file(&path);

    if snapshot.services.is_empty() {
        return;
    }

    let requested: HashMap<String, Vec<String>> = snapshot
        .services
        .into_iter()
        .map(|service| (service.name, service.processes))
        .collect();
    let entries = config::load_service_entries();
    let order = match topo_sort_restore_services(&requested, &entries) {
        Ok(order) => order,
        Err(error) => {
            tracing::warn!("failed to order resume snapshot services: {}", error);
            requested.keys().cloned().collect()
        }
    };

    for name in order {
        let Some(processes) = requested.get(&name) else {
            continue;
        };
        if !entries.contains_key(&name) {
            tracing::warn!("skipping resume for missing service '{}'", name);
            continue;
        }
        match supervisor
            .start_service_filtered(&name, false, processes, &[])
            .await
        {
            Ok(message) => tracing::info!("restored {} ({})", name, message),
            Err(error) => {
                tracing::warn!("failed to restore service '{}': {}", name, error);
                continue;
            }
        }
        if requested.keys().any(|other| {
            entries
                .get(other)
                .map(|entry| entry.depends_on.iter().any(|dep| dep == &name))
                .unwrap_or(false)
        }) {
            supervisor.wait_for_ready(&name).await;
        }
    }
}

fn topo_sort_restore_services(
    requested: &HashMap<String, Vec<String>>,
    entries: &std::collections::BTreeMap<String, config::ServiceEntry>,
) -> Result<Vec<String>, String> {
    let requested_names: HashSet<&str> = requested.keys().map(|name| name.as_str()).collect();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for name in &requested_names {
        in_degree.entry(name).or_insert(0);
        let Some(entry) = entries.get(*name) else {
            continue;
        };
        for dep in &entry.depends_on {
            if requested_names.contains(dep.as_str()) {
                *in_degree.entry(name).or_insert(0) += 1;
                dependents.entry(dep.as_str()).or_default().push(name);
            }
        }
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(name, _)| *name)
        .collect();
    let mut order = Vec::new();

    while let Some(name) = queue.pop_front() {
        order.push(name.to_string());
        if let Some(children) = dependents.get(name) {
            for child in children {
                if let Some(degree) = in_degree.get_mut(child) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }
    }

    if order.len() != requested.len() {
        return Err("circular project dependency in resume snapshot".to_string());
    }

    Ok(order)
}

async fn run_http_server(supervisor: Arc<supervisor::Supervisor>, port: u16) {
    let app = api::router(supervisor);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind HTTP on {}: {}", addr, e);
            return;
        }
    };
    tracing::info!("HTTP listening on {}", addr);
    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("HTTP server error: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagaya::{ProcessDef, ProcessState, ServiceType};
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct TestEnv {
        _lock: std::sync::MutexGuard<'static, ()>,
        old_home: Option<OsString>,
        old_xdg_config_home: Option<OsString>,
        old_xdg_state_home: Option<OsString>,
        root: PathBuf,
    }

    impl TestEnv {
        fn new(name: &str) -> Self {
            let lock = ENV_LOCK.lock().unwrap();
            let n = TEST_COUNTER.fetch_add(1, AtomicOrdering::SeqCst);
            let root = std::env::temp_dir().join(format!("kagaya-daemon-test-{}-{}", n, name));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();

            let old_home = std::env::var_os("HOME");
            let old_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
            let old_xdg_state_home = std::env::var_os("XDG_STATE_HOME");

            let home = root.join("home");
            let xdg_config_home = root.join("xdg-config");
            let xdg_state_home = root.join("xdg-state");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(&xdg_config_home).unwrap();
            std::fs::create_dir_all(&xdg_state_home).unwrap();

            unsafe {
                std::env::set_var("HOME", &home);
                std::env::set_var("XDG_CONFIG_HOME", &xdg_config_home);
                std::env::set_var("XDG_STATE_HOME", &xdg_state_home);
            }

            Self {
                _lock: lock,
                old_home,
                old_xdg_config_home,
                old_xdg_state_home,
                root,
            }
        }

        fn paths(&self) -> muzan::DaemonPaths {
            muzan::DaemonPaths::new("kagaya")
        }

        fn config_dir(&self) -> PathBuf {
            crate::protocol::config_dir()
        }

        fn temp_dir(&self, name: &str) -> PathBuf {
            let dir = self.root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            unsafe {
                match &self.old_home {
                    Some(value) => std::env::set_var("HOME", value),
                    None => std::env::remove_var("HOME"),
                }
                match &self.old_xdg_config_home {
                    Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                    None => std::env::remove_var("XDG_CONFIG_HOME"),
                }
                match &self.old_xdg_state_home {
                    Some(value) => std::env::set_var("XDG_STATE_HOME", value),
                    None => std::env::remove_var("XDG_STATE_HOME"),
                }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn proc(name: &str, command: &str) -> ProcessDef {
        ProcessDef {
            name: name.to_string(),
            command: command.to_string(),
            service_type: ServiceType::Service,
            restart: false,
            max_retries: 0,
            restart_delay_secs: 0,
            env: std::collections::HashMap::new(),
            autostart: true,
            pre_start: None,
            ports: vec![],
            depends_on: vec![],
            ready: None,
            ready_timeout: 10,
        }
    }

    fn test_supervisor() -> Arc<supervisor::Supervisor> {
        supervisor::Supervisor::new(config::GlobalConfig::default(), None)
    }

    #[tokio::test]
    async fn write_resume_snapshot_persists_only_running_processes() {
        let env = TestEnv::new("write-resume");
        let paths = env.paths();
        std::fs::create_dir_all(paths.state_dir()).unwrap();
        let supervisor = test_supervisor();
        let dir = env.temp_dir("service");

        let defs = vec![proc("web", "sleep 60"), proc("worker", "sleep 60")];
        supervisor
            .inner
            .start_service("app", &dir, &defs, false, &["web".to_string()])
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        write_resume_snapshot(&paths, &supervisor).await;

        let snapshot_bytes = std::fs::read(resume_snapshot_path(&paths)).unwrap();
        let snapshot: ResumeSnapshot = serde_json::from_slice(&snapshot_bytes).unwrap();
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.services[0].name, "app");
        assert_eq!(snapshot.services[0].processes, vec!["web".to_string()]);

        let _ = supervisor.stop_service("app").await;
    }

    #[tokio::test]
    async fn write_resume_snapshot_clears_file_when_nothing_running() {
        let env = TestEnv::new("clear-resume");
        let paths = env.paths();
        std::fs::create_dir_all(paths.state_dir()).unwrap();
        std::fs::write(resume_snapshot_path(&paths), b"stale").unwrap();
        let supervisor = test_supervisor();

        write_resume_snapshot(&paths, &supervisor).await;

        assert!(!resume_snapshot_path(&paths).exists());
    }

    #[tokio::test]
    async fn restore_resume_snapshot_restores_saved_services_and_processes() {
        let env = TestEnv::new("restore-resume");
        let paths = env.paths();
        std::fs::create_dir_all(paths.state_dir()).unwrap();
        std::fs::create_dir_all(env.config_dir()).unwrap();

        let db_dir = env.temp_dir("db-project");
        let app_dir = env.temp_dir("app-project");
        std::fs::write(db_dir.join("services.toml"), "db = \"sleep 60\"\n").unwrap();
        std::fs::write(
            app_dir.join("services.toml"),
            "web = \"sleep 60\"\nworker = \"sleep 60\"\n",
        )
        .unwrap();
        std::fs::write(
            env.config_dir().join("projects.toml"),
            format!(
                "[db]\ndir = \"{}\"\n\n[app]\ndir = \"{}\"\ndepends_on = \"db\"\n",
                db_dir.display(),
                app_dir.display()
            ),
        )
        .unwrap();

        let snapshot = ResumeSnapshot {
            services: vec![
                ResumeService {
                    name: "app".to_string(),
                    processes: vec!["web".to_string()],
                },
                ResumeService {
                    name: "db".to_string(),
                    processes: vec!["db".to_string()],
                },
            ],
        };
        std::fs::write(
            resume_snapshot_path(&paths),
            serde_json::to_vec_pretty(&snapshot).unwrap(),
        )
        .unwrap();

        let supervisor = test_supervisor();
        restore_resume_snapshot(&paths, &supervisor).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let statuses = supervisor.status().await;
        let db = statuses
            .iter()
            .find(|service| service.name == "db")
            .unwrap();
        let app = statuses
            .iter()
            .find(|service| service.name == "app")
            .unwrap();
        assert!(matches!(
            db.processes[0].state,
            ProcessState::Running { .. }
        ));
        let web = app
            .processes
            .iter()
            .find(|process| process.name == "web")
            .unwrap();
        let worker = app
            .processes
            .iter()
            .find(|process| process.name == "worker")
            .unwrap();
        assert!(matches!(web.state, ProcessState::Running { .. }));
        assert_eq!(worker.state, ProcessState::Stopped);
        assert!(!resume_snapshot_path(&paths).exists());

        let _ = supervisor.stop_service("app").await;
        let _ = supervisor.stop_service("db").await;
    }

    #[test]
    fn topo_sort_restore_services_orders_dependencies() {
        let requested = HashMap::from([
            ("app".to_string(), vec!["web".to_string()]),
            ("db".to_string(), vec!["db".to_string()]),
        ]);
        let entries = BTreeMap::from([
            (
                "app".to_string(),
                config::ServiceEntry {
                    name: "app".to_string(),
                    dir: PathBuf::from("/tmp/app"),
                    inline_command: None,
                    autostart: false,
                    depends_on: vec!["db".to_string()],
                },
            ),
            (
                "db".to_string(),
                config::ServiceEntry {
                    name: "db".to_string(),
                    dir: PathBuf::from("/tmp/db"),
                    inline_command: None,
                    autostart: false,
                    depends_on: vec![],
                },
            ),
        ]);

        let order = topo_sort_restore_services(&requested, &entries).unwrap();
        assert_eq!(order, vec!["db".to_string(), "app".to_string()]);
    }

    #[test]
    fn topo_sort_restore_services_detects_cycles() {
        let requested = HashMap::from([
            ("app".to_string(), vec!["web".to_string()]),
            ("db".to_string(), vec!["db".to_string()]),
        ]);
        let entries = BTreeMap::from([
            (
                "app".to_string(),
                config::ServiceEntry {
                    name: "app".to_string(),
                    dir: PathBuf::from("/tmp/app"),
                    inline_command: None,
                    autostart: false,
                    depends_on: vec!["db".to_string()],
                },
            ),
            (
                "db".to_string(),
                config::ServiceEntry {
                    name: "db".to_string(),
                    dir: PathBuf::from("/tmp/db"),
                    inline_command: None,
                    autostart: false,
                    depends_on: vec!["app".to_string()],
                },
            ),
        ]);

        assert!(topo_sort_restore_services(&requested, &entries).is_err());
    }
}
