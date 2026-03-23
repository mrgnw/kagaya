use owo_colors::OwoColorize;
use std::path::PathBuf;

const LABEL: &str = "com.kagaya.autostart";

// ── Public query API (used by daemon HTTP endpoints) ─────────────────────────

pub fn status_info() -> AutostartInfo {
    let installed = is_installed();
    let active = if installed { is_active() } else { false };
    let path = agent_path().map(|p| p.to_string_lossy().to_string());
    let projects = crate::config::autostart_project_names();
    AutostartInfo {
        installed,
        active,
        agent_path: path,
        projects,
    }
}

pub struct AutostartInfo {
    pub installed: bool,
    pub active: bool,
    pub agent_path: Option<String>,
    pub projects: Vec<String>,
}

pub fn enable() -> Result<String, String> {
    if is_installed() {
        if !is_active() {
            activate_result()?;
            return Ok("autostart activated".into());
        }
        return Ok("autostart already enabled".into());
    }
    install_result()
}

pub fn disable() -> Result<String, String> {
    if !is_installed() {
        return Ok("autostart not installed".into());
    }
    uninstall_result()
}

// ── CLI entry point ──────────────────────────────────────────────────────────

pub fn cmd_autostart(args: &[String]) {
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");

    match subcmd {
        "on" => cmd_on(),
        "off" => cmd_off(),
        "status" | "st" => cmd_status(),
        "help" | "--help" | "-h" => print_usage(),
        _ => {
            eprintln!("unknown subcommand: {}", subcmd);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("ky autostart — start services on login");
    eprintln!();
    eprintln!("usage: ky autostart [command]");
    eprintln!();
    eprintln!("commands:");
    eprintln!(
        "  {}              Install boot agent (LaunchAgent / systemd)",
        "on".bold()
    );
    eprintln!("  {}             Remove boot agent", "off".bold());
    eprintln!("  {}          Show status (default)", "status".bold());
    eprintln!();
    eprintln!("configure which projects start on boot in projects.toml:");
    eprintln!("  [myapp]");
    eprintln!("  dir = \"~/dev/myapp\"");
    eprintln!("  autostart = true");
}

fn cmd_status() {
    let info = status_info();

    if info.installed {
        if info.active {
            eprintln!("{} autostart is {}", "●".green(), "on".green());
        } else {
            eprintln!(
                "{} autostart is {} (installed but not active)",
                "⚠".yellow(),
                "inactive".yellow()
            );
        }
        if let Some(ref path) = info.agent_path {
            eprintln!("  agent: {}", path.dimmed());
        }
    } else {
        eprintln!("{} autostart is {}", "◻".dimmed(), "off".dimmed());
        eprintln!("  run {} to enable", "ky autostart on".bold());
    }

    eprintln!();
    if info.projects.is_empty() {
        eprintln!("  no projects with autostart = true");
        eprintln!("  add to projects.toml:");
        eprintln!("    [myapp]");
        eprintln!("    dir = \"~/dev/myapp\"");
        eprintln!("    autostart = true");
    } else {
        eprintln!("  projects with autostart:");
        for name in &info.projects {
            eprintln!("    {}", name);
        }
    }
}

fn cmd_on() {
    match enable() {
        Ok(msg) => eprintln!("{}", msg),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }

    let names = crate::config::autostart_project_names();
    if names.is_empty() {
        eprintln!();
        eprintln!("warning: no projects have autostart = true in projects.toml");
        eprintln!("the boot agent will start kagaya but no services will auto-start");
    }
}

fn cmd_off() {
    match disable() {
        Ok(msg) => eprintln!("{}", msg),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}

// ── Platform: macOS ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn ky_binary_path() -> String {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("ky"))
        .to_string_lossy()
        .to_string()
}

#[cfg(target_os = "macos")]
fn agent_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", LABEL)),
    )
}

#[cfg(target_os = "macos")]
fn log_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("kagaya")
        .join("launchd")
}

