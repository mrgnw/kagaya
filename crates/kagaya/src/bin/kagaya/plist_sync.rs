use crate::config::ServiceEntry;
use crate::logs::log_dir;
use crate::utils::listening_ports_for_pids;
use kagaya::types::{ProcessDef, ProcessState, ProcessStatus, ServiceStatus, ServiceType};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub(crate) const KAGAYA_PREFIX: &str = "com.kagaya.";

pub(crate) fn get_uid() -> u32 {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(501)
}

pub(crate) fn user_agents_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join("Library").join("LaunchAgents")
}

const LAUNCHCTL_TIMEOUT: Duration = Duration::from_secs(15);

/// Run launchctl with a hard deadline. launchd can park a call indefinitely
/// (e.g. kickstart on a throttled job); ky must never inherit that hang.
pub(crate) fn run_launchctl(args: &[&str]) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = Command::new("launchctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("launchctl: {}", e))?;

    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + LAUNCHCTL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "launchctl {} timed out after {}s",
                        args.first().copied().unwrap_or(""),
                        LAUNCHCTL_TIMEOUT.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("launchctl: {}", e)),
        }
    };

    Ok(std::process::Output {
        status,
        stdout: out_reader.join().unwrap_or_default(),
        stderr: err_reader.join().unwrap_or_default(),
    })
}

/// Parse `launchctl list` into label -> (pid, last exit code).
fn parse_launchctl_list() -> BTreeMap<String, (Option<u32>, Option<i32>)> {
    let mut map = BTreeMap::new();
    let output = match run_launchctl(&["list"]) {
        Ok(o) => o,
        Err(_) => return map,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let pid = parts[0].trim().parse::<u32>().ok();
        let exit_code = parts[1].trim().parse::<i32>().ok();
        let label = parts[2].trim().to_string();
        map.insert(label, (pid, exit_code));
    }
    map
}

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
        // Keep this short: launchd blocks kickstart/respawn for the full
        // throttle window after a kill, which is also what a deliberate
        // `ky restart` looks like. 5s still damps crash loops.
        dict.insert("ThrottleInterval".into(), plist::Value::Integer(5.into()));
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
    run_launchctl(&["print", &format!("gui/{}/{}", get_uid(), label)])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// What `launchctl print` says about a label right now: launchd's lifetime
/// spawn counter, and whether an instance is live.
///
/// `runs` is None when the job is not loaded (or the field is absent), which is
/// distinct from `Some(0)` — "loaded but never spawned".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TaskRunState {
    runs: Option<u64>,
    running: bool,
}

/// Pull `runs = N` and a live `pid = N` out of a `launchctl print` block.
fn parse_task_run_state(text: &str) -> TaskRunState {
    TaskRunState {
        runs: text.lines().find_map(|line| {
            line.trim_start()
                .strip_prefix("runs = ")?
                .trim()
                .parse()
                .ok()
        }),
        running: text.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("pid = ")
                && !trimmed.starts_with("pid = 0")
                && !trimmed.contains("(none)")
        }),
    }
}

fn task_run_state(label: &str) -> TaskRunState {
    match run_launchctl(&["print", &format!("gui/{}/{}", get_uid(), label)]) {
        Ok(o) if o.status.success() => parse_task_run_state(&String::from_utf8_lossy(&o.stdout)),
        _ => TaskRunState::default(),
    }
}

pub fn is_running_label(label: &str) -> bool {
    task_run_state(label).running
}

pub fn is_loaded(project: &str) -> bool {
    is_loaded_label(&label_for(project, None))
}

