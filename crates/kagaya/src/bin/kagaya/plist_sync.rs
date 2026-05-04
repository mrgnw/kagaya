use crate::config::ServiceEntry;
use crate::launchd::{get_uid, parse_launchctl_list, user_agents_dir, KAGAYA_PREFIX};
use crate::logs::log_dir;
use crate::utils::listening_ports_for_pids;
use kagaya::types::{ProcessState, ProcessStatus, ServiceStatus, ServiceType};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Command;

// ── Labels + paths ────────────────────────────────────────────────────────────
//
// A project with one process uses:    com.kagaya.<project>
// A project with N processes uses:    com.kagaya.<project>.<proc>   (one per proc)
//
// proc=None  means "this is the single plist for the project".
// proc=Some  means "this is the <proc> plist inside the project's group".

pub fn label_for(project: &str, proc: Option<&str>) -> String {
    match proc {
        Some(p) => format!("{}{}.{}", KAGAYA_PREFIX, project, p),
        None => format!("{}{}", KAGAYA_PREFIX, project),
    }
}

pub fn plist_path_for(project: &str, proc: Option<&str>) -> PathBuf {
    user_agents_dir().join(format!("{}.plist", label_for(project, proc)))
}

pub fn plist_exists(project: &str) -> bool {
    !plist_paths_for_project(project).is_empty()
}