#[cfg(target_os = "macos")]
fn is_installed() -> bool {
    agent_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn is_active() -> bool {
    let output = std::process::Command::new("launchctl").arg("list").output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.lines().any(|line| line.contains(LABEL))
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn get_uid() -> u32 {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(501)
}

#[cfg(target_os = "macos")]
fn install_result() -> Result<String, String> {
    let plist_path =
        agent_path().ok_or_else(|| "could not determine LaunchAgents directory".to_string())?;

    let agents_dir = plist_path.parent().unwrap();
    let _ = std::fs::create_dir_all(agents_dir);

    let log_dir = log_dir();
    let _ = std::fs::create_dir_all(&log_dir);

    let bin = ky_binary_path();
    let stdout_log = log_dir.join("autostart.out.log");
    let stderr_log = log_dir.join("autostart.err.log");

    let mut dict = plist::Dictionary::new();
    dict.insert("Label".into(), plist::Value::String(LABEL.into()));
    dict.insert(
        "ProgramArguments".into(),
        plist::Value::Array(vec![
            plist::Value::String(bin),
            plist::Value::String("start".into()),
            plist::Value::String("--autostart".into()),
        ]),
    );
    dict.insert("RunAtLoad".into(), plist::Value::Boolean(true));
    dict.insert(
        "StandardOutPath".into(),
        plist::Value::String(stdout_log.to_string_lossy().into()),
    );
    dict.insert(
        "StandardErrorPath".into(),
        plist::Value::String(stderr_log.to_string_lossy().into()),
    );

    let value = plist::Value::Dictionary(dict);
    value
        .to_file_xml(&plist_path)
        .map_err(|e| format!("error writing plist: {}", e))?;

    activate_result()?;
    Ok(format!("autostart enabled ({})", plist_path.display()))
}

#[cfg(target_os = "macos")]
fn activate_result() -> Result<String, String> {
    let plist_path =
        agent_path().ok_or_else(|| "could not determine LaunchAgents path".to_string())?;

    let uid = get_uid();
    let target = format!("gui/{}", uid);
    let result = std::process::Command::new("launchctl")
        .args(["bootstrap", &target, &plist_path.to_string_lossy()])
        .output();

    match result {
        Ok(output) if output.status.success() => Ok("autostart enabled".into()),
        Ok(output) => {
            let err = String::from_utf8_lossy(&output.stderr);
            let legacy = std::process::Command::new("launchctl")
                .args(["load", &plist_path.to_string_lossy()])
                .output();
            match legacy {
                Ok(o) if o.status.success() => Ok("autostart enabled (legacy)".into()),
                _ => Err(format!("failed to load agent: {}", err.trim())),
            }
        }
        Err(e) => Err(format!("failed to load agent: {}", e)),
    }
}

#[cfg(target_os = "macos")]
fn uninstall_result() -> Result<String, String> {
    let plist_path =
        agent_path().ok_or_else(|| "could not determine LaunchAgents path".to_string())?;

    if is_active() {
        let uid = get_uid();
        let target = format!("gui/{}/{}", uid, LABEL);
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &target])
            .output();
    }

    if plist_path.exists() {
        std::fs::remove_file(&plist_path).map_err(|e| format!("error removing plist: {}", e))?;
    }

    Ok("autostart disabled".into())
}

// ── Platform: Linux ──────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn ky_binary_path() -> String {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("ky"))
        .to_string_lossy()
        .to_string()
}

#[cfg(target_os = "linux")]
fn agent_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("systemd")
            .join("user")
            .join("kagaya-autostart.service"),
    )
}

#[cfg(target_os = "linux")]
fn is_installed() -> bool {
    agent_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_active() -> bool {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "kagaya-autostart.service"])
        .output();
    match output {
        Ok(o) => {
            let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
            status == "active"
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn install_result() -> Result<String, String> {
    let unit_path =
        agent_path().ok_or_else(|| "could not determine systemd user directory".to_string())?;

    let unit_dir = unit_path.parent().unwrap();
    let _ = std::fs::create_dir_all(unit_dir);

    let bin = ky_binary_path();
    let content = format!(
		"[Unit]\nDescription=kagaya autostart\n\n[Service]\nType=oneshot\nExecStart={} start --autostart\nRemainAfterExit=no\n\n[Install]\nWantedBy=default.target\n",
		bin
	);

    std::fs::write(&unit_path, &content).map_err(|e| format!("error writing unit file: {}", e))?;

    activate_result()?;
    Ok(format!("autostart enabled ({})", unit_path.display()))
}

#[cfg(target_os = "linux")]
fn activate_result() -> Result<String, String> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    let result = std::process::Command::new("systemctl")
        .args(["--user", "enable", "kagaya-autostart.service"])
        .output();
    match result {
        Ok(output) if output.status.success() => Ok("autostart enabled".into()),
        Ok(output) => {
            let err = String::from_utf8_lossy(&output.stderr);
            Err(format!("failed to enable service: {}", err.trim()))
        }
        Err(e) => Err(format!("error: {}", e)),
    }
}

#[cfg(target_os = "linux")]
fn uninstall_result() -> Result<String, String> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "kagaya-autostart.service"])
        .output();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "stop", "kagaya-autostart.service"])
        .output();

    if let Some(path) = agent_path() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("error removing unit file: {}", e))?;
        }
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    Ok("autostart disabled".into())
}

// ── Fallback for other platforms ─────────────────────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn agent_path() -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn is_installed() -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn is_active() -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn install_result() -> Result<String, String> {
    Err("autostart is not supported on this platform".into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn activate_result() -> Result<String, String> {
    Err("autostart is not supported on this platform".into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn uninstall_result() -> Result<String, String> {
    Err("autostart is not supported on this platform".into())
}
