use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInfo {
    pub name: String,
    pub dir: String,
    pub running: bool,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: Option<u32>,
    pub status: String,
    pub autostart: bool,
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDetail {
    pub name: String,
    pub dir: String,
    pub running: bool,
    pub state: String,
    pub processes: Vec<ProcessInfo>,
}

fn aggregate_state(processes: &[ProcessInfo]) -> &'static str {
    let any_running = processes.iter().any(|p| p.status.starts_with("running"));
    let any_failed = processes
        .iter()
        .any(|p| p.status.starts_with("crashed") || p.status.starts_with("failed"));
    match (any_running, any_failed) {
        (true, true) => "degraded",
        (false, true) => "err",
        (true, false) => "on",
        (false, false) => "off",
    }
}

fn home_dir() -> PathBuf {
    PathBuf::from(env::var("HOME").expect("HOME not set"))
}

fn expand_tilde(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("~/") {
        format!("{}/{rest}", home_dir().display())
    } else {
        raw.to_string()
    }
}

fn config_dir() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return Path::new(&xdg).join("kagaya");
    }
    home_dir().join(".config/kagaya")
}

fn projects_config_path() -> PathBuf {
    config_dir().join("projects")
}

fn config_path() -> PathBuf {
    projects_config_path()
}

fn commands_config_path() -> PathBuf {
    config_dir().join("commands")
}

pub struct Service {
    pub name: String,
    pub dir: PathBuf,
}

impl Service {
    pub fn socket_path(&self) -> PathBuf {
        self.dir.join(".kagaya.sock")
    }

    pub fn is_running(&self) -> bool {
        // Just check if we can get status - socket existence isn't enough for kagaya daemon
        self.ky_run(&["status"]).is_ok()
    }

    pub fn info(&self) -> ServiceInfo {
        let processes = self.parse_status();
        let state = aggregate_state(&processes).to_string();
        ServiceInfo {
            name: self.name.clone(),
            dir: self.dir.display().to_string(),
            running: processes.iter().any(|p| p.status.starts_with("running")),
            state,
        }
    }

    pub fn ky_output(&self, args: &[&str]) -> Result<String, String> {
        let mut final_args = vec![self.name.as_str()];
        final_args.extend(args);

        Command::new("ky")
            .args(&final_args)
            .output()
            .map_err(|e| format!("failed to run ky: {e}"))
            .and_then(|out| {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if out.status.success() {
                    Ok(stdout)
                } else {
                    Err(format!("{stdout}{stderr}"))
                }
            })
    }

    pub fn ky_run(&self, args: &[&str]) -> Result<String, String> {
        // For commands that target a project, we pass the project name first
        let mut final_args = vec![args[0]]; // command like "start"
        final_args.push(self.name.as_str()); // project name
        if args.len() > 1 {
            final_args.extend(&args[1..]); // rest of args
        }

        Command::new("ky")
            .args(&final_args)
            .output()
            .map_err(|e| format!("failed to run ky: {e}"))
            .map(|out| {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                format!("{stdout}{stderr}")
            })
    }

    pub fn detail(&self) -> ServiceDetail {
        let processes = self.parse_status();
        let running = processes.iter().any(|p| p.status.starts_with("running"));
        let state = aggregate_state(&processes).to_string();
        ServiceDetail {
            name: self.name.clone(),
            dir: self.dir.display().to_string(),
            running,
            state,
            processes,
        }
    }

    fn parse_status(&self) -> Vec<ProcessInfo> {
        // ky status output format:
        // project   process   pid     status    uptime    ports
        // myapp     web       12345   running   1h 2m     3000
        let output = match self.ky_output(&["status"]) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        output
            .lines()
            .skip(1) // header
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                // Skip the project name (first column)
                if parts.len() < 4 {
                    return None;
                }

                let name = parts.get(1).unwrap_or(&"unknown").to_string();
                let pid = parts.get(2).and_then(|p| p.parse::<u32>().ok());
                let status = parts.get(3).unwrap_or(&"unknown").to_string();

                // Try to parse ports from the end if available
                let mut ports = vec![];
                if let Some(last) = parts.last() {
                    if let Ok(p) = last.parse::<u16>() {
                        ports.push(p);
                    }
                }

                Some(ProcessInfo {
                    name,
                    pid,
                    status,
                    autostart: true,
                    ports,
                })
            })
            .flatten()
            .collect()
    }
}

pub fn load_services() -> BTreeMap<String, Service> {
    let mut services = BTreeMap::new();

    // Load directory-based services from projects
    let path = config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let sep = if line.contains(':') { ':' } else { '\t' };
            let parts: Vec<&str> = line.splitn(2, sep).collect();
            if parts.len() != 2 {
                continue;
            }

            let name = parts[0].trim().to_string();
            let dir = PathBuf::from(expand_tilde(parts[1].trim()));

            services.insert(name.clone(), Service { name, dir });
        }
    }

    // Load command-based services from commands
    let commands_path = commands_config_path();
    if let Ok(content) = fs::read_to_string(&commands_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((name, _cmd)) = line.split_once(':') {
                let name = name.trim().to_string();
                let svc_dir = config_dir().join("_commands").join(&name);
                services.insert(name.clone(), Service { name, dir: svc_dir });
            }
        }
    }

    services
}

pub fn list_services() -> Vec<ServiceInfo> {
    load_services().values().map(|s| s.info()).collect()
}

pub fn get_service_detail(name: &str) -> Result<ServiceDetail, String> {
    let services = load_services();
    match services.get(name) {
        Some(svc) => Ok(svc.detail()),
        None => Err(format!("unknown service: {name}")),
    }
}

pub fn start_service(name: &str) -> Result<String, String> {
    let services = load_services();
    let svc = services
        .get(name)
        .ok_or(format!("unknown service: {name}"))?;
    // ky start handles idempotency
    svc.ky_run(&["start"])
        .map(|out| format!("{name}: started\n{out}"))
}

pub fn stop_service(name: &str) -> Result<String, String> {
    let services = load_services();
    let svc = services
        .get(name)
        .ok_or(format!("unknown service: {name}"))?;

    let result = svc.ky_run(&["stop"]);
    result.map(|out| format!("{name}: stopped\n{out}"))
}

pub fn restart_process(service_name: &str, process_name: &str) -> Result<String, String> {
    let services = load_services();
    let svc = services
        .get(service_name)
        .ok_or(format!("unknown service: {service_name}"))?;

    svc.ky_run(&["restart", process_name])
        .map(|out| format!("{service_name}/{process_name}: restarted\n{out}"))
}

pub fn kill_process(service_name: &str, process_name: &str) -> Result<String, String> {
    let services = load_services();
    let svc = services
        .get(service_name)
        .ok_or(format!("unknown service: {service_name}"))?;

    svc.ky_run(&["kill", process_name])
        .map(|out| format!("{service_name}/{process_name}: killed\n{out}"))
}

pub fn reload_service(name: &str) -> Result<String, String> {
    let services = load_services();
    let svc = services
        .get(name)
        .ok_or(format!("unknown service: {name}"))?;

    svc.ky_run(&["reload"])
        .map(|out| format!("{name}: reloaded\n{out}"))
}
