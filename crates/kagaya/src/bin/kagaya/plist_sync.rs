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

fn has_shell_metachars(cmd: &str) -> bool {
    cmd.contains("&&")
        || cmd.contains("||")
        || cmd.contains(';')
        || cmd.contains('|')
        || cmd.contains('>')
        || cmd.contains('<')
        || cmd.contains("$(")
        || cmd.contains('`')
        || cmd.contains('*')
        || cmd.contains('~')
}

fn which_in_path(bin: &str) -> Option<PathBuf> {
    if bin.contains('/') {
        let p = PathBuf::from(bin);
        return if p.is_file() { Some(p) } else { None };
    }
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        let candidate = PathBuf::from(dir).join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Build the ProgramArguments array for a run string. If the command has shell
/// metacharacters, wrap in `/bin/sh -c` (launchd's PATH is minimal, but the
/// shell-wrapped form is explicit user intent). Otherwise tokenize the command
/// and resolve the first token against the caller's PATH at plist-write time
/// so the absolute binary path is baked in.
fn build_program_args(cmd: &str) -> Vec<String> {
    if has_shell_metachars(cmd) {
        return vec!["/bin/sh".into(), "-c".into(), cmd.to_string()];
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.is_empty() {
        return vec!["/bin/sh".into(), "-c".into(), cmd.to_string()];
    }
    let resolved = which_in_path(tokens[0])
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| tokens[0].to_string());
    let mut args = vec![resolved];
    for t in &tokens[1..] {
        args.push(t.to_string());
    }
    args
}

fn resolved_command(svc: &ServiceEntry) -> Option<Vec<String>> {
    if let Some(inline) = &svc.inline_command {
        return Some(build_program_args(&inline.run));
    }
    let services_toml = svc.dir.join("services.toml");
    if services_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&services_toml) {
            if let Ok(root) = toml::from_str::<toml::Value>(&content) {
                if let Some(table) = root.as_table() {
                    for (_, value) in table {
                        if let Some(run) = value.as_str() {
                            return Some(build_program_args(run));
                        }
                        if let Some(run) = value.get("run").and_then(|v| v.as_str()) {
                            return Some(build_program_args(run));
                        }
                    }
                }
            }
        }
    }
    let suggestions = crate::detect::detect_services(&svc.dir);
    let first = suggestions.into_iter().next()?;
    Some(build_program_args(&first.command))
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

    let mut env = service_env(svc);
    if !env.contains_key("PATH") {
        if let Ok(current_path) = std::env::var("PATH") {
            env.insert("PATH".into(), current_path);
        }
    }
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

pub fn set_run_at_load(name: &str, value: bool) -> Result<(), String> {
    let path = plist_path(name);
    if !path.exists() {
        return Err(format!("no plist for '{}'", name));
    }
    let mut value_plist =
        plist::Value::from_file(&path).map_err(|e| format!("reading plist: {}", e))?;
    let dict = value_plist
        .as_dictionary_mut()
        .ok_or_else(|| "plist root is not a dictionary".to_string())?;
    dict.insert("RunAtLoad".into(), plist::Value::Boolean(value));
    value_plist
        .to_file_xml(&path)
        .map_err(|e| format!("writing plist: {}", e))?;
    // Always bootout so launchd drops its in-memory state (which ignores
    // the new RunAtLoad and keeps the process alive via KeepAlive).
    let was_loaded = is_loaded(name);
    if was_loaded {
        let _ = bootout(name);
    }
    // Only re-bootstrap if we're turning autostart ON. Turning OFF = stop.
    if value {
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