pub fn bootstrap(path: &PathBuf) -> Result<(), String> {
    let target = format!("gui/{}", get_uid());
    let out = run_launchctl(&["bootstrap", &target, &path.to_string_lossy()])?;
    if out.status.success() {
        return Ok(());
    }
    let legacy = run_launchctl(&["load", &path.to_string_lossy()])?;
    if legacy.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

fn bootout_label(label: &str, fallback_plist: Option<&PathBuf>) -> Result<(), String> {
    let target = format!("gui/{}/{}", get_uid(), label);
    let out = run_launchctl(&["bootout", &target])?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("Could not find specified service") {
        return Ok(());
    }
    if let Some(p) = fallback_plist {
        let legacy = run_launchctl(&["unload", &p.to_string_lossy()])?;
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
    pub working_dir: Option<PathBuf>,
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
        working_dir: dict
            .get("WorkingDirectory")
            .and_then(|v| v.as_string())
            .map(PathBuf::from),
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
    pub ok: bool,
    pub message: String,
}

pub type ProcessFilters = BTreeMap<String, Vec<String>>;

#[derive(Clone, Copy, Default)]
pub struct StartOpts {
    /// Block until every started process is ready before returning.
    pub wait: bool,
    /// Kill foreign processes holding configured ports before starting.
    pub force: bool,
}

pub fn start_services(
    entries: &BTreeMap<String, ServiceEntry>,
    names: &[String],
    proc_filters: &ProcessFilters,
    opts: StartOpts,
) -> Vec<OpResult> {
    crate::logs::enforce_log_caps();
    names
        .iter()
        .map(|n| {
            start_one_service(
                entries,
                n,
                proc_filters.get(n).map(Vec::as_slice).unwrap_or(&[]),
                opts,
            )
        })
        .collect()
}

pub fn stop_services(names: &[String], proc_filters: &ProcessFilters) -> Vec<OpResult> {
    names
        .iter()
        .map(|n| stop_one_service(n, proc_filters.get(n).map(Vec::as_slice).unwrap_or(&[])))
        .collect()
}

pub fn restart_services(
    entries: &BTreeMap<String, ServiceEntry>,
    names: &[String],
    proc_filters: &ProcessFilters,
    force: bool,
) -> Vec<OpResult> {
    crate::logs::enforce_log_caps();
    names
        .iter()
        .map(|n| {
            restart_one_service(
                entries,
                n,
                proc_filters.get(n).map(Vec::as_slice).unwrap_or(&[]),
                force,
            )
        })
        .collect()
}

/// One startable unit of a service: a plist, plus its config definition when
/// services.toml has one (auto-detected projects have plists but no defs).
struct Unit {
    proc_name: Option<String>,
    path: PathBuf,
    def: Option<ProcessDef>,
}

impl Unit {
    fn display(&self, project: &str) -> String {
        self.proc_name
            .clone()
            .unwrap_or_else(|| project.to_string())
    }
}

/// Re-sync plists from config, then resolve the requested units in
/// depends_on order (dependencies are pulled in transitively).
fn prepare_units(
    entries: &BTreeMap<String, ServiceEntry>,
    project: &str,
    proc_filter: &[String],
) -> Result<Vec<Unit>, String> {
    // Sync so edits to services.toml/projects.toml take effect on start —
    // plists are a compiled cache of the config, never the source of truth.
    if let Some(entry) = entries.get(project) {
        sync_service(entry).map_err(|e| format!("{}: {}", project, e))?;
    }

    let plists = plist_paths_for_project(project);
    if plists.is_empty() {
        return Err(format!(
            "no plist for '{}'. run `ky add {}` first",
            project, project
        ));
    }

    let defs = entries.get(project).map_or_else(Vec::new, |e| {
        crate::config::load_service(e, &crate::config::load_global_config().defaults).processes
    });

    // Single-plist service: one unit, its def is the only one (if any).
    if plists.len() == 1 && plists[0].0.is_none() {
        let (proc_name, path) = plists.into_iter().next().unwrap();
        return Ok(vec![Unit {
            proc_name,
            path,
            def: defs.into_iter().next(),
        }]);
    }

    let requested: Vec<&str> = if proc_filter.is_empty() {
        plists.iter().filter_map(|(p, _)| p.as_deref()).collect()
    } else {
        let known: HashSet<&str> = plists.iter().filter_map(|(p, _)| p.as_deref()).collect();
        let missing: Vec<&String> = proc_filter
            .iter()
            .filter(|p| !known.contains(p.as_str()))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "{}: process target not found: {}",
                project,
                proc_filter.join(", ")
            ));
        }
        proc_filter.iter().map(|s| s.as_str()).collect()
    };

    // depends_on order; requested processes pull their dependencies in.
    let ordered: Vec<String> = if defs.is_empty() {
        requested.iter().map(|s| s.to_string()).collect()
    } else {
        kagaya::toposort_processes(&defs, &requested).map_err(|e| format!("{}: {}", project, e))?
    };

    let mut by_name: HashMap<String, PathBuf> = plists
        .into_iter()
        .filter_map(|(p, path)| p.map(|n| (n, path)))
        .collect();
    let mut def_by_name: HashMap<String, ProcessDef> =
        defs.into_iter().map(|d| (d.name.clone(), d)).collect();

    Ok(ordered
        .into_iter()
        .filter_map(|name| {
            by_name.remove(&name).map(|path| Unit {
                def: def_by_name.remove(&name),
                proc_name: Some(name),
                path,
            })
        })
        .collect())
}

/// Per-label `runs`/pid state captured immediately before we start or restart
/// a task, so readiness can mean "finished *this* invocation" rather than
/// "no process right now". Only tasks are recorded; nothing else consults it.
type TaskBaselines = HashMap<String, TaskRunState>;

/// Snapshot a task's counter before we kick it. Non-tasks are skipped so a
/// service without dependencies pays no extra `launchctl print`.
fn record_task_baseline(unit: &Unit, label: &str, baselines: &mut TaskBaselines) {
    let is_task = unit
        .def
        .as_ref()
        .is_some_and(|d| d.service_type == ServiceType::Task);
    if is_task {
        baselines.insert(label.to_string(), task_run_state(label));
    }
}

