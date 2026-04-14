use crate::config::ServiceEntry;
use crate::launchd::{get_uid, user_agents_dir, KAGAYA_PREFIX};
use crate::logs::log_dir;
use std::path::PathBuf;
use std::process::Command;

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

fn bootstrap(path: &PathBuf) -> Result<(), String> {
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

fn bootout(name: &str) -> Result<(), String> {
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