/// Return every plist file under this project: the single `com.kagaya.<project>.plist`
/// plus any per-process `com.kagaya.<project>.<proc>.plist`. Processes are
/// returned with `proc_name = Some(...)`, single-proc plist with None.
pub fn plist_paths_for_project(project: &str) -> Vec<(Option<String>, PathBuf)> {
    let mut out = Vec::new();
    let dir = user_agents_dir();
    let single = plist_path_for(project, None);
    if single.exists() {
        out.push((None, single));
    }
    let prefix = format!("{}{}.", KAGAYA_PREFIX, project);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(label) = name.strip_suffix(".plist") else {
                continue;
            };
            let Some(proc) = label.strip_prefix(&prefix) else {
                continue;
            };
            if proc.is_empty() {
                continue;
            }
            out.push((Some(proc.to_string()), entry.path()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// ── Command resolution ────────────────────────────────────────────────────────

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

pub struct ProcessSpec {
    pub proc_name: Option<String>,
    pub command: String,
    pub env: HashMap<String, String>,
    pub service_type: ServiceType,
    pub restart: bool,
}

/// Decide how many processes this project has and what each one runs.
/// Precedence:
///   1. Inline `run = ...` from projects.toml       → one process, proc=None
///   2. services.toml with N entries                → N processes (or one if N==1)
///   3. Auto-detection (Procfile / package.json ..) → one process per suggestion
fn resolve_processes(svc: &ServiceEntry) -> Vec<ProcessSpec> {
    if let Some(inline) = &svc.inline_command {
        let is_task = inline.service_type == ServiceType::Task;
        return vec![ProcessSpec {
            proc_name: None,
            command: inline.run.clone(),
            env: inline.env.clone(),
            service_type: inline.service_type.clone(),
            restart: inline.restart.unwrap_or(!is_task),
        }];
    }

    let services_toml = svc.dir.join("services.toml");
    if services_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&services_toml) {
            if let Ok(root) = toml::from_str::<toml::Value>(&content) {
                if let Some(table) = root.as_table() {
                    let mut procs = Vec::new();
                    for (pname, value) in table {
                        let cmd = if let Some(s) = value.as_str() {
                            Some(s.to_string())
                        } else if let Some(s) = value.get("run").and_then(|v| v.as_str()) {
                            Some(s.to_string())
                        } else {
                            None
                        };
                        let env = value
                            .get("env")
                            .and_then(|v| v.as_table())
                            .map(|t| {
                                t.iter()
                                    .filter_map(|(k, v)| {
                                        v.as_str().map(|s| (k.clone(), s.to_string()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let service_type = match value.get("type").and_then(|v| v.as_str()) {
                            Some("task") => ServiceType::Task,
                            _ => ServiceType::Service,
                        };
                        let is_task = service_type == ServiceType::Task;
                        let restart = value
                            .get("restart")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(!is_task);
                        if let Some(cmd) = cmd {
                            procs.push(ProcessSpec {
                                proc_name: Some(pname.clone()),
                                command: cmd,
                                env,
                                service_type,
                                restart,
                            });
                        }
                    }
                    if procs.len() == 1 {
                        procs[0].proc_name = None;
                    }
                    if !procs.is_empty() {
                        return procs;
                    }
                }
            }
        }
    }

    let suggestions = crate::detect::detect_services(&svc.dir);
    if suggestions.is_empty() {
        return Vec::new();
    }
    if suggestions.len() == 1 {
        return vec![ProcessSpec {
            proc_name: None,
            command: suggestions.into_iter().next().unwrap().command,
            env: HashMap::new(),
            service_type: ServiceType::Service,
            restart: true,
        }];
    }
    suggestions
        .into_iter()
        .map(|s| ProcessSpec {
            proc_name: Some(s.name),
            command: s.command,
            env: HashMap::new(),
            service_type: ServiceType::Service,
            restart: true,
        })
        .collect()
}

// ── Plist writer ──────────────────────────────────────────────────────────────

fn log_path_for(project: &str, proc: Option<&str>, stderr: bool) -> PathBuf {
    let log_root = log_dir();
    let _ = std::fs::create_dir_all(&log_root);
    let stem = match proc {
        Some(p) => format!("{}.{}", project, p),
        None => project.to_string(),
    };
    log_root.join(if stderr {
        format!("{}.err.log", stem)
    } else {
        format!("{}.log", stem)
    })
}

fn build_plist_value(svc: &ServiceEntry, spec: &ProcessSpec) -> plist::Value {
    let label = label_for(&svc.name, spec.proc_name.as_deref());
    let stdout_log = log_path_for(&svc.name, spec.proc_name.as_deref(), false);
    let stderr_log = log_path_for(&svc.name, spec.proc_name.as_deref(), true);
    let program_args = build_program_args(&spec.command);

    let mut dict = plist::Dictionary::new();
    dict.insert("Label".into(), plist::Value::String(label));
    dict.insert(
        "ProgramArguments".into(),
        plist::Value::Array(program_args.into_iter().map(plist::Value::String).collect()),
    );
    dict.insert(
        "WorkingDirectory".into(),
        plist::Value::String(svc.dir.to_string_lossy().to_string()),
    );
    let is_task = spec.service_type == ServiceType::Task;
    if is_task || !spec.restart {
        dict.insert("KeepAlive".into(), plist::Value::Boolean(false));
    } else {
        let mut ka = plist::Dictionary::new();
        ka.insert("SuccessfulExit".into(), plist::Value::Boolean(false));
        dict.insert("KeepAlive".into(), plist::Value::Dictionary(ka));
        dict.insert("ThrottleInterval".into(), plist::Value::Integer(30.into()));
    }
    dict.insert("RunAtLoad".into(), plist::Value::Boolean(svc.autostart));
    dict.insert(
        "StandardOutPath".into(),
        plist::Value::String(stdout_log.to_string_lossy().to_string()),
    );
    dict.insert(
        "StandardErrorPath".into(),
        plist::Value::String(stderr_log.to_string_lossy().to_string()),
    );

    let mut env = spec.env.clone();
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
    plist::Value::Dictionary(dict)
}

// ── launchctl primitives ──────────────────────────────────────────────────────

pub fn is_loaded_label(label: &str) -> bool {
    Command::new("launchctl")
        .args(["print", &format!("gui/{}/{}", get_uid(), label)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_running_label(label: &str) -> bool {
    let out = Command::new("launchctl")
        .args(["print", &format!("gui/{}/{}", get_uid(), label)])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            text.lines().any(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("pid = ")
                    && !trimmed.starts_with("pid = 0")
                    && !trimmed.contains("(none)")
            })
        }
        _ => false,
    }
}

pub fn is_loaded(project: &str) -> bool {
    is_loaded_label(&label_for(project, None))
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

fn bootout_label(label: &str, fallback_plist: Option<&PathBuf>) -> Result<(), String> {
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
    if let Some(p) = fallback_plist {
        let legacy = Command::new("launchctl")
            .args(["unload", &p.to_string_lossy()])
            .output()
            .map_err(|e| e.to_string())?;
        if legacy.status.success() {
            return Ok(());
        }
    }
    Err(stderr.trim().to_string())
}

pub fn bootout(project: &str) -> Result<(), String> {
    let label = label_for(project, None);
    let path = plist_path_for(project, None);
    bootout_label(&label, Some(&path))
}

// Order-independent comparison: `plist::Dictionary` is backed by IndexMap, so
// the derived `PartialEq` is order-sensitive. We only care about semantic
// equivalence — matching keys/values regardless of insertion order.
fn plist_value_eq(a: &plist::Value, b: &plist::Value) -> bool {
    use plist::Value;
    match (a, b) {
        (Value::Dictionary(x), Value::Dictionary(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|v2| plist_value_eq(v, v2)))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(ai, bi)| plist_value_eq(ai, bi))
        }
        _ => a == b,
    }
}

// ── Sync ──────────────────────────────────────────────────────────────────────

pub fn sync_service(svc: &ServiceEntry) -> Result<usize, String> {
    let procs = resolve_processes(svc);
    if procs.is_empty() {
        return Err(format!(
			"no runnable command for '{}' (need inline `run = ...`, services.toml, or auto-detectable project)",
			svc.name
		));
    }

    let _ = std::fs::create_dir_all(user_agents_dir());

    // Figure out which labels we'll write this pass.
    let wanted_labels: std::collections::HashSet<String> = procs
        .iter()
        .map(|p| label_for(&svc.name, p.proc_name.as_deref()))
        .collect();

    let mut written = 0usize;

    // Any plist currently on disk under this project that we're NOT about to
    // write is stale — bootout + delete.
    for (proc_opt, path) in plist_paths_for_project(&svc.name) {
        let lbl = label_for(&svc.name, proc_opt.as_deref());
        if !wanted_labels.contains(&lbl) {
            let _ = bootout_label(&lbl, Some(&path));
            let _ = std::fs::remove_file(&path);
            written += 1;
        }
    }

    for spec in &procs {
        let label = label_for(&svc.name, spec.proc_name.as_deref());
        let path = plist_path_for(&svc.name, spec.proc_name.as_deref());
        let new_value = build_plist_value(svc, spec);

        // Skip bootout/write/bootstrap if the on-disk plist is already identical —
        // every bootstrap fires a "Login Items" notification from macOS.
        let unchanged = plist::Value::from_file(&path)
            .map(|existing| plist_value_eq(&existing, &new_value))
            .unwrap_or(false);
        if unchanged {
            continue;
        }

        let was_loaded = is_loaded_label(&label);
        let was_running = was_loaded && is_running_label(&label);
        if was_loaded {
            let _ = bootout_label(&label, Some(&path));
        }
        new_value
            .to_file_xml(&path)
            .map_err(|e| format!("writing {}: {}", path.display(), e))?;
        if was_loaded || svc.autostart {
            bootstrap(&path)?;
        }
        if was_running && !svc.autostart {
            let _ = kickstart_label(&label);
        }
        written += 1;
    }
    Ok(written)
}

pub fn set_run_at_load(project: &str, value: bool) -> Result<(), String> {
    let plists = plist_paths_for_project(project);
    if plists.is_empty() {
        return Err(format!("no plist for '{}'", project));
    }
    for (proc_opt, path) in plists {
        let label = label_for(project, proc_opt.as_deref());
        let mut v = plist::Value::from_file(&path)
            .map_err(|e| format!("reading {}: {}", path.display(), e))?;
        let dict = v
            .as_dictionary_mut()
            .ok_or_else(|| format!("{}: plist root is not a dictionary", label))?;
        dict.insert("RunAtLoad".into(), plist::Value::Boolean(value));
        let was_loaded = is_loaded_label(&label);
        let was_running = was_loaded && is_running_label(&label);
        v.to_file_xml(&path)
            .map_err(|e| format!("writing {}: {}", path.display(), e))?;
        if was_loaded {
            let _ = bootout_label(&label, Some(&path));
        }
        if value || was_running {
            bootstrap(&path)?;
        }
        if was_running && !value {
            let _ = kickstart_label(&label);
        }
    }
    Ok(())
}

pub fn remove_service(project: &str) -> Result<(), String> {
    for (proc_opt, path) in plist_paths_for_project(project) {
        let label = label_for(project, proc_opt.as_deref());
        let _ = bootout_label(&label, Some(&path));
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("removing {}: {}", path.display(), e))?;
        }
    }
    Ok(())
}

// ── Readers ───────────────────────────────────────────────────────────────────

pub struct PlistInfo {
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
    pub run_at_load: bool,
}

pub fn read_plist_at(path: &PathBuf) -> Option<PlistInfo> {
    let value = plist::Value::from_file(path).ok()?;
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
    })
}

pub fn read_plist(project: &str) -> Option<PlistInfo> {
    let plists = plist_paths_for_project(project);
    let (_, path) = plists.into_iter().next()?;
    read_plist_at(&path)
}

/// Return `(stdout_path, stderr_path)` pairs for every plist under this project.
pub fn log_paths_all(project: &str) -> Vec<(Option<String>, PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for (proc_opt, path) in plist_paths_for_project(project) {
        if let Some(info) = read_plist_at(&path) {
            if let (Some(o), Some(e)) = (info.stdout_path, info.stderr_path) {
                out.push((proc_opt, o, e));
            }
        }
    }
    out
}

/// Back-compat: first (stdout, stderr) pair for this project.
pub fn log_paths(project: &str) -> Option<(PathBuf, PathBuf)> {
    log_paths_all(project)
        .into_iter()
        .next()
        .map(|(_, o, e)| (o, e))
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

pub fn status_for(svc: &ServiceEntry) -> ServiceStatus {
    let launchctl = parse_launchctl_list();
    let plists = plist_paths_for_project(&svc.name);

    let mut processes = Vec::new();
    if plists.is_empty() {
        processes.push(ProcessStatus {
            name: svc.name.clone(),
            state: ProcessState::Stopped,
            pid: None,
            autostart: svc.autostart,
            service_type: ServiceType::Service,
            ports: vec![],
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        });
    } else {
        for (proc_opt, path) in &plists {
            let label = label_for(&svc.name, proc_opt.as_deref());
            let info = read_plist_at(path);
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
            let proc_display = proc_opt.clone().unwrap_or_else(|| svc.name.clone());
            processes.push(ProcessStatus {
                name: proc_display,
                state,
                pid,
                autostart: run_at_load,
                service_type: ServiceType::Service,
                ports,
                ports_expected: vec![],
                state_since: None,
                cpu_percent: None,
                memory_bytes: None,
            });
        }
    }

    ServiceStatus {
        name: svc.name.clone(),
        dir: svc.dir.clone(),
        processes,
    }
}

pub fn query_all(services: &BTreeMap<String, ServiceEntry>) -> Vec<ServiceStatus> {
    services.values().map(status_for).collect()
}

// ── Orchestration ─────────────────────────────────────────────────────────────

pub struct OpResult {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

pub fn start_services(names: &[String], proc_filter: &[String]) -> Vec<OpResult> {
    names.iter().map(|n| fan_op(n, Op::Start, proc_filter)).collect()
}

pub fn stop_services(names: &[String]) -> Vec<OpResult> {
    names.iter().map(|n| fan_op(n, Op::Stop, &[])).collect()
}

pub fn restart_services(names: &[String]) -> Vec<OpResult> {
    names.iter().map(|n| fan_op(n, Op::Restart, &[])).collect()
}

#[derive(Clone, Copy)]
enum Op {
    Start,
    Stop,
    Restart,
}

fn fan_op(project: &str, op: Op, proc_filter: &[String]) -> OpResult {
    let plists = plist_paths_for_project(project);
    if plists.is_empty() {
        return OpResult {
            name: project.to_string(),
            ok: false,
            message: format!("no plist for '{}'. run `ky add {}` first", project, project),
        };
    }
    let mut messages = Vec::new();
    let mut any_err = false;
    for (proc_opt, path) in plists {
        if !proc_filter.is_empty() {
            match &proc_opt {
                Some(proc_name) if !proc_filter.contains(proc_name) => continue,
                None => continue,
                _ => {}
            }
        }
        let label = label_for(project, proc_opt.as_deref());
        let display = proc_opt.clone().unwrap_or_else(|| project.to_string());
        let result = match op {
            Op::Start => start_one_label(&label, &path),
            Op::Stop => stop_one_label(&label, &path),
            Op::Restart => restart_one_label(&label, &path),
        };
        match result {
            Ok(verb) => messages.push(format!("{}: {}", display, verb)),
            Err(e) => {
                any_err = true;
                messages.push(format!("{}: {}", display, e));
            }
        }
    }
    OpResult {
        name: project.to_string(),
        ok: !any_err,
        message: messages.join("\n"),
    }
}

fn start_one_label(label: &str, path: &PathBuf) -> Result<&'static str, String> {
    if is_loaded_label(label) {
        kickstart_label(label)?;
        return Ok("restarted");
    }
    bootstrap(path)?;
    Ok("started")
}

fn stop_one_label(label: &str, path: &PathBuf) -> Result<&'static str, String> {
    bootout_label(label, Some(path))?;
    Ok("stopped")
}

fn restart_one_label(label: &str, path: &PathBuf) -> Result<&'static str, String> {
    if is_loaded_label(label) {
        kickstart_label(label)?;
        Ok("restarted")
    } else {
        bootstrap(path)?;
        Ok("started")
    }
}

fn kickstart_label(label: &str) -> Result<(), String> {
    let target = format!("gui/{}/{}", get_uid(), label);
    let out = Command::new("launchctl")
        .args(["kickstart", "-kp", &target])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
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