/// Readiness barrier shared by start and restart: every `depends_on` entry of
/// `unit` must be ready before `unit` itself is touched.
///
/// Returns one message per unready dependency; empty means the caller may
/// proceed. Dependencies already confirmed ready this pass are not re-waited,
/// and a dependency with no plist of its own is not something we can wait on.
fn wait_for_deps(
    entry: &ServiceEntry,
    project: &str,
    unit: &Unit,
    units: &[Unit],
    baselines: &TaskBaselines,
    ready_done: &mut HashSet<String>,
) -> Vec<String> {
    let Some(def) = &unit.def else {
        return Vec::new();
    };
    let mut unready = Vec::new();
    for dep in &def.depends_on {
        if ready_done.contains(dep.as_str()) {
            continue;
        }
        let dep_unit = units
            .iter()
            .find(|u| u.proc_name.as_deref() == Some(dep.as_str()));
        let Some(dep_unit) = dep_unit else { continue };
        let dep_label = label_for(project, dep_unit.proc_name.as_deref());
        let baseline = baselines.get(&dep_label).copied().unwrap_or_default();
        match wait_unit_ready(entry, dep_unit, &dep_label, baseline) {
            Ok(()) => {
                ready_done.insert(dep.clone());
            }
            Err(e) => unready.push(format!(
                "{}: skipped — {} {}",
                unit.display(project),
                dep,
                e
            )),
        }
    }
    unready
}

fn start_one_service(
    entries: &BTreeMap<String, ServiceEntry>,
    project: &str,
    proc_filter: &[String],
    opts: StartOpts,
) -> OpResult {
    let units = match prepare_units(entries, project, proc_filter) {
        Ok(u) => u,
        Err(message) => return OpResult { ok: false, message },
    };
    let entry = entries.get(project);

    let mut messages = Vec::new();
    let mut any_err = false;
    let mut ready_done: HashSet<String> = HashSet::new();
    let mut baselines: TaskBaselines = HashMap::new();

    for unit in &units {
        let label = label_for(project, unit.proc_name.as_deref());

        let mut deps_ready = true;
        if let Some(entry) = entry {
            let unready = wait_for_deps(entry, project, unit, &units, &baselines, &mut ready_done);
            if !unready.is_empty() {
                deps_ready = false;
                any_err = true;
                messages.extend(unready);
            }
        }
        if let (Some(def), true) = (&unit.def, opts.force) {
            if !def.ports.is_empty() {
                messages.extend(force_free_ports(&label, &def.ports));
            }
        }
        if !deps_ready {
            continue;
        }

        record_task_baseline(unit, &label, &mut baselines);
        match start_one_label(&label, &unit.path) {
            Ok(verb) => messages.push(format!("{}: {}", unit.display(project), verb)),
            Err(e) => {
                any_err = true;
                messages.push(format!("{}: {}", unit.display(project), e));
            }
        }
    }

    if opts.wait {
        if let Some(entry) = entry {
            for unit in &units {
                let name = unit.display(project);
                if unit
                    .proc_name
                    .as_deref()
                    .is_some_and(|n| ready_done.contains(n))
                {
                    continue;
                }
                let label = label_for(project, unit.proc_name.as_deref());
                let baseline = baselines.get(&label).copied().unwrap_or_default();
                if let Err(e) = wait_unit_ready(entry, unit, &label, baseline) {
                    any_err = true;
                    messages.push(format!("{}: {}", name, e));
                } else {
                    messages.push(format!("{}: ready", name));
                }
            }
        }
    }

    OpResult {
        ok: !any_err,
        message: messages.join("\n"),
    }
}

fn stop_one_service(project: &str, proc_filter: &[String]) -> OpResult {
    let plists = plist_paths_for_project(project);
    if plists.is_empty() {
        return OpResult {
            ok: false,
            message: format!("no plist for '{}'. run `ky add {}` first", project, project),
        };
    }
    let plists = match filtered_project_plists(project, plists, proc_filter) {
        Ok(plists) => plists,
        Err(message) => return OpResult { ok: false, message },
    };
    let mut messages = Vec::new();
    let mut any_err = false;
    for (proc_opt, path) in plists {
        let label = label_for(project, proc_opt.as_deref());
        let display = proc_opt.clone().unwrap_or_else(|| project.to_string());
        match stop_one_label(&label, &path) {
            Ok(verb) => messages.push(format!("{}: {}", display, verb)),
            Err(e) => {
                any_err = true;
                messages.push(format!("{}: {}", display, e));
            }
        }
    }
    OpResult {
        ok: !any_err,
        message: messages.join("\n"),
    }
}

