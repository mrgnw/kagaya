use crate::config;
use crate::plist_sync;
use owo_colors::OwoColorize;

pub struct AutostartInfo {
    pub installed: bool,
    pub active: bool,
    pub agent_path: Option<String>,
    pub projects: Vec<String>,
}

pub fn status_info() -> AutostartInfo {
    let entries = config::load_service_entries();
    let projects: Vec<String> = entries
        .keys()
        .filter(|name| {
            plist_sync::read_plist(name)
                .map(|i| i.run_at_load)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    let active = !projects.is_empty();
    AutostartInfo {
        installed: active,
        active,
        agent_path: None,
        projects,
    }
}

pub fn enable() -> Result<String, String> {
    set_all_result(true)
}

pub fn disable() -> Result<String, String> {
    set_all_result(false)
}

fn set_all_result(value: bool) -> Result<String, String> {
    let entries = config::load_service_entries();
    if entries.is_empty() {
        return Err("no services registered".to_string());
    }
    let mut lines = Vec::new();
    let mut errs = Vec::new();
    for name in entries.keys() {
        if !plist_sync::plist_exists(name) {
            if let Some(svc) = entries.get(name) {
                let _ = plist_sync::sync_service(svc);
            }
        }
        match plist_sync::set_run_at_load(name, value) {
            Ok(()) => lines.push(format!(
                "{}: autostart {}",
                name,
                if value { "enabled" } else { "disabled" }
            )),
            Err(e) => errs.push(format!("{}: {}", name, e)),
        }
    }
    if errs.is_empty() {
        Ok(lines.join("\n"))
    } else {
        Err(errs.join("\n"))
    }
}

// ── CLI entry point ──────────────────────────────────────────────────────────

pub fn cmd_autostart(args: &[String]) {
    let entries = config::load_service_entries();

    match args.first().map(|s| s.as_str()) {
        None | Some("status") | Some("st") => cmd_status_list(),
        Some("on") => match args.get(1) {
            Some(name) => set_one(name, true),
            None => set_all(true),
        },
        Some("off") => match args.get(1) {
            Some(name) => set_one(name, false),
            None => set_all(false),
        },
        Some("help") | Some("--help") | Some("-h") => print_usage(),
        Some(name) if entries.contains_key(name) => match args.get(1).map(|s| s.as_str()) {
            Some("on") => set_one(name, true),
            Some("off") => set_one(name, false),
            _ => show_one(name),
        },
        Some(other) => {
            eprintln!("unknown service or subcommand: {}", other);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("ky autostart — start services on login via launchd");
    eprintln!();
    eprintln!("usage:");
    eprintln!(
        "  {}            Show autostart state for each service",
        "ky autostart".bold()
    );
    eprintln!(
        "  {}     Enable autostart for a single service",
        "ky autostart <name> on".bold()
    );
    eprintln!(
        "  {}    Disable autostart for a single service",
        "ky autostart <name> off".bold()
    );
    eprintln!(
        "  {}         Enable autostart for every registered service",
        "ky autostart on".bold()
    );
    eprintln!(
        "  {}        Disable autostart for every registered service",
        "ky autostart off".bold()
    );
}

fn cmd_status_list() {
    let entries = config::load_service_entries();
    if entries.is_empty() {
        eprintln!("no services registered. run {} to add one", "ky add".bold());
        return;
    }
    for (name, _svc) in &entries {
        let on = plist_sync::read_plist(name)
            .map(|i| i.run_at_load)
            .unwrap_or(false);
        let marker = if on {
            "●".green().to_string()
        } else {
            "◻".dimmed().to_string()
        };
        let state = if on {
            "on".green().to_string()
        } else {
            "off".dimmed().to_string()
        };
        eprintln!("{} {}  {}", marker, state, name);
    }
}

fn show_one(name: &str) {
    match plist_sync::read_plist(name) {
        Some(info) => {
            let state = if info.run_at_load {
                "on".green().to_string()
            } else {
                "off".dimmed().to_string()
            };
            eprintln!("{}: autostart is {}", name, state);
        }
        None => {
            eprintln!(
                "{}: no plist. run {} first",
                name,
                format!("ky add {}", name).bold()
            );
            std::process::exit(1);
        }
    }
}

fn set_one(name: &str, value: bool) {
    let entries = config::load_service_entries();
    if !entries.contains_key(name) {
        eprintln!(
            "{}: not registered. run {} first",
            name,
            format!("ky add {}", name).bold()
        );
        std::process::exit(1);
    }
    if !plist_sync::plist_exists(name) {
        let svc = entries.get(name).unwrap();
        if let Err(e) = plist_sync::sync_service(svc) {
            eprintln!("{}: {}", name, e);
            std::process::exit(1);
        }
    }
    match plist_sync::set_run_at_load(name, value) {
        Ok(()) => {
            let verb = if value { "enabled" } else { "disabled" };
            eprintln!("{}: autostart {}", name, verb);
        }
        Err(e) => {
            eprintln!("{}: {}", name, e);
            std::process::exit(1);
        }
    }
}

fn set_all(value: bool) {
    let entries = config::load_service_entries();
    if entries.is_empty() {
        eprintln!("no services registered");
        return;
    }
    let mut any_err = false;
    for name in entries.keys() {
        if !plist_sync::plist_exists(name) {
            if let Some(svc) = entries.get(name) {
                let _ = plist_sync::sync_service(svc);
            }
        }
        match plist_sync::set_run_at_load(name, value) {
            Ok(()) => eprintln!(
                "{}: autostart {}",
                name,
                if value { "enabled" } else { "disabled" }
            ),
            Err(e) => {
                eprintln!("{}: {}", name, e);
                any_err = true;
            }
        }
    }
    if any_err {
        std::process::exit(1);
    }
}
