use crate::config;
use crate::plist_sync;
use crate::utils;
use owo_colors::OwoColorize;

// ── Durable state ────────────────────────────────────────────────────────────
//
// projects.toml is the source of truth; plists are a compiled cache of it.
// Writing only the plist gets silently reverted by the next `ky start` /
// `ky restart`, so every autostart change writes the config first.

/// The single durable path: config first, then the plist cache.
fn apply_one(
    name: &str,
    entries: &std::collections::BTreeMap<String, config::ServiceEntry>,
    value: bool,
) -> Result<(), String> {
    persist_autostart(name, value)?;
    if !plist_sync::plist_exists(name) {
        if let Some(svc) = entries.get(name) {
            plist_sync::sync_service(svc).map(|_| ())?;
        }
    }
    plist_sync::set_run_at_load(name, value)
}

fn persist_autostart(name: &str, value: bool) -> Result<(), String> {
    let path = utils::config_dir().join("projects.toml");
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {}", path.display(), e))?;
    let updated = set_project_autostart(&content, name, value)
        .ok_or_else(|| format!("'{}' not found in projects.toml", name))?;
    if updated == content {
        return Ok(());
    }
    std::fs::write(&path, updated).map_err(|e| format!("writing {}: {}", path.display(), e))
}

/// The bare key of a `key = value` line, or None for blanks, comments and
/// table headers.
fn line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    let eq = trimmed.find('=')?;
    Some(trimmed[..eq].trim())
}

/// Rewrite the value of an existing `autostart = ...` line, keeping its
/// indentation and any trailing comment.
fn rewrite_bool_line(line: &str, value: bool) -> String {
    let eq = line.find('=').expect("caller matched a key = value line");
    let (prefix, rest) = line.split_at(eq + 1);
    match rest.find('#') {
        Some(c) => format!("{} {}  {}", prefix, value, &rest[c..]),
        None => format!("{} {}", prefix, value),
    }
}

/// Set `autostart` for a project in projects.toml content, preserving the
/// formatting, comments and ordering of every other line.
///
/// A simple entry (`name = "~/dir"`) has nowhere to hold the key, so it is
/// promoted to table form. The table is appended at the end of the file, not
/// written in place: a `[name]` header inserted mid-file would swallow every
/// following top-level key.
///
/// Returns None if the project is not in the file.
fn set_project_autostart(content: &str, name: &str, value: bool) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let header = format!("[{}]", name);

    if let Some(start) = lines.iter().position(|l| l.trim() == header) {
        let end = (start + 1..lines.len())
            .find(|&j| lines[j].trim().starts_with('['))
            .unwrap_or(lines.len());
        match (start + 1..end).find(|&j| line_key(lines[j]) == Some("autostart")) {
            Some(j) => out[j] = rewrite_bool_line(lines[j], value),
            None => {
                let mut at = end;
                while at > start + 1 && lines[at - 1].trim().is_empty() {
                    at -= 1;
                }
                out.insert(at, format!("autostart = {}", value));
            }
        }
        return Some(rejoin(out, content));
    }

    // Simple form: promote to a table appended at the end of the file.
    let i = lines.iter().position(|l| line_key(l) == Some(name))?;
    let eq = lines[i].find('=').expect("line_key matched an assignment");
    let dir = lines[i][eq + 1..].trim().to_string();
    out.remove(i);
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.push(String::new());
    out.push(header);
    out.push(format!("dir = {}", dir));
    out.push(format!("autostart = {}", value));
    Some(rejoin(out, content))
}

fn rejoin(lines: Vec<String>, original: &str) -> String {
    let mut s = lines.join("\n");
    if original.ends_with('\n') {
        s.push('\n');
    }
    s
}

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
        match apply_one(name, &entries, value) {
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
    match apply_one(name, &entries, value) {
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
        match apply_one(name, &entries, value) {
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

#[cfg(test)]
mod tests {
    use super::set_project_autostart;

    #[test]
    fn table_entry_updates_existing_key() {
        let content = "# my projects\n\n[canvas]\ndir = \"/Users/m/dev/canvas\"\nautostart = true\n\n[other]\ndir = \"/tmp\"\n";
        let out = set_project_autostart(content, "canvas", false).unwrap();
        assert_eq!(
            out,
            "# my projects\n\n[canvas]\ndir = \"/Users/m/dev/canvas\"\nautostart = false\n\n[other]\ndir = \"/tmp\"\n"
        );
    }

    #[test]
    fn table_entry_inserts_missing_key() {
        let content = "[canvas]\ndir = \"/tmp\"\ndepends_on = \"db\"\n\n[other]\ndir = \"/tmp\"\n";
        let out = set_project_autostart(content, "canvas", true).unwrap();
        assert_eq!(
            out,
            "[canvas]\ndir = \"/tmp\"\ndepends_on = \"db\"\nautostart = true\n\n[other]\ndir = \"/tmp\"\n"
        );
    }

    #[test]
    fn table_entry_preserves_trailing_comment() {
        let content = "[canvas]\nautostart = true # was on\n";
        let out = set_project_autostart(content, "canvas", false).unwrap();
        assert_eq!(out, "[canvas]\nautostart = false  # was on\n");
    }

    #[test]
    fn simple_entry_is_promoted_to_a_table_at_the_end() {
        let content = "# projects\nanani = \"~/dev/anani\"\nkagaya = \"~/dev/kagaya\"\n\n[canvas]\ndir = \"/tmp\"\n";
        let out = set_project_autostart(content, "anani", true).unwrap();
        assert_eq!(
            out,
            "# projects\nkagaya = \"~/dev/kagaya\"\n\n[canvas]\ndir = \"/tmp\"\n\n[anani]\ndir = \"~/dev/anani\"\nautostart = true\n"
        );
    }

    #[test]
    fn unrelated_lines_are_untouched() {
        let content = "# header comment\n\nanani = \"~/dev/anani\"   # keep me\n\n[canvas]\n# inner note\ndir = \"/tmp\"\nautostart = true\n";
        let out = set_project_autostart(content, "canvas", false).unwrap();
        assert!(out.contains("anani = \"~/dev/anani\"   # keep me"));
        assert!(out.contains("# header comment"));
        assert!(out.contains("# inner note"));
        assert!(out.contains("autostart = false"));
    }

    #[test]
    fn unknown_project_is_none() {
        assert!(set_project_autostart("[canvas]\ndir = \"/tmp\"\n", "nope", true).is_none());
    }

    #[test]
    fn result_stays_valid_toml_after_promotion() {
        let content = "anani = \"~/dev/anani\"\nkagaya = \"~/dev/kagaya\"\n";
        let out = set_project_autostart(content, "anani", false).unwrap();
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(parsed["anani"]["dir"].as_str(), Some("~/dev/anani"));
        assert_eq!(parsed["anani"]["autostart"].as_bool(), Some(false));
        assert_eq!(parsed["kagaya"].as_str(), Some("~/dev/kagaya"));
    }
}