fn restart_one_service(
    entries: &BTreeMap<String, ServiceEntry>,
    project: &str,
    proc_filter: &[String],
    force: bool,
) -> OpResult {
    let units = match prepare_units(entries, project, proc_filter) {
        Ok(u) => u,
        Err(message) => return OpResult { ok: false, message },
    };

    let entry = entries.get(project);

    let mut messages = Vec::new();
    let mut any_err = false;
    let mut ready_done: HashSet<String> = HashSet::new();
    let mut baselines: TaskBaselines = HashMap::new();

    for unit in &units {
        let label = label_for(project, unit.proc_name.as_deref());

        // Same barrier as the start path: units are topo-ordered, so a
        // dependency was restarted on an earlier iteration and must be ready
        // before its dependent is torn down and brought back.
        if let Some(entry) = entry {
            let unready = wait_for_deps(entry, project, unit, &units, &baselines, &mut ready_done);
            if !unready.is_empty() {
                any_err = true;
                messages.extend(unready);
                continue;
            }
        }

        let expected = unit
            .def
            .as_ref()
            .map(|d| d.ports.clone())
            .unwrap_or_else(|| expected_ports_for(&unit.path, unit.proc_name.as_deref()));
        record_task_baseline(unit, &label, &mut baselines);
        match restart_one_label(&label, &unit.path, &expected, force) {
            Ok(verb) => messages.push(format!("{}: {}", unit.display(project), verb)),
            Err(e) => {
                any_err = true;
                messages.push(format!("{}: {}", unit.display(project), e));
            }
        }
    }
    OpResult {
        ok: !any_err,
        message: messages.join("\n"),
    }
}

// ── Readiness ─────────────────────────────────────────────────────────────────

const READY_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Has this task finished an invocation since `baseline` was captured?
///
/// "Not running" alone is not enough: launchd takes ~250ms to spawn a job after
/// `kickstart`, well under `READY_POLL_INTERVAL`, so a just-kicked task reads
/// "not running" simply because it has not started yet. The `runs` counter
/// distinguishes the two, with two tolerances measured on macOS 25.5:
///
/// - `runs` can jump by more than one between samples (observed 1 -> 3), so
///   only the direction of travel is meaningful, never the delta.
/// - `bootout` + `bootstrap` resets it to 0 (observed 3 -> 0), and
///   `restart_one_label` does exactly that for port-holding units. A count
///   *below* the baseline is therefore a fresh epoch we created, in which any
///   run at all is ours.
///
/// A task already running when the baseline was taken needs no counter at all:
/// it has completed once it is no longer running.
fn task_completed_since(baseline: TaskRunState, now: TaskRunState) -> bool {
    if now.running {
        return false;
    }
    if baseline.running {
        return true;
    }
    match (baseline.runs, now.runs) {
        (_, None) => false,
        (Some(base), Some(n)) if n < base => n >= 1,
        (Some(base), Some(n)) => n > base,
        (None, Some(n)) => n >= 1,
    }
}

/// Wait (bounded by ready_timeout) until a unit is ready:
/// `ready` command exit 0 > all `ports` listening > task finished > running.
fn wait_unit_ready(
    entry: &ServiceEntry,
    unit: &Unit,
    label: &str,
    task_baseline: TaskRunState,
) -> Result<(), String> {
    let timeout = unit
        .def
        .as_ref()
        .map(|d| d.ready_timeout)
        .unwrap_or(10)
        .max(1);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        if unit_ready(entry, unit, label, task_baseline) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("not ready after {}s", timeout));
        }
        std::thread::sleep(READY_POLL_INTERVAL);
    }
}

