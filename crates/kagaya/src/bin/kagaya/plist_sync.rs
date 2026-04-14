use crate::config::ServiceEntry;
use crate::launchd::{get_uid, parse_launchctl_list, user_agents_dir, KAGAYA_PREFIX};
use crate::logs::log_dir;
use crate::utils::listening_ports_for_pids;
use kagaya::types::{ProcessState, ProcessStatus, ServiceStatus, ServiceType};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn label_for(name: &str) -> String {
    format!("{}{}", KAGAYA_PREFIX, name)
}

pub fn plist_path(name: &str) -> PathBuf {
    user_agents_dir().join(format!("{}.plist", label_for(name)))
}

pub fn plist_exists(name: &str) -> bool {
    plist_path(name).exists()
}

fn resolved_command(svc: &ServiceEntry) -> Option<Vec<String>> {
    if let Some(inline) = &svc.inline_command {
        return Some(vec!["/bin/sh".into(), "-c".into(), inline.run.clone()]);
    }
    let services_toml = svc.dir.join("services.toml");
    if services_toml.exists() {
        let content = std::fs::read_to_string(&services_toml).ok()?;
        let root: toml::Value = toml::from_str(&content).ok()?;
        let table = root.as_table()?;
        for (_, value) in table {
            if let Some(run) = value.get("run").and_then(|v| v.as_str()) {
                return Some(vec!["/bin/sh".into(), "-c".into(), run.to_string()]);
            }
        }
    }
    None
}

fn service_env(svc: &ServiceEntry) -> std::collections::HashMap<String, String> {
    svc.inline_command
        .as_ref()
        .map(|c| c.env.clone())
        .unwrap_or_default()
}

pub fn build_plist(svc: &ServiceEntry) -> Option<plist::Value> {
    let label = label_for(&svc.name);
    let command = resolved_command(svc)?;
    let log_root = log_dir();
    let _ = std::fs::create_dir_all(&log_root);
    let stdout_log = log_root.join(format!("{}.log", svc.name));
    let stderr_log = log_root.join(format!("{}.err.log", svc.name));

    let mut dict = plist::Dictionary::new();
    dict.insert("Label".into(), plist::Value::String(label));
    dict.insert(
        "ProgramArguments".into(),
        plist::Value::Array(command.into_iter().map(plist::Value::String).collect()),
    );
    dict.insert(
        "WorkingDirectory".into(),
        plist::Value::String(svc.dir.to_string_lossy().to_string()),
    );
    dict.insert("KeepAlive".into(), plist::Value::Boolean(true));
    dict.insert("RunAtLoad".into(), plist::Value::Boolean(svc.autostart));
    dict.insert(
        "StandardOutPath".into(),
        plist::Value::String(stdout_log.to_string_lossy().to_string()),
    );
    dict.insert(
        "StandardErrorPath".into(),
        plist::Value::String(stderr_log.to_string_lossy().to_string()),
    );

    let env = service_env(svc);
    if !env.is_empty() {
        let mut env_dict = plist::Dictionary::new();
        for (k, v) in env {
            env_dict.insert(k, plist::Value::String(v));
        }
        dict.insert(
            "EnvironmentVariables".into(),
            plist::Value::Dictionary(env_dict),
        );
    }
    Some(plist::Value::Dictionary(dict))
}