fn unit_ready(entry: &ServiceEntry, unit: &Unit, label: &str, task_baseline: TaskRunState) -> bool {
    let Some(def) = &unit.def else {
        return is_running_label(label);
    };
    if let Some(cmd) = &def.ready {
        return Command::new("/bin/sh")
            .args(["-c", cmd])
            .current_dir(&entry.dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    if !def.ports.is_empty() {
        return ports_in_use(&def.ports).len() == def.ports.len();
    }
    if def.service_type == ServiceType::Task {
        // ponytail: done when this invocation has finished; exit code not checked
        return task_completed_since(task_baseline, task_run_state(label));
    }
    is_running_label(label)
}

/// Public readiness barrier for chain sequencing (`ky start db..api`).
pub fn wait_service_ready(
    entries: &BTreeMap<String, ServiceEntry>,
    project: &str,
    proc_filter: &[String],
) -> Result<(), String> {
    let entry = entries
        .get(project)
        .ok_or_else(|| format!("unknown service: {}", project))?;
    let units = prepare_units(entries, project, proc_filter)?;
    for unit in &units {
        let label = label_for(project, unit.proc_name.as_deref());
        // No baseline: this barrier observes units someone else started, so
        // "has ever completed a run" is the strongest claim available.
        wait_unit_ready(entry, unit, &label, TaskRunState::default())
            .map_err(|e| format!("{}: {}", unit.display(project), e))?;
    }
    Ok(())
}

/// Kill foreign processes listening on `ports` (bounded SIGTERM → SIGKILL).
fn force_free_ports(our_label: &str, ports: &[u16]) -> Vec<String> {
    let ours = pid_for_label(our_label);
    let holders: Vec<PortHolder> = tcp_listener_holders(ports)
        .into_iter()
        .filter(|h| Some(h.pid) != ours)
        .collect();
    if holders.is_empty() {
        return Vec::new();
    }
    let mut messages = Vec::new();
    for h in &holders {
        let _ = Command::new("kill").arg(h.pid.to_string()).output();
        messages.push(format!(
            "killed pid {} ({}) holding port {}",
            h.pid, h.name, h.port
        ));
    }
    let held: Vec<u16> = holders.iter().map(|h| h.port).collect();
    let survivors = wait_for_ports_free(&held, Duration::from_secs(3));
    for h in holders.iter().filter(|h| survivors.contains(&h.port)) {
        let _ = Command::new("kill")
            .args(["-9", &h.pid.to_string()])
            .output();
    }
    wait_for_ports_free(&survivors, Duration::from_secs(2));
    messages
}

fn filtered_project_plists(
    project: &str,
    plists: Vec<(Option<String>, PathBuf)>,
    proc_filter: &[String],
) -> Result<Vec<(Option<String>, PathBuf)>, String> {
    if proc_filter.is_empty() {
        return Ok(plists);
    }

    let filtered: Vec<_> = plists
        .into_iter()
        .filter(|(proc_opt, _)| {
            proc_opt
                .as_ref()
                .is_some_and(|proc_name| proc_filter.contains(proc_name))
        })
        .collect();

    if filtered.is_empty() {
        Err(format!(
            "{}: process target not found: {}",
            project,
            proc_filter.join(", ")
        ))
    } else {
        Ok(filtered)
    }
}

fn start_one_label(label: &str, path: &PathBuf) -> Result<&'static str, String> {
    if is_running_label(label) {
        return Ok("already running");
    }
    if is_loaded_label(label) {
        kickstart_label(label)?;
        return Ok("started");
    }
    bootstrap(path)?;
    // `bootstrap` only *runs* the job when RunAtLoad is set; with autostart off
    // it loads it idle. Reporting "started" then would be a lie, and any
    // dependent waiting on this unit would block until its ready_timeout.
    //
    // Decided by the plist rather than by `!is_running_label`: launchd takes
    // ~250ms to spawn, so a RunAtLoad job still reads "not running" right here
    // and `kickstart -k` would kill the instance it just spawned and run a
    // task twice.
    if !read_plist_at(path).is_some_and(|i| i.run_at_load) {
        kickstart_label(label)?;
    }
    Ok("started")
}

fn stop_one_label(label: &str, path: &PathBuf) -> Result<&'static str, String> {
    bootout_label(label, Some(path))?;
    Ok("stopped")
}

/// Restart a single launchd label.
///
/// When the service binds ports, do it port-safely: fully stop the running
/// instance, wait (bounded) for it to release its ports, refuse to rebind over a
/// foreign process, then start fresh. This guarantees we never leave two
/// listeners or fail to rebind on a rapid `ky restart`. Portless services take
/// the cheap native path (`kickstart`), which avoids a macOS Login Items
/// notification that bootout+bootstrap would otherwise raise on every restart.
fn restart_one_label(
    label: &str,
    path: &PathBuf,
    expected_ports: &[u16],
    force: bool,
) -> Result<&'static str, String> {
    let old_pid = pid_for_label(label);
    let owned: Vec<u16> = old_pid.map(runtime_ports_for_pid).unwrap_or_default();

    // Ports we must guarantee are free before the new instance binds: the ports
    // the running instance holds now, plus any configured in services.toml.
    let mut guard = owned.clone();
    for &p in expected_ports {
        if !guard.contains(&p) {
            guard.push(p);
        }
    }

    if guard.is_empty() {
        return restart_native(label, path);
    }

    // Stop the running instance so we — not launchd — own the gap before rebind.
    if is_loaded_label(label) {
        bootout_label(label, Some(path))?;
    }

    // Bounded wait for the old instance to release the ports it held.
    wait_for_ports_free(&owned, PORT_RELEASE_TIMEOUT);

    // Our instance is stopped, so anything still bound on a guarded port is a
    // foreign process (or a stubborn orphan). Report it instead of bootstrapping
    // into a crash loop — unless --force, which kills the holders.
    let blockers = tcp_listener_holders(&guard);
    if !blockers.is_empty() {
        if !force {
            return Err(format_port_conflict(label, &blockers));
        }
        force_free_ports(label, &guard);
    }

    // Start fresh. bootstrap honours RunAtLoad; when autostart is off the job
    // loads but stays idle, so kickstart it to actually run the restart.
    bootstrap(path)?;
    if !is_running_label(label) {
        kickstart_label(label)?;
    }
    Ok("restarted")
}

/// Cheap restart for services with no ports to protect.
fn restart_native(label: &str, path: &PathBuf) -> Result<&'static str, String> {
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
    // No -p (print pid): it blocks until launchd actually spawns the job,
    // which stalls for the ThrottleInterval after a recent kill.
    let out = run_launchctl(&["kickstart", "-k", &target])?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

// ── Port-safe restart ──────────────────────────────────────────────────────────
//
// Bounded waits (Tiger Style: no unbounded loops). A restart must never leave two
// listeners or a service that failed to rebind; a port held by a foreign process
// must produce a clear error rather than a launchd crash loop.

const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(5);
const PORT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortHolder {
    pub port: u16,
    pub pid: u32,
    pub name: String,
}

/// The pid of the running instance behind `label`, if any.
fn pid_for_label(label: &str) -> Option<u32> {
    let out = run_launchctl(&["print", &format!("gui/{}/{}", get_uid(), label)]).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("pid = ") {
            if let Ok(pid) = rest.trim().parse::<u32>() {
                if pid != 0 {
                    return Some(pid);
                }
            }
        }
    }
    None
}

/// TCP ports held by `pid` and its descendants.
fn runtime_ports_for_pid(pid: u32) -> Vec<u16> {
    listening_ports_for_pids(&[pid])
        .into_values()
        .flatten()
        .collect()
}

/// Every distinct TCP listener currently bound to one of `ports`.
fn tcp_listener_holders(ports: &[u16]) -> Vec<PortHolder> {
    if ports.is_empty() {
        return Vec::new();
    }
    let wanted: HashSet<u16> = ports.iter().copied().collect();
    let listeners = match listeners::get_all() {
        Ok(l) => l,
        Err(_) => return Vec::new(),
    };
    let mut holders = Vec::new();
    let mut seen = HashSet::new();
    for l in &listeners {
        if l.protocol != listeners::Protocol::TCP {
            continue;
        }
        let port = l.socket.port();
        if port == 0 || !wanted.contains(&port) || !seen.insert(port) {
            continue;
        }
        holders.push(PortHolder {
            port,
            pid: l.process.pid,
            name: l.process.name.clone(),
        });
    }
    holders
}

/// The subset of `ports` that still has a TCP listener.
fn ports_in_use(ports: &[u16]) -> Vec<u16> {
    tcp_listener_holders(ports)
        .into_iter()
        .map(|h| h.port)
        .collect()
}

/// Poll until every port in `ports` is free or `timeout` elapses. Returns the
/// ports still in use when it gives up (bounded).
fn wait_for_ports_free(ports: &[u16], timeout: Duration) -> Vec<u16> {
    if ports.is_empty() {
        return Vec::new();
    }
    let deadline = Instant::now() + timeout;
    loop {
        let still = ports_in_use(ports);
        if still.is_empty() {
            return Vec::new();
        }
        if Instant::now() >= deadline {
            return still;
        }
        std::thread::sleep(PORT_POLL_INTERVAL);
    }
}

/// A clear, actionable error for a restart blocked by a held port.
fn format_port_conflict(label: &str, blockers: &[PortHolder]) -> String {
    let project = label.strip_prefix(KAGAYA_PREFIX).unwrap_or(label);
    let mut parts: Vec<String> = blockers
        .iter()
        .map(|h| format!("port {} held by pid {} ({})", h.port, h.pid, h.name))
        .collect();
    parts.sort();
    format!(
        "cannot restart {}: {}; free the port or stop that process, then retry",
        project,
        parts.join(", ")
    )
}

/// Configured `ports = [...]` for a process, read out of a services.toml body.
/// `proc_opt` is the process/section name; `None` is the single collapsed
/// process (resolvable only when there is exactly one entry).
fn parse_service_ports(toml_body: &str, proc_opt: Option<&str>) -> Vec<u16> {
    let root = match toml::from_str::<toml::Value>(toml_body) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let table = match root.as_table() {
        Some(t) => t,
        None => return Vec::new(),
    };
    let read_ports = |v: &toml::Value| -> Vec<u16> {
        v.get("ports")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_integer())
                    .filter_map(|n| u16::try_from(n).ok())
                    .collect()
            })
            .unwrap_or_default()
    };
    match proc_opt {
        Some(name) => table.get(name).map(read_ports).unwrap_or_default(),
        None => {
            // The sole real service resolves even alongside the reserved
            // `log_max_mb` knob; several real services stay ambiguous.
            let mut services = table.iter().filter(|(k, _)| k.as_str() != "log_max_mb");
            match (services.next(), services.next()) {
                (Some((_, v)), None) => read_ports(v),
                _ => Vec::new(),
            }
        }
    }
}

/// Configured ports for the process behind `path`'s plist, via its
/// WorkingDirectory's services.toml. Empty when none are declared.
fn expected_ports_for(path: &PathBuf, proc_opt: Option<&str>) -> Vec<u16> {
    let dir = match read_plist_at(path).and_then(|i| i.working_dir) {
        Some(d) => d,
        None => return Vec::new(),
    };
    let body = match std::fs::read_to_string(dir.join("services.toml")) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    parse_service_ports(&body, proc_opt)
}

#[cfg(test)]
mod tests {
    use super::{
        filtered_project_plists, format_port_conflict, parse_etime, parse_service_ports,
        parse_task_run_state, task_completed_since, tcp_listener_holders, wait_for_ports_free,
        PortHolder, TaskRunState,
    };
    use std::path::PathBuf;

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