pub fn is_loaded(name: &str) -> bool {
    let label = label_for(name);
    Command::new("launchctl")
        .args(["print", &format!("gui/{}/{}", get_uid(), label)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn bootstrap(path: &PathBuf) -> Result<(), String> {
    let target = format!("gui/{}", get_uid());
    let out = Command::new("launchctl")
        .args(["bootstrap", &target, &path.to_string_lossy()])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let legacy = Command::new("launchctl")
        .args(["load", &path.to_string_lossy()])
        .output()
        .map_err(|e| e.to_string())?;
    if legacy.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

pub fn bootout(name: &str) -> Result<(), String> {
    let label = label_for(name);
    let target = format!("gui/{}/{}", get_uid(), label);
    let out = Command::new("launchctl")
        .args(["bootout", &target])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("Could not find specified service") {
        return Ok(());
    }
    let legacy = Command::new("launchctl")
        .args(["unload", &plist_path(name).to_string_lossy()])
        .output()
        .map_err(|e| e.to_string())?;
    if legacy.status.success() {
        return Ok(());
    }
    Err(stderr.trim().to_string())
}

pub fn sync_service(svc: &ServiceEntry) -> Result<(), String> {
    let Some(value) = build_plist(svc) else {
        return Err(format!(
            "no runnable command for '{}' (need inline `run = ...` or services.toml with `run`)",
            svc.name
        ));
    };
    let path = plist_path(&svc.name);
    let _ = std::fs::create_dir_all(user_agents_dir());
    let was_loaded = is_loaded(&svc.name);
    if was_loaded {
        let _ = bootout(&svc.name);
    }
    value
        .to_file_xml(&path)
        .map_err(|e| format!("writing plist: {}", e))?;
    if was_loaded || svc.autostart {
        bootstrap(&path)?;
    }
    Ok(())
}

pub fn remove_service(name: &str) -> Result<(), String> {
    let _ = bootout(name);
    let path = plist_path(name);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("removing plist: {}", e))?;
    }
    Ok(())
}

// ── Readers (launchctl + plist) ───────────────────────────────────────────────

pub struct PlistInfo {
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
    pub run_at_load: bool,
    pub ports: Vec<u16>,
}

pub fn read_plist(name: &str) -> Option<PlistInfo> {
    let path = plist_path(name);
    let value = plist::Value::from_file(&path).ok()?;
    let dict = value.as_dictionary()?;
    Some(PlistInfo {
        stdout_path: dict
            .get("StandardOutPath")
            .and_then(|v| v.as_string())
            .map(PathBuf::from),
        stderr_path: dict
            .get("StandardErrorPath")
            .and_then(|v| v.as_string())
            .map(PathBuf::from),
        run_at_load: dict
            .get("RunAtLoad")
            .and_then(|v| v.as_boolean())
            .unwrap_or(false),
        ports: vec![],
    })
}

pub fn log_paths(name: &str) -> Option<(PathBuf, PathBuf)> {
    let info = read_plist(name)?;
    Some((info.stdout_path?, info.stderr_path?))
}

fn uptime_secs_for_pid(pid: u32) -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "etime=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_etime(&raw)
}

fn parse_etime(s: &str) -> Option<u64> {
    // ps etime formats: "SS", "MM:SS", "HH:MM:SS", "DD-HH:MM:SS"
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, s),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (h, m, sec) = match parts.len() {
        1 => (0u64, 0u64, parts[0].parse::<u64>().ok()?),
        2 => (0, parts[0].parse().ok()?, parts[1].parse().ok()?),
        3 => (
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ),
        _ => return None,
    };
    Some(days * 86400 + h * 3600 + m * 60 + sec)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn status_for(svc: &ServiceEntry) -> ServiceStatus {
    let label = label_for(&svc.name);
    let launchctl = parse_launchctl_list();
    let info = read_plist(&svc.name);
    let run_at_load = info
        .as_ref()
        .map(|i| i.run_at_load)
        .unwrap_or(svc.autostart);

    let entry = launchctl.get(&label);
    let (state, pid) = match entry {
        Some((Some(pid), _)) => (
            ProcessState::Running {
                pid: *pid,
                uptime_secs: uptime_secs_for_pid(*pid).unwrap_or(0),
            },
            Some(*pid),
        ),
        Some((None, Some(exit))) if *exit != 0 => (
            ProcessState::Crashed {
                exit_code: *exit,
                retries: 0,
            },
            None,
        ),
        Some((None, _)) => (ProcessState::Stopped, None),
        None => (ProcessState::Stopped, None),
    };

    let ports: Vec<u16> = pid
        .map(|p| {
            listening_ports_for_pids(&[p])
                .into_values()
                .flatten()
                .collect()
        })
        .unwrap_or_default();

    ServiceStatus {
        name: svc.name.clone(),
        dir: svc.dir.clone(),
        processes: vec![ProcessStatus {
            name: svc.name.clone(),
            state,
            pid,
            autostart: run_at_load,
            service_type: ServiceType::Service,
            ports,
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        }],
    }
}

pub fn query_all(services: &BTreeMap<String, ServiceEntry>) -> Vec<ServiceStatus> {
    services.values().map(status_for).collect()
}

// ── Orchestration (used by start/stop/restart handlers) ──────────────────────

pub struct OpResult {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

pub fn start_services(names: &[String]) -> Vec<OpResult> {
    names.iter().map(|n| start_one(n)).collect()
}

pub fn stop_services(names: &[String]) -> Vec<OpResult> {
    names.iter().map(|n| stop_one(n)).collect()
}

pub fn restart_services(names: &[String]) -> Vec<OpResult> {
    names.iter().map(|n| restart_one(n)).collect()
}

fn start_one(name: &str) -> OpResult {
    let path = plist_path(name);
    if !path.exists() {
        return OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("no plist for '{}'. run `ky add {}` first", name, name),
        };
    }
    if is_loaded(name) {
        return kickstart(name);
    }
    match bootstrap(&path) {
        Ok(()) => OpResult {
            name: name.to_string(),
            ok: true,
            message: format!("{}: started", name),
        },
        Err(e) => OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("{}: {}", name, e),
        },
    }
}

fn stop_one(name: &str) -> OpResult {
    if !plist_path(name).exists() {
        return OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("{}: not registered", name),
        };
    }
    match bootout(name) {
        Ok(()) => OpResult {
            name: name.to_string(),
            ok: true,
            message: format!("{}: stopped", name),
        },
        Err(e) => OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("{}: {}", name, e),
        },
    }
}

fn restart_one(name: &str) -> OpResult {
    let path = plist_path(name);
    if !path.exists() {
        return OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("no plist for '{}'. run `ky add {}` first", name, name),
        };
    }
    if is_loaded(name) {
        return kickstart(name);
    }
    match bootstrap(&path) {
        Ok(()) => OpResult {
            name: name.to_string(),
            ok: true,
            message: format!("{}: started", name),
        },
        Err(e) => OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("{}: {}", name, e),
        },
    }
}

fn kickstart(name: &str) -> OpResult {
    let label = label_for(name);
    let target = format!("gui/{}/{}", get_uid(), label);
    let out = Command::new("launchctl")
        .args(["kickstart", "-kp", &target])
        .output();
    match out {
        Ok(o) if o.status.success() => OpResult {
            name: name.to_string(),
            ok: true,
            message: format!("{}: restarted", name),
        },
        Ok(o) => OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("{}: {}", name, String::from_utf8_lossy(&o.stderr).trim()),
        },
        Err(e) => OpResult {
            name: name.to_string(),
            ok: false,
            message: format!("{}: {}", name, e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse_etime;

    #[test]
    fn etime_formats() {
        assert_eq!(parse_etime("42"), Some(42));
        assert_eq!(parse_etime("01:30"), Some(90));
        assert_eq!(parse_etime("02:15:04"), Some(2 * 3600 + 15 * 60 + 4));
        assert_eq!(
            parse_etime("3-04:05:06"),
            Some(3 * 86400 + 4 * 3600 + 5 * 60 + 6)
        );
        assert_eq!(parse_etime("garbage"), None);
    }
}