    fn plist_set() -> Vec<(Option<String>, PathBuf)> {
        vec![
            (None, PathBuf::from("/tmp/com.kagaya.jobs.plist")),
            (
                Some("sync".to_string()),
                PathBuf::from("/tmp/com.kagaya.jobs.sync.plist"),
            ),
            (
                Some("ui".to_string()),
                PathBuf::from("/tmp/com.kagaya.jobs.ui.plist"),
            ),
        ]
    }

    #[test]
    fn empty_process_filter_keeps_whole_service() {
        let filtered = filtered_project_plists("jobs", plist_set(), &[]).unwrap();

        let names: Vec<Option<String>> = filtered.into_iter().map(|(proc, _)| proc).collect();
        assert_eq!(names, vec![None, Some("sync".into()), Some("ui".into())]);
    }

    #[test]
    fn process_filter_selects_only_named_process() {
        let filtered = filtered_project_plists("jobs", plist_set(), &["ui".to_string()]).unwrap();

        let names: Vec<Option<String>> = filtered.into_iter().map(|(proc, _)| proc).collect();
        assert_eq!(names, vec![Some("ui".into())]);
    }

    #[test]
    fn process_filter_selects_multiple_named_processes_without_single_plist() {
        let filtered =
            filtered_project_plists("jobs", plist_set(), &["sync".to_string(), "ui".to_string()])
                .unwrap();

        let names: Vec<Option<String>> = filtered.into_iter().map(|(proc, _)| proc).collect();
        assert_eq!(names, vec![Some("sync".into()), Some("ui".into())]);
    }

    #[test]
    fn process_filter_errors_when_target_is_missing() {
        let err =
            filtered_project_plists("jobs", plist_set(), &["worker".to_string()]).unwrap_err();

        assert_eq!(err, "jobs: process target not found: worker");
    }

    // ── task readiness ─────────────────────────────────────────────────────
    //
    // Fixtures are trimmed from real `launchctl print gui/501/<label>` output
    // on macOS 25.5; the nested `state = active` lines are why the parser
    // anchors on the field name rather than searching the whole block.

    const PRINT_NEVER_RUN: &str = "\
com.kagaya.demo.build = {
\tactive count = 0
\tstate = not running
\truns = 0
\tlast exit code = (never exited)
}";

    const PRINT_RUNNING: &str = "\
com.kagaya.demo.build = {
\tactive count = 1
\tstate = running
\truns = 1
\tpid = 12567
\tendpoints = {
\t\t\"com.kagaya.demo\" = {
\t\t\tstate = active
\t\t}
\t}
}";

    const PRINT_FINISHED: &str = "\
com.kagaya.demo.build = {
\tstate = not running
\truns = 1
\tlast exit code = 0
}";

    fn stopped(runs: u64) -> TaskRunState {
        parse_task_run_state(&format!("\tstate = not running\n\truns = {}\n", runs))
    }

    fn running(runs: u64) -> TaskRunState {
        parse_task_run_state(&format!("\truns = {}\n\tpid = 999\n", runs))
    }

    #[test]
    fn never_run_task_reports_runs_zero_not_absent() {
        // Q1 from the plan: launchd emits `runs = 0` for a bootstrapped job
        // that has never spawned. Absent `runs` must stay distinguishable.
        let state = parse_task_run_state(PRINT_NEVER_RUN);
        assert_eq!(state.runs, Some(0));
        assert!(!state.running);
    }

    #[test]
    fn running_task_reports_pid_and_counter() {
        let state = parse_task_run_state(PRINT_RUNNING);
        assert_eq!(state.runs, Some(1));
        assert!(state.running);
    }

    #[test]
    fn finished_task_reports_counter_without_pid() {
        let state = parse_task_run_state(PRINT_FINISHED);
        assert_eq!(state.runs, Some(1));
        assert!(!state.running);
    }

    #[test]
    fn unloaded_job_has_no_counter() {
        assert_eq!(parse_task_run_state(""), TaskRunState::default());
        assert_eq!(parse_task_run_state("").runs, None);
    }

    #[test]
    fn idle_pid_forms_are_not_running() {
        assert!(!parse_task_run_state("\tpid = 0\n").running);
        assert!(!parse_task_run_state("\tpid = (none)\n").running);
    }

    #[test]
    fn just_kickstarted_task_is_not_ready_before_launchd_spawns_it() {
        // The original defect: launchd takes ~250ms to spawn, so the task is
        // "not running" purely because it has not started. It must not read
        // ready just because no pid exists yet.
        assert!(!task_completed_since(stopped(1), stopped(1)));
    }

    #[test]
    fn task_is_not_ready_while_running() {
        assert!(!task_completed_since(stopped(1), running(2)));
    }

    #[test]
    fn task_is_ready_once_counter_moved_and_process_exited() {
        assert!(task_completed_since(stopped(1), stopped(2)));
    }

    #[test]
    fn counter_jumping_more_than_one_still_counts_as_ready() {
        // Observed on macOS 25.5: consecutive kickstarts took runs 1 -> 3.
        // Only the direction of travel is meaningful.
        assert!(task_completed_since(stopped(1), stopped(3)));
    }

    #[test]
    fn bootstrapped_but_never_run_task_is_not_ready() {
        assert!(!task_completed_since(stopped(0), stopped(0)));
        assert!(!task_completed_since(TaskRunState::default(), stopped(0)));
    }

    #[test]
    fn counter_reset_by_bootout_bootstrap_is_a_fresh_epoch() {
        // `restart_one_label` boots out and bootstraps port-holding units,
        // which resets runs to 0 (observed 3 -> 0). Mid-reset the task has not
        // run yet; once it has, that run is ours even though 1 < 3.
        assert!(!task_completed_since(stopped(3), stopped(0)));
        assert!(task_completed_since(stopped(3), stopped(1)));
    }

    #[test]
    fn task_already_running_at_baseline_is_ready_once_it_exits() {
        // `start_one_label` reports "already running" without kickstarting, so
        // the counter never moves; completion is the pid going away.
        assert!(!task_completed_since(running(1), running(1)));
        assert!(task_completed_since(running(1), stopped(1)));
    }

    #[test]
    fn unreadable_current_state_is_never_ready() {
        assert!(!task_completed_since(stopped(1), TaskRunState::default()));
    }

    #[test]
    fn without_a_baseline_any_completed_run_counts() {
        // `wait_service_ready` (ky start a..b) has no baseline to capture.
        assert!(task_completed_since(TaskRunState::default(), stopped(2)));
    }

    // ── port-safe restart ──────────────────────────────────────────────────

    #[test]
    fn service_ports_read_from_full_form() {
        let body = "[api]\nrun = \"python s.py\"\nports = [8080, 9090]\n";
        assert_eq!(parse_service_ports(body, Some("api")), vec![8080, 9090]);
    }

    #[test]
    fn service_ports_simple_form_has_none() {
        let body = "web = \"npm run dev\"\n";
        assert_eq!(parse_service_ports(body, Some("web")), Vec::<u16>::new());
    }

    #[test]
    fn service_ports_single_collapsed_process_resolves_sole_entry() {
        // A one-process service collapses to proc=None but ports still resolve.
        let body = "[api]\nrun = \"python s.py\"\nports = [8080]\n";
        assert_eq!(parse_service_ports(body, None), vec![8080]);
    }

    #[test]
    fn service_ports_multi_process_selects_named_and_rejects_none() {
        let body = "[api]\nrun = \"a\"\nports = [8080]\n\n[web]\nrun = \"b\"\nports = [3000]\n";
        assert_eq!(parse_service_ports(body, Some("web")), vec![3000]);
        // proc=None is ambiguous when there are several processes.
        assert_eq!(parse_service_ports(body, None), Vec::<u16>::new());
    }

    #[test]
    fn service_ports_missing_process_is_empty() {
        let body = "[api]\nrun = \"a\"\nports = [8080]\n";
        assert_eq!(parse_service_ports(body, Some("worker")), Vec::<u16>::new());
    }

    #[test]
    fn service_ports_ignores_log_max_mb_when_collapsing() {
        // A single-service file plus the reserved knob still resolves proc=None.
        let body = "log_max_mb = 50\n[api]\nrun = \"python s.py\"\nports = [8080]\n";
        assert_eq!(parse_service_ports(body, None), vec![8080]);
        // Two real services remain ambiguous even with the knob present.
        let body = "log_max_mb = 50\n[api]\nrun = \"a\"\nports = [8080]\n[web]\nrun = \"b\"\nports = [3000]\n";
        assert_eq!(parse_service_ports(body, None), Vec::<u16>::new());
    }

    #[test]
    fn port_conflict_message_names_project_port_and_holder() {
        let blockers = vec![PortHolder {
            port: 8080,
            pid: 4321,
            name: "node".to_string(),
        }];
        let msg = format_port_conflict("com.kagaya.drover", &blockers);
        assert!(msg.contains("drover"), "{}", msg);
        assert!(msg.contains("8080"), "{}", msg);
        assert!(msg.contains("4321"), "{}", msg);
        assert!(msg.contains("node"), "{}", msg);
    }

    #[test]
    fn free_port_reports_free_without_burning_the_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let start = std::time::Instant::now();
        let stuck = wait_for_ports_free(&[port], std::time::Duration::from_secs(5));
        assert!(
            stuck.is_empty(),
            "expected port {} free, got {:?}",
            port,
            stuck
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn held_port_is_detected_and_reported_stuck() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let holders = tcp_listener_holders(&[port]);
        assert_eq!(
            holders.iter().map(|h| h.port).collect::<Vec<_>>(),
            vec![port]
        );
        assert_eq!(holders[0].pid, std::process::id());

        let stuck = wait_for_ports_free(&[port], std::time::Duration::from_millis(300));
        assert_eq!(stuck, vec![port]);
        drop(listener);
    }
}
