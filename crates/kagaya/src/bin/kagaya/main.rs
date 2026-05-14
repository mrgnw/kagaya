mod autostart;
mod cli;
mod config;
mod detect;
mod format;
mod koku_client;
mod launchd;
mod logs;
mod migrate;
mod plist_sync;
mod self_update;
mod utils;

use clap::Parser;
use cli::{output_format, set_output_format, Cli, Cmd, OutputFormat, ServeAction};
use config::ServiceEntry;
use kagaya::*;
use owo_colors::OwoColorize;
use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Local result shape used by the remaining action handlers (post-daemon).
enum Response {
    Ok { message: Option<String> },
    Error { message: String },
}

fn main() {
    // Two-pass parsing: try clap first, fall back to service-name dispatch
    match Cli::try_parse() {
        Ok(cli) => {
            if cli.json {
                set_output_format(OutputFormat::Json);
            } else if cli.tsv {
                set_output_format(OutputFormat::Tsv);
            }

            if cli.help {
                if let Some(ref cmd) = cli.command {
                    print_subcommand_help(cmd);
                } else {
                    print_usage();
                }
                return;
            }
            if cli.version {
                println!("kagaya {}", env!("CARGO_PKG_VERSION"));
                return;
            }

            match cli.command {
                None => {
                    if cli.watch {
                        cmd_status(&["--watch".to_string()]);
                    } else {
                        render_condensed_status(&[]);
                    }
                }
                Some(Cmd::Status {
                    names,
                    all,
                    detailed,
                    watch,
                    watch_interval,
                }) => {
                    let mut args = names;
                    if all {
                        args.push("--all".to_string());
                    }
                    if detailed {
                        args.push("--detailed".to_string());
                    }
                    if watch || cli.watch {
                        args.push("--watch".to_string());
                    }
                    if let Some(iv) = watch_interval {
                        args.push("--watch-interval".to_string());
                        args.push(iv.to_string());
                    }
                    cmd_status(&args);
                }
                Some(Cmd::Start {
                    names,
                    all,
                    autostart,
                    detailed,
                    echo,
                    wait,
                    force,
                    watch,
                    no_watch,
                    watch_interval,
                }) => {
                    let mut args = names.clone();
                    if all {
                        args.push("--all".to_string());
                    }
                    if detailed {
                        args.push("--detailed".to_string());
                    }
                    if autostart {
                        args.push("--autostart".to_string());
                    }
                    if wait {
                        args.push("--wait".to_string());
                    }
                    if force {
                        args.push("--force".to_string());
                    }
                    if watch || cli.watch {
                        args.push("--watch".to_string());
                    }
                    if no_watch {
                        args.push("--no-watch".to_string());
                    }
                    if let Some(iv) = watch_interval {
                        args.push("--watch-interval".to_string());
                        args.push(iv.to_string());
                    }
                    cmd_start(&args);
                    if echo {
                        echo_after_action(&names, None);
                    }
                }
                Some(Cmd::Stop {
                    names,
                    all,
                    detailed,
                    echo,
                    watch,
                    no_watch,
                    watch_interval,
                }) => {
                    let mut args = names.clone();
                    if all {
                        args.push("--all".to_string());
                    }
                    if detailed {
                        args.push("--detailed".to_string());
                    }
                    if watch || cli.watch {
                        args.push("--watch".to_string());
                    }
                    if no_watch {
                        args.push("--no-watch".to_string());
                    }
                    if let Some(iv) = watch_interval {
                        args.push("--watch-interval".to_string());
                        args.push(iv.to_string());
                    }
                    cmd_stop(&args);
                    if echo {
                        echo_after_stop(&names);
                    }
                }
                Some(Cmd::Restart {
                    target,
                    all,
                    detailed,
                    echo,
                    force,
                    watch,
                    no_watch,
                    watch_interval,
                }) => {
                    let mut args = target.clone();
                    if all {
                        args.push("--all".to_string());
                    }
                    if detailed {
                        args.push("--detailed".to_string());
                    }
                    if force {
                        args.push("--force".to_string());
                    }
                    if watch || cli.watch {
                        args.push("--watch".to_string());
                    }
                    if no_watch {
                        args.push("--no-watch".to_string());
                    }
                    if let Some(iv) = watch_interval {
                        args.push("--watch-interval".to_string());
                        args.push(iv.to_string());
                    }
                    cmd_restart(&args);
                    if echo {
                        echo_after_action(&target, None);
                    }
                }
                Some(Cmd::Logs { args }) => cmd_logs(&args),
                Some(Cmd::Tail { args }) => cmd_tail(&args),
                Some(Cmd::Echo { args }) => cmd_echo(&args),
                Some(Cmd::Show { args }) => cmd_show(&args),
                Some(Cmd::Cron { args }) => cmd_cron(&args),
                Some(Cmd::ReloadConfig) => cmd_reload_config(),
                Some(Cmd::Serve { action }) => cmd_serve(action),
                Some(Cmd::Add { args, run }) => cmd_add(&args, run.as_deref()),
                Some(Cmd::Remove { args }) => cmd_remove(&args),
                Some(Cmd::Init) => cmd_init(),
                Some(Cmd::Migrate { force }) => migrate::cmd_migrate(force),
                Some(Cmd::Autostart { args }) => autostart::cmd_autostart(&args),
                Some(Cmd::Launchd { args }) => launchd::cmd_launchd(&args),
                Some(Cmd::SelfCmd { args }) => match args.first().map(|s| s.as_str()) {
                    Some("update") => self_update::cmd_self_update(),
                    _ => {
                        eprintln!("usage: ky self update");
                        std::process::exit(1);
                    }
                },
                Some(Cmd::All) => cmd_status(&["all".to_string()]),
                Some(Cmd::Help) => print_usage(),
                Some(Cmd::Version) => println!("kagaya {}", env!("CARGO_PKG_VERSION")),
                Some(Cmd::External(args)) => dispatch_external(&args),
            }
        }
        Err(e) => {
            // clap error — could be bad flags, etc.
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn dispatch_external(args: &[String]) {
    if args.is_empty() {
        print_usage();
        return;
    }

    let name = &args[0];

    if name == "--help" || name == "-h" || name == "help" {
        print_usage();
        return;
    }
    if name == "--version" || name == "-V" || name == "version" {
        println!("kagaya {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let services = config::load_service_entries();
    let base_name = name.split('.').next().unwrap_or(name);
    if services.contains_key(base_name) && args.len() > 1 {
        match args[1].as_str() {
            "start" => cmd_start(&[args[0].clone()]),
            "stop" => cmd_stop(&[args[0].clone()]),
            "status" | "st" => cmd_status(&[args[0].clone()]),
            "logs" => cmd_logs(args),
            "tail" => cmd_tail(args),
            "echo" => cmd_echo(args),
            "show" => cmd_show(args),
            "restart" => {
                let mut restart_args = vec![args[0].clone()];
                if args.len() > 2 {
                    restart_args.push(args[2].clone());
                }
                cmd_restart(&restart_args);
            }
            _ => {
                eprintln!("unknown command: {}", args[1]);
                std::process::exit(1);
            }
        }
    } else if services.contains_key(base_name) {
        cmd_status(&[args[0].clone()]);
    } else {
        eprintln!("unknown command or service: {}", name);
        eprintln!();
        eprintln!("available commands: status, start, stop, restart, logs, echo, show, add, remove, init, daemon, reload-config, serve, cron, autostart");
        eprintln!();
        let names: Vec<&str> = services.keys().map(|s| s.as_str()).collect();
        if !names.is_empty() {
            eprintln!("registered services: {}", names.join(", "));
        }
        eprintln!();
        eprintln!("run 'ky help' for usage");
        std::process::exit(1);
    }
}

fn hline(cmd: &str, args: &str, desc: &str, cmd_width: usize) {
    let pad = cmd_width.saturating_sub(cmd.len());
    if args.is_empty() {
        eprintln!("  {}{:pad$}  {}", cmd.bold(), "", desc.dimmed());
    } else {
        eprintln!("  {}{:pad$}  {}  {}", cmd.bold(), "", args, desc.dimmed());
    }
}

fn print_usage() {
    eprintln!(
        "{} {} — launchctl frontend for services",
        "ky".bold(),
        env!("CARGO_PKG_VERSION")
    );
    eprintln!();
    eprintln!("usage: {} [command] [service] [options]", "ky".bold());
    eprintln!();

    let w = 16; // column width for command names
    eprintln!("{}", "services".cyan().bold());
    hline(
        "status|st",
        "[name|--all]",
        "Show status (default command)",
        w,
    );
    hline("start", "[name|--all]", "Start service(s)", w);
    hline("stop", "[name|--all]", "Stop service(s)", w);
    hline(
        "restart",
        "[name|--all]",
        "Restart service(s) or a single process",
        w,
    );
    eprintln!();
    eprintln!(
        "  start/stop/restart auto-watch status briefly; {} to skip",
        "-W".bold()
    );
    eprintln!(
        "  {} streams live output after action; {} shows per-process detail",
        "-e".bold(),
        "-d".bold()
    );
    eprintln!();

    eprintln!("{}", "logs".cyan().bold());
    hline("logs", "<name> [process]", "Show log file paths", w);
    hline("echo", "<name> [process]", "Tail + stream live output", w);
    eprintln!();

    eprintln!("{}", "config".cyan().bold());
    hline(
        "show",
        "[name] [process]",
        "Show config or process command",
        w,
    );
    hline("add", "[name] [dir]", "Register a project", w);
    hline(
        "add",
        "<name> --run <cmd>",
        "Register a standalone command",
        w,
    );
    hline("remove|rm", "<name>", "Unregister a project", w);
    hline("init", "", "Create config files", w);
    eprintln!();

    eprintln!("{}", "cron (via koku)".cyan().bold());
    hline("cron", "[status]", "Show cron job status", w);
    hline("cron", "run|pause|resume", "Manage individual jobs", w);
    hline("cron", "reload", "Reload koku config", w);
    eprintln!();

    eprintln!("{}", "system".cyan().bold());
    hline(
        "autostart",
        "[<name>] [on|off]",
        "RunAtLoad toggle per service",
        w,
    );
    hline(
        "reload-config|rc",
        "",
        "Re-sync plists from projects.toml",
        w,
    );
    hline("serve", "[stop|status]", "HTTP UI launchd agent", w);
    hline("launchd|lctl", "[command]", "macOS launchd escape hatch", w);
    hline("self update", "", "Update to latest version", w);
    eprintln!();

    eprintln!("{}", "targeting".cyan().bold());
    eprintln!(
        "  {} dot syntax targets one process: ky start app.web",
        "name.process".bold()
    );
    eprintln!("  Service-first syntax:              ky myapp start");
    eprintln!("  Context-aware: run from a project dir to auto-target it");
    eprintln!();

    eprintln!("{}", "shortcuts".cyan().bold());
    eprintln!("  ky                  status (current project or all)");
    eprintln!("  ky all              status --all");
    eprintln!("  ky -w               live watch mode");
    eprintln!("  ky <service>        status for a single service");
    eprintln!();

    eprintln!("{}", "dependencies".cyan().bold());
    eprintln!(
        "  In services.toml, use {} to order startup:",
        "depends_on".bold()
    );
    eprintln!("    [api]");
    eprintln!("    run = \"python server.py\"");
    eprintln!("    depends_on = \"db\"");
    eprintln!(
        "  {} polls a command until exit 0; {} sets timeout (default 10s)",
        "ready".bold(),
        "ready_timeout".bold()
    );
    eprintln!();

    eprintln!("{}", "files".cyan().bold());
    eprintln!(
        "  {}   registered projects",
        "~/.config/kagaya/projects.toml".dimmed()
    );
    eprintln!(
        "  {}     global defaults",
        "~/.config/kagaya/config.toml".dimmed()
    );
    eprintln!(
        "  {}          per-project services",
        "<project>/services.toml".dimmed()
    );
    eprintln!();

    eprintln!("Run {} for detailed options", "ky <command> --help".bold());
}

fn print_subcommand_help(cmd: &Cmd) {
    use clap::CommandFactory;
    let name = match cmd {
        Cmd::Status { .. } => "status",
        Cmd::Start { .. } => "start",
        Cmd::Stop { .. } => "stop",
        Cmd::Restart { .. } => "restart",
        Cmd::Logs { .. } => "logs",
        Cmd::Echo { .. } => "echo",
        Cmd::Show { .. } => "show",
        Cmd::Cron { .. } => "cron",
        Cmd::ReloadConfig => "reload-config",
        Cmd::Serve { .. } => "serve",
        Cmd::Add { .. } => "add",
        Cmd::Remove { .. } => "remove",
        Cmd::Init => "init",
        Cmd::Migrate { .. } => "migrate",
        Cmd::Autostart { .. } => "autostart",
        Cmd::Launchd { .. } => "launchd",
        Cmd::SelfCmd { .. } => "self",
        _ => {
            print_usage();
            return;
        }
    };
    let mut app = Cli::command();
    if let Some(sub) = app.find_subcommand_mut(name) {
        sub.print_help().ok();
    } else {
        print_usage();
    }
}

// --- Config management (no daemon needed) ---

fn cmd_init() {
    let config_dir = utils::config_dir();
    let _ = std::fs::create_dir_all(&config_dir);

    let projects_file = config_dir.join("projects.toml");
    if !projects_file.exists() {
        let content = "# myapp = \"~/dev/myapp\"\n#\n# [myapp]\n# dir = \"~/dev/myapp\"\n# autostart = true          # start on login (ky autostart on)\n#\n# [tunnel]\n# run = \"ssh -N -L 5432:localhost:5432 myserver\"\n";
        let _ = std::fs::write(&projects_file, content);
        eprintln!("created {}", projects_file.display());
    } else {
        eprintln!("already exists: {}", projects_file.display());
    }

    eprintln!();
    eprintln!("getting started:");
    eprintln!("  1. add projects: ky add (from a project dir)");
    eprintln!("  2. start: ky start [name|--all]");
    eprintln!("  3. check: ky status");
}

/// Insert text before the first TOML table header (`[section]`).
/// If there are no table headers, appends to the end.
/// Ensures a trailing newline before the insertion point.
fn insert_before_first_table(content: &str, new_line: &str) -> String {
    let mut insert_pos = None;
    let mut byte_offset = 0usize;
    for line in content.lines() {
        if line.starts_with('[') {
            insert_pos = Some(byte_offset);
            break;
        }
        byte_offset += line.len() + 1; // +1 for newline
    }

    match insert_pos {
        Some(pos) => {
            let mut result = String::with_capacity(content.len() + new_line.len() + 1);
            let before = &content[..pos];
            let after = &content[pos..];
            result.push_str(before);
            if !before.is_empty() && !before.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(new_line);
            if !new_line.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(after);
            result
        }
        None => {
            let mut result = content.to_string();
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(new_line);
            result
        }
    }
}

/// Ensure services.toml exists — offer to create it if missing.
/// Returns true if services.toml exists (pre-existing or just created).
fn ensure_services_toml(dir: &Path) -> bool {
    let services_toml = dir.join("services.toml");
    if services_toml.exists() {
        return true;
    }
    let suggestions = detect::detect_services(dir);
    if !suggestions.is_empty() && io::stdin().is_terminal() {
        let detected = detect::describe_detected(dir);
        let toml_content = detect::format_services_toml(&suggestions);
        eprintln!("detected: {}", detected.join(", "));
        eprintln!();
        eprintln!("{}:", services_toml.display());
        for line in toml_content.lines() {
            eprintln!("  {}", line);
        }
        eprintln!();
        eprint!("create services.toml? [Y/n] ");
        let mut input = String::new();
        let confirmed = if io::stdin().read_line(&mut input).is_ok() {
            let input = input.trim().to_lowercase();
            input.is_empty() || input == "y" || input == "yes"
        } else {
            false
        };
        if confirmed {
            std::fs::write(&services_toml, &toml_content).unwrap();
            eprintln!("wrote {}", services_toml.display());
            return true;
        }
    } else if suggestions.is_empty() {
        eprintln!("note: no services.toml found in {}", dir.display());
        eprintln!("create one with service definitions, e.g.:");
        eprintln!("  web = \"npm run dev\"");
    }
    false
}

fn cmd_add(args: &[String], run: Option<&str>) {
    let config_dir = utils::config_dir();
    let _ = std::fs::create_dir_all(&config_dir);
    let projects_file = config_dir.join("projects.toml");

    if let Some(cmd) = run {
        // Standalone command mode: ky add <name> --run <command>
        let name = if let Some(n) = args.first() {
            n.clone()
        } else {
            eprintln!("error: --run requires a name: ky add <name> --run <command>");
            std::process::exit(1);
        };

        let existing_content = std::fs::read_to_string(&projects_file).unwrap_or_default();
        if let Ok(table) = toml::from_str::<toml::Value>(&existing_content) {
            if let Some(map) = table.as_table() {
                if map.contains_key(&name) {
                    eprintln!("{}: already registered", name);
                    return;
                }
            }
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&projects_file)
            .unwrap();
        writeln!(file, "\n[{}]\nrun = {:?}", name, cmd).unwrap();
        eprintln!("{}: added (run: {})", name, cmd);
        sync_plist_after_add(&name);
        return;
    }

    // Directory-based project mode
    let (name, dir) = if args.len() >= 2 {
        (args[0].clone(), PathBuf::from(&args[1]))
    } else if args.len() == 1 {
        let dir = std::env::current_dir().unwrap();
        (args[0].clone(), dir)
    } else {
        let dir = std::env::current_dir().unwrap();
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();
        (name, dir)
    };

    let dir = dir.canonicalize().unwrap_or(dir);

    if !dir.exists() {
        eprintln!("error: directory does not exist: {}", dir.display());
        std::process::exit(1);
    }

    let existing_content = std::fs::read_to_string(&projects_file).unwrap_or_default();
    if let Ok(table) = toml::from_str::<toml::Value>(&existing_content) {
        if let Some(map) = table.as_table() {
            let already = if map.contains_key(&name) {
                true
            } else {
                // Also check inside table sections for duplicate keys
                map.iter().any(|(_section, val)| {
                    val.as_table().is_some_and(|sub| sub.contains_key(&name))
                })
            };
            if already {
                eprintln!("{}: already registered", name);
                ensure_services_toml(&dir);
                return;
            }
        }
    }

    if !ensure_services_toml(&dir) {
        return;
    }

    let new_line = format!("{} = {:?}\n", name, dir.display().to_string());
    // Insert before the first table header so the entry stays top-level
    let updated = insert_before_first_table(&existing_content, &new_line);
    std::fs::write(&projects_file, updated).unwrap();
    eprintln!("{}: added ({})", name, dir.display());
    sync_plist_after_add(&name);
}

fn sync_plist_after_add(name: &str) {
    let entries = config::load_service_entries();
    let Some(svc) = entries.get(name) else {
        return;
    };
    match plist_sync::sync_service(svc) {
        Ok(0) => eprintln!("{}: plist unchanged", name),
        Ok(_) => eprintln!("{}: plist written", name),
        Err(e) => eprintln!("{}: plist not written: {}", name, e),
    }
}

fn cmd_remove(args: &[String]) {
    let name = if let Some(n) = args.first() {
        n.clone()
    } else {
        let dir = std::env::current_dir().unwrap();
        dir.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
    };

    let config_dir = utils::config_dir();
    let projects_file = config_dir.join("projects.toml");

    let content = match std::fs::read_to_string(&projects_file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("no projects.toml found");
            std::process::exit(1);
        }
    };

    let new_content = match remove_project_entry(&content, &name) {
        Some(c) => c,
        None => {
            eprintln!("{}: not found in projects.toml", name);
            std::process::exit(1);
        }
    };

    std::fs::write(&projects_file, new_content).unwrap();

    // Clean up _commands/ dir for standalone commands
    let commands_dir = config_dir.join("_commands").join(&name);
    if commands_dir.exists() {
        let _ = std::fs::remove_dir_all(&commands_dir);
    }

    if let Err(e) = plist_sync::remove_service(&name) {
        eprintln!("{}: plist cleanup: {}", name, e);
    }

    eprintln!("{}: removed", name);
}

/// Remove a project entry from projects.toml content, preserving formatting.
/// Returns None if the entry was not found.
/// Handles both simple entries (`name = "..."`) and table entries (`[name]\n...`).
fn remove_project_entry(content: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut remove_start = None;
    let mut remove_end = None;

    // Look for a table header: [name]
    let table_header = format!("[{}]", name);
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == table_header {
            remove_start = Some(i);
            // Find the end: next table header or EOF
            remove_end = Some(lines.len());
            for j in (i + 1)..lines.len() {
                let t = lines[j].trim();
                if t.starts_with('[') && !t.starts_with("[[") {
                    remove_end = Some(j);
                    break;
                }
            }
            break;
        }
    }

    // If no table header found, look for a simple key: name = "..."
    if remove_start.is_none() {
        let key_prefix = format!("{} ", name);
        let key_prefix_eq = format!("{}=", name);
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if (trimmed.starts_with(&key_prefix) || trimmed.starts_with(&key_prefix_eq))
                && trimmed.contains('=')
            {
                // Verify this is actually a key assignment for our name by parsing the key
                if let Some(eq_pos) = trimmed.find('=') {
                    let key = trimmed[..eq_pos].trim();
                    if key == name {
                        remove_start = Some(i);
                        remove_end = Some(i + 1);
                        break;
                    }
                }
            }
        }
    }

    let start = remove_start?;
    let end = remove_end.unwrap();

    // Strip trailing blank lines from the removed block
    let mut actual_end = end;
    while actual_end > start + 1
        && lines
            .get(actual_end - 1)
            .map_or(false, |l| l.trim().is_empty())
    {
        actual_end -= 1;
    }

    let mut result_lines: Vec<&str> = Vec::new();
    result_lines.extend_from_slice(&lines[..start]);
    // Skip blank lines immediately before the removed block too
    while result_lines.last().map_or(false, |l| l.trim().is_empty()) {
        result_lines.pop();
    }
    if !result_lines.is_empty() && end < lines.len() {
        result_lines.push(""); // single blank separator
    }
    result_lines.extend_from_slice(&lines[end..]);

    // Trim trailing blank lines from result
    while result_lines.last().map_or(false, |l| l.trim().is_empty()) {
        result_lines.pop();
    }

    let mut result = result_lines.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    Some(result)
}

// --- Commands ---

fn cmd_status(args: &[String]) {
    let (watch, rest) = parse_watch_opts(args, None);
    if watch.enabled && !output_format().is_plain() && io::stdout().is_terminal() {
        watch_status(&rest, &watch);
    } else {
        let data = gather_status_data(&rest);
        if data.detailed {
            render_detailed_status(&rest);
        } else {
            render_condensed_status(&rest);
        }
    }
}

fn format_state_duration(state_since: Option<u64>) -> String {
    match state_since {
        Some(ts) => format_uptime(utils::duration_since_timestamp(ts)),
        None => String::new(),
    }
}

type AggregateState = ServiceState;

fn aggregate_state(status: &ServiceStatus) -> AggregateState {
    status.aggregate_state()
}

fn aggregate_symbol(agg: AggregateState) -> String {
    match agg {
        AggregateState::On => "●".green().to_string(),
        AggregateState::Degraded => "⚠".yellow().to_string(),
        AggregateState::Err => "✖".red().to_string(),
        AggregateState::Off => "◻".dimmed().to_string(),
    }
}

fn aggregate_label(agg: AggregateState) -> String {
    match agg {
        AggregateState::On => "on".green().to_string(),
        AggregateState::Degraded => "deg".yellow().to_string(),
        AggregateState::Err => "err".red().to_string(),
        AggregateState::Off => "off".dimmed().to_string(),
    }
}

fn color_duration(duration: &str, agg: AggregateState) -> String {
    if duration.is_empty() {
        return String::new();
    }
    match agg {
        AggregateState::On => duration.green().to_string(),
        AggregateState::Degraded => duration.yellow().to_string(),
        AggregateState::Err => duration.red().to_string(),
        AggregateState::Off => duration.dimmed().to_string(),
    }
}

fn process_mini_icon(proc: &ProcessStatus) -> String {
    match &proc.state {
        ProcessState::Running { .. } if is_port_pending(proc) => "◌".cyan().to_string(),
        ProcessState::Running { .. } => "•".green().to_string(),
        ProcessState::Stopped if !proc.autostart => "◦".dimmed().to_string(),
        ProcessState::Stopped => "◦".dimmed().to_string(),
        ProcessState::Crashed { .. } => "⚠".yellow().to_string(),
        ProcessState::Failed { .. } => "✖".red().to_string(),
    }
}

fn process_mini_icon_key(proc: &ProcessStatus) -> u8 {
    match &proc.state {
        ProcessState::Running { .. } if is_port_pending(proc) => 0,
        ProcessState::Running { .. } => 1,
        ProcessState::Stopped if !proc.autostart => 2,
        ProcessState::Stopped => 3,
        ProcessState::Crashed { .. } => 4,
        ProcessState::Failed { .. } => 5,
    }
}

fn format_condensed_mini_icons(status: &ServiceStatus) -> String {
    let procs = &status.processes;
    if procs.len() <= 4 {
        return procs.iter().map(|p| process_mini_icon(p)).collect();
    }

    let mut groups: Vec<(u8, usize)> = Vec::new();
    for proc in procs {
        let key = process_mini_icon_key(proc);
        if let Some(last) = groups.last_mut() {
            if last.0 == key {
                last.1 += 1;
                continue;
            }
        }
        groups.push((key, 1));
    }

    let dummy_procs: Vec<ProcessStatus> = groups
        .iter()
        .map(|(key, _)| {
            let state = match key {
                0 => ProcessState::Running {
                    pid: 0,
                    uptime_secs: 0,
                },
                1 => ProcessState::Running {
                    pid: 0,
                    uptime_secs: 0,
                },
                2 => ProcessState::Stopped,
                3 => ProcessState::Stopped,
                4 => ProcessState::Crashed {
                    exit_code: 1,
                    retries: 0,
                },
                _ => ProcessState::Failed { exit_code: 1 },
            };
            ProcessStatus {
                name: String::new(),
                state,
                pid: None,
                autostart: *key != 2,
                service_type: ServiceType::Service,
                ports: vec![],
                ports_expected: if *key == 0 { vec![1] } else { vec![] },
                state_since: None,
                cpu_percent: None,
                memory_bytes: None,
            }
        })
        .collect();

    groups
        .iter()
        .zip(dummy_procs.iter())
        .map(|((_, count), dummy)| {
            let icon = process_mini_icon(dummy);
            if *count > 1 {
                format!("{}{}", icon, count)
            } else {
                icon
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn process_state_color(proc: &ProcessStatus) -> AggregateState {
    match &proc.state {
        ProcessState::Running { .. } => AggregateState::On,
        ProcessState::Stopped => AggregateState::Off,
        ProcessState::Crashed { .. } => AggregateState::Degraded,
        ProcessState::Failed { .. } => AggregateState::Err,
    }
}

fn relevant_state_since(status: &ServiceStatus, agg: AggregateState) -> Option<u64> {
    match agg {
        AggregateState::On | AggregateState::Degraded => status
            .processes
            .iter()
            .filter(|p| p.state.is_running())
            .filter_map(|p| p.state_since)
            .max(),
        AggregateState::Err => status
            .processes
            .iter()
            .filter(|p| {
                matches!(
                    p.state,
                    ProcessState::Failed { .. } | ProcessState::Crashed { .. }
                )
            })
            .filter_map(|p| p.state_since)
            .max(),
        AggregateState::Off => status.processes.iter().filter_map(|p| p.state_since).max(),
    }
}

fn format_port_ranges(ports: &[u16]) -> String {
    if ports.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < ports.len() {
        let start = ports[i];
        let mut end = start;
        while i + 1 < ports.len() && ports[i + 1] == end + 1 {
            i += 1;
            end = ports[i];
        }
        if end - start >= 2 {
            parts.push(format!("{}..{}", start, end));
        } else {
            parts.push(start.to_string());
            if end != start {
                parts.push(end.to_string());
            }
        }
        i += 1;
    }
    parts.join(", ")
}

fn format_condensed_ports(status: &ServiceStatus) -> String {
    let mut configured: Vec<u16> = Vec::new();
    let mut detected: Vec<u16> = Vec::new();

    for proc in &status.processes {
        if !proc.state.is_running() {
            continue;
        }
        for &port in &proc.ports {
            if proc.ports_expected.contains(&port) {
                if !configured.contains(&port) {
                    configured.push(port);
                }
            } else if !detected.contains(&port) {
                detected.push(port);
            }
        }
    }

    configured.sort();
    detected.sort();

    if configured.is_empty() && detected.is_empty() {
        return String::new();
    }

    if configured.is_empty() {
        let max_show = 3;
        let shown: Vec<u16> = detected.iter().copied().take(max_show).collect();
        let rest = detected.len().saturating_sub(max_show);
        let mut s = format_port_ranges(&shown);
        if rest > 0 {
            s.push_str(&format!(" (+{})", rest));
        }
        return s;
    }

    let mut s = format_port_ranges(&configured);
    if !detected.is_empty() {
        s.push_str(&format!(" (+{})", detected.len()));
    }
    s
}

fn print_process_line(proc: &ProcessStatus, name_width: usize) {
    let pcolor = process_state_color(proc);
    let (symbol, label, extra) = match &proc.state {
        ProcessState::Running { .. } if is_port_pending(proc) => {
            let duration = format_state_duration(proc.state_since);
            let extra = format!("{:<8}", color_duration(&duration, pcolor));
            ("◌".cyan().to_string(), "starting".cyan().to_string(), extra)
        }
        ProcessState::Running { .. } => {
            let duration = format_state_duration(proc.state_since);
            let ports = format_port_ranges(&proc.ports);
            let extra = format!("{:<8} {}", color_duration(&duration, pcolor), ports);
            ("•".green().to_string(), "on".green().to_string(), extra)
        }
        ProcessState::Stopped if !proc.autostart => {
            let duration = format_state_duration(proc.state_since);
            let extra = if duration.is_empty() {
                String::new()
            } else {
                color_duration(&duration, pcolor)
            };
            (
                "◦".dimmed().to_string(),
                "optional".dimmed().to_string(),
                extra,
            )
        }
        ProcessState::Stopped => {
            let duration = format_state_duration(proc.state_since);
            let extra = if duration.is_empty() {
                String::new()
            } else {
                color_duration(&duration, pcolor)
            };
            ("◦".dimmed().to_string(), "off".dimmed().to_string(), extra)
        }
        ProcessState::Crashed { exit_code, retries } => {
            let duration = format_state_duration(proc.state_since);
            let dur_str = if duration.is_empty() {
                String::new()
            } else {
                format!("{}  ", color_duration(&duration, pcolor))
            };
            let extra = format!("{}exit {}  retry {}", dur_str, exit_code, retries);
            (
                "⚠".yellow().to_string(),
                "crashed".yellow().to_string(),
                extra,
            )
        }
        ProcessState::Failed { exit_code } => {
            let duration = format_state_duration(proc.state_since);
            let dur_str = if duration.is_empty() {
                String::new()
            } else {
                format!("{}  ", color_duration(&duration, pcolor))
            };
            let extra = format!("{}exit {}", dur_str, exit_code);
            ("✖".red().to_string(), "failed".red().to_string(), extra)
        }
    };
    let dotname = format!(".{}", proc.name);
    let extra_str = if extra.is_empty() {
        String::new()
    } else {
        format!("  {}", extra.trim_end())
    };
    println!(
        "{} {:<width$} {}{}",
        symbol,
        dotname,
        label,
        extra_str,
        width = name_width
    );
}

fn response_from_ops(results: Vec<plist_sync::OpResult>) -> Response {
    let has_err = results.iter().any(|r| !r.ok);
    let body = results
        .iter()
        .map(|r| r.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if has_err {
        Response::Error { message: body }
    } else {
        Response::Ok {
            message: Some(body),
        }
    }
}

fn handle_action_response(response: &Response) {
    let fmt = output_format();
    match response {
        Response::Ok { message } => {
            if fmt == OutputFormat::Json {
                format::json_ok(message.clone());
            } else {
                if let Some(msg) = message {
                    for line in msg.lines() {
                        eprintln!("{}", line);
                    }
                }
            }
        }
        Response::Error { message } => {
            if fmt == OutputFormat::Json {
                format::json_error(message);
                std::process::exit(1);
            } else {
                eprintln!("error: {}", message);
                std::process::exit(1);
            }
        }
        _ => {}
    }
}

fn cmd_start(args: &[String]) {
    let (mut watch, rest) = parse_watch_opts(args, Some(4));
    let entries = config::load_service_entries();
    let plain = output_format().is_plain();

    let autostart_only = rest.iter().any(|a| a == "--autostart");
    let start_all = rest.iter().any(|a| is_all_flag(a));
    let detailed = rest.iter().any(|a| is_detailed_flag(a));
    let wait_for_ready = rest.iter().any(|a| a == "--wait");
    let force = rest.iter().any(|a| a == "--force" || a == "-f");
    let rest: Vec<String> = rest
        .into_iter()
        .filter(|a| {
            !is_all_flag(a)
                && !is_detailed_flag(a)
                && a != "--autostart"
                && a != "--wait"
                && a != "--force"
                && a != "-f"
        })
        .collect();

    if autostart_only {
        let (names, _chains) = config::autostart_sorted();
        if names.is_empty() {
            if plain {
                format::json_error("no projects with autostart = true");
            } else {
                eprintln!("no projects with autostart = true");
            }
            return;
        }
        let _ = force;
        let response = response_from_ops(plist_sync::start_services(
            &names,
            &plist_sync::ProcessFilters::new(),
        ));
        handle_action_response(&response);
        return;
    }

    // Parse `..` chain syntax: "db..api..worker" becomes a chain [db, api, worker]
    let (chain_args, plain_args): (Vec<String>, Vec<String>) =
        rest.iter().cloned().partition(|a| a.contains(".."));

    let mut chains: Vec<Vec<String>> = Vec::new();
    let mut chain_names: Vec<String> = Vec::new();
    for arg in &chain_args {
        let chain: Vec<String> = arg.split("..").map(|s| s.to_string()).collect();
        for name in &chain {
            if !chain_names.contains(name) {
                chain_names.push(name.clone());
            }
        }
        chains.push(chain);
    }

    let combined_args: Vec<String> = if start_all && plain_args.is_empty() && chain_names.is_empty()
    {
        vec!["--all".to_string()]
    } else {
        let mut args = plain_args.clone();
        args.extend(chain_names);
        args
    };

    let (resolved, target_processes) = resolve_service_targets(&combined_args, &entries);

    if resolved.is_empty() {
        eprintln!("no services to start");
        std::process::exit(1);
    }

    let _ = (chains, wait_for_ready, force, start_all);
    let response = response_from_ops(plist_sync::start_services(&resolved, &target_processes));

    if plain {
        handle_action_response(&response);
        return;
    }

    match response {
        Response::Ok { message } => {
            if let Some(msg) = message {
                for line in msg.lines() {
                    println!("{}", line);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));

            if !watch.enabled && !watch.no_watch && io::stdout().is_terminal() {
                watch.enabled = true;
                watch.duration = Some(4);
            }
            watch.mode = WatchMode::Start;
            if watch.enabled {
                let mut status_args = resolved.clone();
                if detailed {
                    status_args.push("--detailed".to_string());
                }
                let success = watch_status(&status_args, &watch);
                if !success {
                    std::process::exit(1);
                }
            }
        }
        Response::Error { message } => {
            eprintln!("error: {}", message);
            std::process::exit(1);
        }
        _ => {}
    }
}

fn cmd_stop(args: &[String]) {
    let (mut watch, rest) = parse_watch_opts(args, Some(4));
    let entries = config::load_service_entries();
    let plain = output_format().is_plain();

    let stop_all = rest.iter().any(|a| is_all_flag(a));
    let detailed = rest.iter().any(|a| is_detailed_flag(a));
    let rest: Vec<String> = rest
        .into_iter()
        .filter(|a| !is_all_flag(a) && !is_detailed_flag(a))
        .collect();

    let args_for_resolve: Vec<String> = if stop_all && rest.is_empty() {
        vec!["--all".to_string()]
    } else {
        rest.clone()
    };
    let (names, target_processes) = resolve_service_targets(&args_for_resolve, &entries);

    if names.is_empty() {
        eprintln!("no services to stop");
        std::process::exit(1);
    }

    let response = response_from_ops(plist_sync::stop_services(&names, &target_processes));

    if plain {
        handle_action_response(&response);
        return;
    }

    match response {
        Response::Ok { message } => {
            if let Some(msg) = message {
                for line in msg.lines() {
                    println!("{}", line);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));

            if !watch.enabled && !watch.no_watch && io::stdout().is_terminal() {
                watch.enabled = true;
                watch.duration = Some(4);
            }
            watch.mode = WatchMode::Stop;
            if watch.enabled {
                let mut status_args = names.clone();
                if detailed {
                    status_args.push("--detailed".to_string());
                }
                let success = watch_status(&status_args, &watch);
                if !success {
                    std::process::exit(1);
                }
            }
        }
        Response::Error { message } => {
            eprintln!("error: {}", message);
            std::process::exit(1);
        }
        _ => {}
    }
}

fn cmd_restart(args: &[String]) {
    let (mut watch, rest) = parse_watch_opts(args, Some(4));
    let entries = config::load_service_entries();
    let plain = output_format().is_plain();

    let restart_all = rest.iter().any(|a| is_all_flag(a));
    let detailed = rest.iter().any(|a| is_detailed_flag(a));
    let force = rest.iter().any(|a| a == "--force" || a == "-f");
    let rest: Vec<String> = rest
        .into_iter()
        .filter(|a| !is_all_flag(a) && !is_detailed_flag(a) && a != "--force" && a != "-f")
        .collect();

    if !watch.enabled && !plain && !watch.no_watch && io::stdout().is_terminal() {
        watch.enabled = true;
        watch.duration = Some(6);
    }
    watch.mode = WatchMode::Restart;

    // If --all or multiple services, do a full reload (stop+start all processes)
    if restart_all || rest.is_empty() || rest.len() > 1 {
        let (names, target_processes) = resolve_service_targets(&rest, &entries);
        if names.is_empty() {
            eprintln!("no services to restart");
            std::process::exit(1);
        }

        let _ = (restart_all, force);
        let response = response_from_ops(plist_sync::restart_services(&names, &target_processes));

        if plain {
            handle_action_response(&response);
            return;
        }

        match response {
            Response::Ok { message } => {
                if let Some(msg) = message {
                    for line in msg.lines() {
                        println!("{}", line);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                let mut status_args: Vec<String> = names.clone();
                if detailed {
                    status_args.push("--detailed".to_string());
                }
                if watch.enabled {
                    let success = watch_status(&status_args, &watch);
                    if !success {
                        std::process::exit(1);
                    }
                }
            }
            Response::Error { message } => {
                eprintln!("error: {}", message);
                std::process::exit(1);
            }
        }
        return;
    }

    // Single target: could be "service" or "service process"
    let (service, process) = resolve_single_target(&rest, &entries);

    // No process name means restart all processes in the service
    if process.is_none() {
        let _ = force;
        let response = response_from_ops(plist_sync::restart_services(
            &[service.clone()],
            &plist_sync::ProcessFilters::new(),
        ));

        if plain {
            handle_action_response(&response);
            return;
        }

        match response {
            Response::Ok { message } => {
                if let Some(msg) = message {
                    for line in msg.lines() {
                        println!("{}", line);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                let mut status_args = vec![service.clone()];
                if detailed {
                    status_args.push("--detailed".to_string());
                }
                if watch.enabled {
                    let success = watch_status(&status_args, &watch);
                    if !success {
                        std::process::exit(1);
                    }
                }
            }
            Response::Error { message } => {
                eprintln!("error: {}", message);
                std::process::exit(1);
            }
        }
        return;
    }

    let mut target_processes = plist_sync::ProcessFilters::new();
    if let Some(process) = process {
        target_processes.insert(service.clone(), vec![process]);
    }
    let response = response_from_ops(plist_sync::restart_services(
        &[service.clone()],
        &target_processes,
    ));

    if plain {
        handle_action_response(&response);
        return;
    }

    match response {
        Response::Ok { message } => {
            if let Some(msg) = message {
                println!("{}", msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            let mut status_args = vec![service.clone()];
            if detailed {
                status_args.push("--detailed".to_string());
            }
            if watch.enabled {
                let success = watch_status(&status_args, &watch);
                if !success {
                    std::process::exit(1);
                }
            }
        }
        Response::Error { message } => {
            eprintln!("error: {}", message);
            std::process::exit(1);
        }
    }
}

fn find_log_files(service: &str, _process: &Option<String>) -> Vec<PathBuf> {
    if let Some((out, err)) = plist_sync::log_paths(service) {
        let mut files = Vec::new();
        if out.exists() {
            files.push(out);
        }
        if err.exists() {
            files.push(err);
        }
        if !files.is_empty() {
            return files;
        }
    }
    // Fallback: flat log file next to plist-derived path
    let flat = logs::log_dir().join(format!("{}.log", service));
    if flat.exists() {
        return vec![flat];
    }
    Vec::new()
}

fn tail_log_lines(service: &str, process: &Option<String>, n: usize) {
    for line in tail_log_lines_string(service, process, n) {
        println!("{}", line);
    }
}

fn tail_log_lines_string(service: &str, process: &Option<String>, n: usize) -> Vec<String> {
    let files = find_log_files(service, process);
    if files.is_empty() {
        return Vec::new();
    }
    let latest = files.last().unwrap();
    let content = std::fs::read_to_string(latest).unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    let start = if lines.len() > n { lines.len() - n } else { 0 };
    lines[start..].iter().map(|l| l.to_string()).collect()
}

fn cmd_logs(args: &[String]) {
    let svc_entries = config::load_service_entries();
    let json = output_format() == OutputFormat::Json;

    let (service, process) = resolve_single_target(args, &svc_entries);

    let plist_paths = plist_sync::log_paths(&service);
    let files = find_log_files(&service, &process);

    if plist_paths.is_none() && files.is_empty() {
        eprintln!("no logs for {}", service);
        std::process::exit(1);
    }

    if json {
        let paths: Vec<String> = if let Some((o, e)) = &plist_paths {
            vec![o.display().to_string(), e.display().to_string()]
        } else {
            files.iter().map(|f| f.display().to_string()).collect()
        };
        format::json_value(&paths);
    } else if let Some((o, e)) = plist_paths {
        println!("{}", o.display());
        println!("{}", e.display());
    } else {
        for f in &files {
            println!("{}", f.display());
        }
    }
}

fn cmd_tail(args: &[String]) {
    eprintln!("{}: use 'ky echo' instead", "tail is deprecated".yellow());
    cmd_echo(args);
}

const DEFAULT_ECHO_TAIL: usize = 14;

fn parse_echo_opts(args: &[String]) -> (usize, Vec<String>) {
    let mut tail_lines = DEFAULT_ECHO_TAIL;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-n" | "--lines" => {
                if i + 1 < args.len() {
                    if let Ok(n) = args[i + 1].parse::<usize>() {
                        tail_lines = n;
                        i += 1;
                    }
                }
            }
            _ => rest.push(args[i].clone()),
        }
        i += 1;
    }
    (tail_lines, rest)
}

fn cmd_echo(args: &[String]) {
    let svc_entries = config::load_service_entries();
    let json = output_format() == OutputFormat::Json;

    let (tail_lines, rest) = parse_echo_opts(args);
    let (service, process) = resolve_single_target(&rest, &svc_entries);

    tail_log_lines(&service, &process, tail_lines);
    tail_files_forever(&service, &process, json);
}

fn tail_files_forever(service: &str, _process: &Option<String>, json: bool) {
    use std::io::{Read, Seek, SeekFrom};

    let paths: Vec<PathBuf> = match plist_sync::log_paths(service) {
        Some((o, e)) => vec![o, e],
        None => find_log_files(service, &None),
    };
    if paths.is_empty() {
        eprintln!("no log files for {}", service);
        std::process::exit(1);
    }

    let mut handles: Vec<(PathBuf, Option<std::fs::File>, u64)> =
        paths.into_iter().map(|p| (p, None, 0)).collect();

    loop {
        for (path, file_opt, offset) in handles.iter_mut() {
            if file_opt.is_none() {
                if let Ok(mut f) = std::fs::File::open(&*path) {
                    let end = f.seek(SeekFrom::End(0)).unwrap_or(0);
                    *offset = end;
                    *file_opt = Some(f);
                }
            }
            let Some(f) = file_opt else { continue };
            if let Ok(meta) = f.metadata() {
                if meta.len() < *offset {
                    *offset = 0;
                    let _ = f.seek(SeekFrom::Start(0));
                }
            }
            let _ = f.seek(SeekFrom::Start(*offset));
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() && !buf.is_empty() {
                *offset += buf.len() as u64;
                if json {
                    for line in buf.lines() {
                        format::json_log_line(line, *offset);
                    }
                } else {
                    print!("{}", buf);
                    let _ = io::stdout().flush();
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

fn echo_after_action(names: &[String], process: Option<String>) {
    let svc_entries = config::load_service_entries();
    let service = if names.is_empty() {
        if let Some(current) = get_current_project(&svc_entries) {
            current
        } else {
            return;
        }
    } else {
        names[0].clone()
    };

    tail_log_lines(&service, &process, DEFAULT_ECHO_TAIL);
    tail_files_forever(&service, &process, false);
}

fn echo_after_stop(names: &[String]) {
    let svc_entries = config::load_service_entries();
    let service = if names.is_empty() {
        if let Some(current) = get_current_project(&svc_entries) {
            current
        } else {
            return;
        }
    } else {
        names[0].clone()
    };

    tail_log_lines(&service, &None, DEFAULT_ECHO_TAIL);
}

fn cmd_show(args: &[String]) {
    let entries = config::load_service_entries();
    let json = output_format() == OutputFormat::Json;

    let filtered_args: Vec<String> = if args.len() >= 2 && args[1] == "show" {
        let mut new_args = vec![args[0].clone()];
        new_args.extend_from_slice(&args[2..]);
        new_args
    } else {
        args.to_vec()
    };

    let (service_name, process_name) = if filtered_args.is_empty() {
        if let Some(current) = get_current_project(&entries) {
            (current, None)
        } else {
            let projects_path = utils::config_dir().join("projects");
            if json {
                let map: BTreeMap<&String, &PathBuf> =
                    entries.iter().map(|(n, e)| (n, &e.dir)).collect();
                format::json_value(&map);
                std::process::exit(0);
            }
            eprintln!("{}", projects_path.display().to_string().dimmed());
            for (name, entry) in &entries {
                eprintln!("{}: {}", name.bold(), entry.dir.display());
            }
            std::process::exit(0);
        }
    } else {
        resolve_single_target(&filtered_args, &entries)
    };

    let service_entry = match entries.get(&service_name) {
        Some(entry) => entry,
        None => {
            eprintln!("unknown service: {}", service_name);
            std::process::exit(1);
        }
    };

    let global_config = config::load_global_config();
    let service = config::load_service(service_entry, &global_config.defaults);

    if service.processes.is_empty() {
        if service_entry.inline_command.is_some() {
            eprintln!(
                "no services defined ({})",
                utils::config_dir().join("projects.toml").display()
            );
        } else {
            eprintln!(
                "no services defined ({})",
                service_entry.dir.join("services.toml").display()
            );
        }
        std::process::exit(1);
    }

    if json {
        format::json_value(&service);
        return;
    }

    if let Some(proc_name) = process_name {
        if let Some(proc) = service.processes.iter().find(|p| p.name == proc_name) {
            println!("{}", proc.command);
        } else {
            eprintln!("process '{}' not found in {}", proc_name, service_name);
            std::process::exit(1);
        }
    } else {
        if service_entry.inline_command.is_some() {
            let projects_path = utils::config_dir().join("projects.toml");
            println!("{}", projects_path.display().to_string().dimmed());
        } else {
            let services_path = service_entry.dir.join("services.toml");
            println!("{}", services_path.display().to_string().dimmed());
        }
        println!();
        for proc in &service.processes {
            let type_tag = match proc.service_type {
                ServiceType::Task => " (task)".dimmed().to_string(),
                ServiceType::Service => String::new(),
            };
            let optional = if !proc.autostart {
                " (optional)".dimmed().to_string()
            } else {
                String::new()
            };
            println!(
                "{}{}{} {}",
                proc.name.cyan(),
                type_tag,
                optional,
                proc.command.dimmed()
            );
        }
    }
}

fn cmd_cron(args: &[String]) {
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    let json = output_format() == OutputFormat::Json || args.iter().any(|a| a == "--json");

    match subcmd {
        "status" | "st" => match koku_client::fetch_status() {
            Some(jobs) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&jobs).unwrap());
                } else if jobs.is_empty() {
                    eprintln!("no cron jobs configured");
                } else {
                    let max_name = jobs.iter().map(|j| j.name.len()).max().unwrap_or(0);
                    for job in &jobs {
                        let sym = koku_client::state_symbol(&job.state);
                        let extra = match (&job.last_exit, &job.next_run) {
                            (Some(code), Some(next)) => format!("exit {}  next {}", code, next),
                            (Some(code), None) => format!("exit {}", code),
                            (None, Some(next)) => format!("next {}", next),
                            (None, None) => String::new(),
                        };
                        println!(
                            "  {} {:<width$} {}{}",
                            sym,
                            job.name,
                            job.state,
                            if extra.is_empty() {
                                String::new()
                            } else {
                                format!("  {}", extra)
                            },
                            width = max_name
                        );
                    }
                }
            }
            None => {
                eprintln!("koku daemon not running");
                std::process::exit(1);
            }
        },
        "run" => {
            let name = args.get(1).unwrap_or_else(|| {
                eprintln!("usage: ky cron run <name>");
                std::process::exit(1);
            });
            match koku_client::run_job(name) {
                Ok(msg) => {
                    if json {
                        format::json_ok(Some(msg));
                    } else {
                        eprintln!("{}", msg);
                    }
                }
                Err(e) => {
                    if json {
                        format::json_error(&e);
                    } else {
                        eprintln!("error: {}", e);
                    }
                    std::process::exit(1);
                }
            }
        }
        "pause" => {
            let name = args.get(1).unwrap_or_else(|| {
                eprintln!("usage: ky cron pause <name>");
                std::process::exit(1);
            });
            match koku_client::pause_job(name) {
                Ok(msg) => {
                    if json {
                        format::json_ok(Some(msg));
                    } else {
                        eprintln!("{}", msg);
                    }
                }
                Err(e) => {
                    if json {
                        format::json_error(&e);
                    } else {
                        eprintln!("error: {}", e);
                    }
                    std::process::exit(1);
                }
            }
        }
        "resume" => {
            let name = args.get(1).unwrap_or_else(|| {
                eprintln!("usage: ky cron resume <name>");
                std::process::exit(1);
            });
            match koku_client::resume_job(name) {
                Ok(msg) => {
                    if json {
                        format::json_ok(Some(msg));
                    } else {
                        eprintln!("{}", msg);
                    }
                }
                Err(e) => {
                    if json {
                        format::json_error(&e);
                    } else {
                        eprintln!("error: {}", e);
                    }
                    std::process::exit(1);
                }
            }
        }
        "reload" => match koku_client::reload() {
            Ok(msg) => {
                if json {
                    format::json_ok(Some(msg));
                } else {
                    eprintln!("{}", msg);
                }
            }
            Err(e) => {
                if json {
                    format::json_error(&e);
                } else {
                    eprintln!("error: {}", e);
                }
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("usage: ky cron [status|run|pause|resume|reload]");
            std::process::exit(1);
        }
    }
}

fn cmd_reload_config() {
    let entries = config::load_service_entries();
    if entries.is_empty() {
        eprintln!("no services registered");
        return;
    }
    let mut written = 0usize;
    let mut changed_services = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for (name, svc) in &entries {
        match plist_sync::sync_service(svc) {
            Ok(0) => {}
            Ok(n) => {
                written += n;
                changed_services += 1;
            }
            Err(e) => failed.push(format!("{}: {}", name, e)),
        }
    }
    let unchanged_services = entries.len() - changed_services - failed.len();
    let json = output_format() == OutputFormat::Json;
    if json {
        format::json_value(&serde_json::json!({
            "written": written,
            "unchanged": unchanged_services,
            "failed": failed,
        }));
    } else if written == 0 {
        println!("all {} service(s) up to date", entries.len());
        for msg in &failed {
            eprintln!("{}", msg);
        }
    } else {
        println!(
            "synced {} plist(s) ({} unchanged)",
            written, unchanged_services
        );
        for msg in &failed {
            eprintln!("{}", msg);
        }
    }
    if !failed.is_empty() {
        std::process::exit(1);
    }
}

fn cmd_serve(action: Option<ServeAction>) {
    match action {
        Some(ServeAction::Stop) => serve_stop(),
        Some(ServeAction::Status) => cmd_serve_status(),
        Some(ServeAction::Foreground) => {
            eprintln!("serve --foreground: HTTP server not implemented yet");
            std::process::exit(1);
        }
        Some(ServeAction::Daemon) | None => serve_install_and_start(),
    }
}

const SERVE_LABEL: &str = "serve";

fn serve_install_and_start() {
    let agents_dir = launchd::user_agents_dir();
    let _ = std::fs::create_dir_all(&agents_dir);
    let plist_path = agents_dir.join(format!("com.kagaya.{}.plist", SERVE_LABEL));
    let log_root = logs::log_dir();
    let _ = std::fs::create_dir_all(&log_root);

    let bin = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "ky".into());

    let needs_write = !plist_path.exists();
    if needs_write {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "Label".into(),
            plist::Value::String(format!("com.kagaya.{}", SERVE_LABEL)),
        );
        dict.insert(
            "ProgramArguments".into(),
            plist::Value::Array(vec![
                plist::Value::String(bin),
                plist::Value::String("serve".into()),
                plist::Value::String("--foreground".into()),
            ]),
        );
        dict.insert("RunAtLoad".into(), plist::Value::Boolean(true));
        dict.insert("KeepAlive".into(), plist::Value::Boolean(true));
        dict.insert(
            "StandardOutPath".into(),
            plist::Value::String(log_root.join("serve.log").to_string_lossy().into()),
        );
        dict.insert(
            "StandardErrorPath".into(),
            plist::Value::String(log_root.join("serve.err.log").to_string_lossy().into()),
        );
        if let Err(e) = plist::Value::Dictionary(dict).to_file_xml(&plist_path) {
            eprintln!("error writing plist: {}", e);
            std::process::exit(1);
        }
        eprintln!("serve: installed {}", plist_path.display());
    }

    if plist_sync::is_loaded(SERVE_LABEL) {
        let uid = launchd::get_uid();
        let target = format!("gui/{}/com.kagaya.{}", uid, SERVE_LABEL);
        let _ = std::process::Command::new("launchctl")
            .args(["kickstart", "-kp", &target])
            .output();
        eprintln!("serve: restarted");
    } else {
        match plist_sync::bootstrap(&plist_path) {
            Ok(()) => eprintln!("serve: started"),
            Err(e) => {
                eprintln!("serve: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn serve_stop() {
    if !plist_sync::is_loaded(SERVE_LABEL) {
        eprintln!("serve: not running");
        return;
    }
    match plist_sync::bootout(SERVE_LABEL) {
        Ok(()) => eprintln!("serve: stopped"),
        Err(e) => {
            eprintln!("serve: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_serve_status() {
    let running = plist_sync::is_loaded("serve");
    let json = output_format() == OutputFormat::Json;
    if json {
        format::json_value(&serde_json::json!({ "running": running }));
    } else if running {
        eprintln!("serve on");
    } else {
        eprintln!("serve off");
    }
}

// --- Watch support ---

#[derive(Clone, Copy, PartialEq)]
enum WatchMode {
    Observe,
    Start,
    Stop,
    Restart,
}

struct WatchOpts {
    duration: Option<u64>,
    interval: u64,
    enabled: bool,
    no_watch: bool,
    mode: WatchMode,
}

fn parse_watch_opts(args: &[String], default_duration: Option<u64>) -> (WatchOpts, Vec<String>) {
    let mut opts = WatchOpts {
        duration: None,
        interval: 1,
        enabled: false,
        no_watch: false,
        mode: WatchMode::Observe,
    };
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--watch" | "-w" => {
                opts.enabled = true;
                if i + 1 < args.len() {
                    if let Ok(n) = args[i + 1].parse::<u64>() {
                        opts.duration = Some(n);
                        i += 1;
                    }
                }
                if opts.duration.is_none() {
                    opts.duration = default_duration;
                }
            }
            "--no-watch" | "-W" => {
                opts.no_watch = true;
            }
            "--watch-interval" => {
                if i + 1 < args.len() {
                    if let Ok(n) = args[i + 1].parse::<u64>() {
                        opts.interval = n.max(1);
                        i += 1;
                    }
                }
            }
            _ => rest.push(args[i].clone()),
        }
        i += 1;
    }
    (opts, rest)
}

fn fetch_status() -> (Vec<ServiceStatus>, Option<u16>) {
    let entries = config::load_service_entries();
    let services = plist_sync::query_all(&entries);
    (services, None)
}

struct StatusData {
    sorted_filter: Vec<String>,
    status_map: std::collections::HashMap<String, ServiceStatus>,
    process_filter: Option<String>,
    max_proc_name_width: usize,
    max_svc_name_width: usize,
    show_extras: bool,
    detailed: bool,
    is_single_service: bool,
    http_port: Option<u16>,
    cron_jobs: Option<Vec<koku::JobStatus>>,
}

fn is_detailed_flag(s: &str) -> bool {
    matches!(s, "--detailed" | "-d")
}

fn gather_status_data(args: &[String]) -> StatusData {
    let (services, http_port) = fetch_status();
    let entries = config::load_service_entries();

    let show_all = args.iter().any(|a| is_all_flag(a));
    let detailed = args.iter().any(|a| is_detailed_flag(a));
    let current_project = get_current_project(&entries);

    let filtered_args: Vec<String> = args
        .iter()
        .filter(|a| !is_all_flag(a) && !is_detailed_flag(a))
        .cloned()
        .collect();

    let (filter, process_filter) = if show_all {
        (entries.keys().cloned().collect(), None)
    } else if filtered_args.is_empty() {
        let svcs = if let Some(ref current) = current_project {
            vec![current.clone()]
        } else {
            entries.keys().cloned().collect()
        };
        (svcs, None)
    } else {
        let (svcs, procs) = resolve_service_targets(&filtered_args, &entries);
        let proc_filter = procs.into_values().next().and_then(|mut p| {
            if p.is_empty() {
                None
            } else {
                Some(p.remove(0))
            }
        });
        (svcs, proc_filter)
    };

    let mut status_map: std::collections::HashMap<String, ServiceStatus> =
        std::collections::HashMap::new();
    for s in services {
        status_map.insert(s.name.clone(), s);
    }

    let fmt = output_format();

    if fmt == OutputFormat::Json {
        let filtered: Vec<ServiceStatus> = filter
            .iter()
            .filter_map(|name| status_map.get(name).cloned())
            .collect();
        let port = if show_all || (args.is_empty() && current_project.is_none()) {
            http_port
        } else {
            None
        };
        format::json_status(&filtered, port);
        std::process::exit(0);
    }

    if fmt == OutputFormat::Tsv {
        let filtered: Vec<ServiceStatus> = filter
            .iter()
            .filter_map(|name| status_map.get(name).cloned())
            .collect();
        format::tsv_status(&filtered);
        std::process::exit(0);
    }

    let mut sorted_filter = filter;
    if let Some(ref current) = current_project {
        sorted_filter.sort_by(|a, b| {
            if a == current {
                std::cmp::Ordering::Less
            } else if b == current {
                std::cmp::Ordering::Greater
            } else {
                a.cmp(b)
            }
        });
    }

    let max_proc_name_width = sorted_filter
        .iter()
        .filter_map(|name| status_map.get(name))
        .flat_map(|s| s.processes.iter().map(|p| p.name.len() + 1))
        .max()
        .unwrap_or(0);

    let max_svc_name_width = sorted_filter
        .iter()
        .map(|name| name.len())
        .max()
        .unwrap_or(0);

    let is_single_service = sorted_filter.len() == 1 && !filtered_args.is_empty() && !show_all;
    let detailed = detailed || is_single_service;
    let show_extras = show_all || (filtered_args.is_empty() && current_project.is_none());
    let cron_jobs = if show_extras {
        koku_client::fetch_status()
    } else {
        None
    };

    StatusData {
        sorted_filter,
        status_map,
        process_filter,
        max_proc_name_width,
        max_svc_name_width,
        show_extras,
        detailed,
        is_single_service,
        http_port,
        cron_jobs,
    }
}

fn render_condensed_status(args: &[String]) -> usize {
    let data = gather_status_data(args);

    if let Some(ref proc_name) = data.process_filter {
        if let Some(name) = data.sorted_filter.first() {
            if let Some(status) = data.status_map.get(name) {
                for proc in &status.processes {
                    if proc.name == *proc_name {
                        print_process_line(proc, proc.name.len() + 1);
                        return 1;
                    }
                }
                eprintln!("process '{}' not found in {}", proc_name, name);
                std::process::exit(1);
            } else {
                eprintln!("service '{}' not running", name);
                std::process::exit(1);
            }
        }
        return 0;
    }

    let mut lines = 0usize;
    use std::io::Write as _;
    use tabwriter::TabWriter;
    let mut tw = TabWriter::new(vec![]).ansi(true).minwidth(0).padding(1);

    for name in &data.sorted_filter {
        let status = data.status_map.get(name);
        let agg = status
            .map(|s| aggregate_state(s))
            .unwrap_or(AggregateState::Off);
        let sym = aggregate_symbol(agg);
        let label = aggregate_label(agg);
        let duration = status.and_then(|s| relevant_state_since(s, agg));
        let dur_colored = color_duration(&format_state_duration(duration), agg);

        let has_multi = status.map(|s| s.processes.len() > 1).unwrap_or(false);
        let mini_icons = if has_multi {
            format_condensed_mini_icons(status.unwrap())
        } else {
            String::new()
        };

        let ports_str = if let Some(status) = status {
            format_condensed_ports(status)
        } else {
            String::new()
        };

        writeln!(
            tw,
            "{}\t{}\t{}\t{}\t{}\t{}",
            sym, name, label, dur_colored, mini_icons, ports_str
        )
        .unwrap();
        lines += 1;

        if matches!(agg, AggregateState::Err | AggregateState::Degraded) {
            let tail = tail_log_lines_string(name, &None, 3);
            for tl in tail {
                // Cap width so a long crash log doesn't blow up the
                // tabwriter column widths for unrelated rows.
                let mut truncated: String = tl.chars().take(60).collect();
                if tl.chars().count() > 60 {
                    truncated.push('…');
                }
                writeln!(tw, " \t{}\t\t\t\t", truncated.dimmed()).unwrap();
                lines += 1;
            }
        }
    }

    if data.show_extras {
        writeln!(tw).unwrap();
        lines += 1;
        if let Some(port) = data.http_port {
            writeln!(
                tw,
                "{}\t{}\t{}\thttp://127.0.0.1:{}\t\t",
                "●".green(),
                "serve",
                "on".green(),
                port
            )
            .unwrap();
        } else {
            writeln!(
                tw,
                "{}\t{}\t{}\t\t\t",
                "○".dimmed(),
                "serve",
                "off".dimmed()
            )
            .unwrap();
        }
        lines += 1;

        if let Some(ref jobs) = data.cron_jobs {
            if !jobs.is_empty() {
                writeln!(tw).unwrap();
                lines += 1;
                for job in jobs {
                    let sym = koku_client::state_symbol(&job.state);
                    let state_str = job.state.to_string();
                    let (sym_colored, state_colored) = match job.state {
                        koku::JobState::Running => {
                            (sym.green().to_string(), state_str.green().to_string())
                        }
                        koku::JobState::Idle => {
                            (sym.dimmed().to_string(), state_str.dimmed().to_string())
                        }
                        koku::JobState::Paused => {
                            (sym.dimmed().to_string(), state_str.dimmed().to_string())
                        }
                        koku::JobState::Failing => {
                            (sym.yellow().to_string(), state_str.yellow().to_string())
                        }
                        koku::JobState::Stopped => {
                            (sym.red().to_string(), state_str.red().to_string())
                        }
                    };
                    writeln!(tw, "{}\t{}\t{}\t\t\t", sym_colored, job.name, state_colored).unwrap();
                    lines += 1;
                }
            }
        }
    }

    tw.flush().unwrap();
    let output = String::from_utf8(tw.into_inner().unwrap()).unwrap();
    print!("{}", output.trim_end());
    println!();

    lines
}

fn render_detailed_status(args: &[String]) -> usize {
    let data = gather_status_data(args);

    if let Some(ref proc_name) = data.process_filter {
        if let Some(name) = data.sorted_filter.first() {
            if let Some(status) = data.status_map.get(name) {
                for proc in &status.processes {
                    if proc.name == *proc_name {
                        print_process_line(proc, proc.name.len() + 1);
                        return 1;
                    }
                }
                eprintln!("process '{}' not found in {}", proc_name, name);
                std::process::exit(1);
            } else {
                eprintln!("service '{}' not running", name);
                std::process::exit(1);
            }
        }
        return 0;
    }

    let name_w = data.max_proc_name_width.max(data.max_svc_name_width);

    let mut lines = 0usize;
    for name in &data.sorted_filter {
        let status = data.status_map.get(name);
        let agg = status
            .map(|s| aggregate_state(s))
            .unwrap_or(AggregateState::Off);

        let show_procs = status
            .map(|s| s.processes.len() > 1 || data.is_single_service)
            .unwrap_or(false);

        if show_procs {
            let sym = aggregate_symbol(agg);
            println!("{} {}", sym, name.bold());
            lines += 1;

            if let Some(status) = status {
                for proc in &status.processes {
                    print_process_line(proc, name_w);
                    lines += 1;
                }
            }
        } else {
            if let Some(status) = status {
                if let Some(proc) = status.processes.first() {
                    let pcolor = process_state_color(proc);
                    let (symbol, label, extra) = match &proc.state {
                        ProcessState::Running { pid, .. } => {
                            let duration = format_state_duration(proc.state_since);
                            let ports = if proc.ports.is_empty() {
                                String::new()
                            } else {
                                format!(":{}", format_port_ranges(&proc.ports).replace(", ", ",:"))
                            };
                            let extra = format!(
                                "{:<8} {:<8} {}",
                                color_duration(&duration, pcolor),
                                pid,
                                ports
                            );
                            ("●".green().to_string(), "on".green().to_string(), extra)
                        }
                        ProcessState::Stopped if !proc.autostart => {
                            let duration = format_state_duration(proc.state_since);
                            let extra = if duration.is_empty() {
                                String::new()
                            } else {
                                color_duration(&duration, pcolor)
                            };
                            ("○".dimmed().to_string(), "off".dimmed().to_string(), extra)
                        }
                        ProcessState::Stopped => {
                            let duration = format_state_duration(proc.state_since);
                            let extra = if duration.is_empty() {
                                String::new()
                            } else {
                                color_duration(&duration, pcolor)
                            };
                            ("◻".dimmed().to_string(), "off".dimmed().to_string(), extra)
                        }
                        ProcessState::Crashed { exit_code, retries } => {
                            let duration = format_state_duration(proc.state_since);
                            let dur_str = if duration.is_empty() {
                                String::new()
                            } else {
                                format!("{}  ", color_duration(&duration, pcolor))
                            };
                            let extra = format!("{}exit {}  retry {}", dur_str, exit_code, retries);
                            (
                                "⚠".yellow().to_string(),
                                "crashed".yellow().to_string(),
                                extra,
                            )
                        }
                        ProcessState::Failed { exit_code } => {
                            let duration = format_state_duration(proc.state_since);
                            let dur_str = if duration.is_empty() {
                                String::new()
                            } else {
                                format!("{}  ", color_duration(&duration, pcolor))
                            };
                            let extra = format!("{}exit {}", dur_str, exit_code);
                            ("✖".red().to_string(), "failed".red().to_string(), extra)
                        }
                    };
                    let extra_str = if extra.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", extra.trim_end())
                    };
                    println!("{} {:<w$} {}{}", symbol, name, label, extra_str, w = name_w);
                    lines += 1;
                }
            } else {
                println!(
                    "{} {:<w$} {}",
                    "◻".dimmed(),
                    name,
                    "off".dimmed(),
                    w = name_w
                );
                lines += 1;
            }
        }
    }

    if data.show_extras {
        println!();
        lines += 1;
        if let Some(port) = data.http_port {
            println!(
                "{} {:<w$} {}  http://127.0.0.1:{}",
                "●".green(),
                "serve",
                "on".green(),
                port,
                w = name_w
            );
        } else {
            println!(
                "{} {:<w$} {}",
                "○".dimmed(),
                "serve",
                "off".dimmed(),
                w = name_w
            );
        }
        lines += 1;

        if let Some(ref jobs) = data.cron_jobs {
            if !jobs.is_empty() {
                println!();
                lines += 1;

                let has_running = jobs.iter().any(|j| j.state == koku::JobState::Running);
                let symbol = if has_running {
                    "●".green().to_string()
                } else {
                    "○".dimmed().to_string()
                };
                println!("{} {}", symbol, "cron".bold());
                lines += 1;

                let max_name = jobs
                    .iter()
                    .map(|j| j.name.len())
                    .max()
                    .unwrap_or(0)
                    .max(name_w);

                for job in jobs {
                    let sym = koku_client::state_symbol(&job.state);
                    let state_str = job.state.to_string();
                    let (sym_colored, state_colored) = match job.state {
                        koku::JobState::Running => {
                            (sym.green().to_string(), state_str.green().to_string())
                        }
                        koku::JobState::Idle => {
                            (sym.dimmed().to_string(), state_str.dimmed().to_string())
                        }
                        koku::JobState::Paused => {
                            (sym.dimmed().to_string(), state_str.dimmed().to_string())
                        }
                        koku::JobState::Failing => {
                            (sym.yellow().to_string(), state_str.yellow().to_string())
                        }
                        koku::JobState::Stopped => {
                            (sym.red().to_string(), state_str.red().to_string())
                        }
                    };

                    let extra = match (&job.last_exit, &job.next_run) {
                        (Some(code), Some(next)) => format!("exit {}  next {}", code, next),
                        (Some(code), None) => format!("exit {}", code),
                        (None, Some(next)) => format!("next {}", next),
                        (None, None) => String::new(),
                    };

                    let extra_str = if extra.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", extra)
                    };
                    println!(
                        "  {} {:<width$} {}{}",
                        sym_colored,
                        job.name,
                        state_colored,
                        extra_str,
                        width = max_name
                    );
                    lines += 1;
                }
            }
        }
    }

    lines
}

// --- Ratatui watch mode ---

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as RLine, Span};
use ratatui::widgets::Paragraph;

fn process_state_style(proc: &ProcessStatus) -> Style {
    match &proc.state {
        ProcessState::Running { .. } => Style::default().fg(Color::Green),
        ProcessState::Stopped => Style::default().add_modifier(Modifier::DIM),
        ProcessState::Crashed { .. } => Style::default().fg(Color::Yellow),
        ProcessState::Failed { .. } => Style::default().fg(Color::Red),
    }
}

fn is_port_pending(proc: &ProcessStatus) -> bool {
    if !proc.state.is_running() || proc.ports_expected.is_empty() {
        return false;
    }
    proc.ports_expected.iter().any(|p| !proc.ports.contains(p))
}

fn process_line_spans<'a>(proc: &ProcessStatus, name_width: usize) -> RLine<'a> {
    let green = Style::default().fg(Color::Green);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let yellow = Style::default().fg(Color::Yellow);
    let red = Style::default().fg(Color::Red);
    let cyan = Style::default().fg(Color::Cyan);
    let state_style = process_state_style(proc);

    let dur_str = format_state_duration(proc.state_since);

    let (symbol, label, extra) = match &proc.state {
        ProcessState::Running { .. } if is_port_pending(proc) => {
            let extra = format!("{:<8}", dur_str);
            (
                Span::styled("◌", cyan),
                Span::styled("starting", cyan),
                extra,
            )
        }
        ProcessState::Running { .. } => {
            let ports = format_port_ranges(&proc.ports);
            let extra = format!("{:<8} {}", dur_str, ports);
            (Span::styled("•", green), Span::styled("on", green), extra)
        }
        ProcessState::Stopped if !proc.autostart => (
            Span::styled("◦", dim),
            Span::styled("optional", dim),
            dur_str.clone(),
        ),
        ProcessState::Stopped => (
            Span::styled("◦", dim),
            Span::styled("off", dim),
            dur_str.clone(),
        ),
        ProcessState::Crashed { exit_code, retries } => {
            let dur_prefix = if dur_str.is_empty() {
                String::new()
            } else {
                format!("{}  ", dur_str)
            };
            let extra = format!("{}exit {}  retry {}", dur_prefix, exit_code, retries);
            (
                Span::styled("⚠", yellow),
                Span::styled("crashed", yellow),
                extra,
            )
        }
        ProcessState::Failed { exit_code } => {
            let dur_prefix = if dur_str.is_empty() {
                String::new()
            } else {
                format!("{}  ", dur_str)
            };
            let extra = format!("{}exit {}", dur_prefix, exit_code);
            (Span::styled("✖", red), Span::styled("failed", red), extra)
        }
    };
    let dotname = format!(".{}", proc.name);
    let padded_name = format!("{:<width$}", dotname, width = name_width);

    let mut spans = vec![
        symbol,
        Span::raw(" "),
        Span::raw(padded_name),
        Span::raw(" "),
        label,
    ];
    if !extra.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(extra.trim_end().to_string(), state_style));
    }
    RLine::from(spans)
}

fn aggregate_state_style(agg: AggregateState) -> Style {
    match agg {
        AggregateState::On => Style::default().fg(Color::Green),
        AggregateState::Degraded => Style::default().fg(Color::Yellow),
        AggregateState::Err => Style::default().fg(Color::Red),
        AggregateState::Off => Style::default().add_modifier(Modifier::DIM),
    }
}

fn aggregate_symbol_span(agg: AggregateState) -> Span<'static> {
    let style = aggregate_state_style(agg);
    match agg {
        AggregateState::On => Span::styled("●", style),
        AggregateState::Degraded => Span::styled("⚠", style),
        AggregateState::Err => Span::styled("✖", style),
        AggregateState::Off => Span::styled("◻", style),
    }
}

fn aggregate_label_span(agg: AggregateState) -> Span<'static> {
    let style = aggregate_state_style(agg);
    match agg {
        AggregateState::On => Span::styled("on", style),
        AggregateState::Degraded => Span::styled("deg", style),
        AggregateState::Err => Span::styled("err", style),
        AggregateState::Off => Span::styled("off", style),
    }
}

fn process_mini_icon_span(proc: &ProcessStatus) -> Span<'static> {
    let green = Style::default().fg(Color::Green);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let yellow = Style::default().fg(Color::Yellow);
    let red = Style::default().fg(Color::Red);
    let cyan = Style::default().fg(Color::Cyan);
    match &proc.state {
        ProcessState::Running { .. } if is_port_pending(proc) => Span::styled("◌", cyan),
        ProcessState::Running { .. } => Span::styled("•", green),
        ProcessState::Stopped => Span::styled("◦", dim),
        ProcessState::Crashed { .. } => Span::styled("⚠", yellow),
        ProcessState::Failed { .. } => Span::styled("✖", red),
    }
}

fn status_data_to_lines<'a>(data: &StatusData) -> Vec<RLine<'a>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let green = Style::default().fg(Color::Green);
    let dim = Style::default().add_modifier(Modifier::DIM);

    if let Some(ref proc_name) = data.process_filter {
        for name in &data.sorted_filter {
            if let Some(status) = data.status_map.get(name) {
                for proc in &status.processes {
                    if proc.name == *proc_name {
                        return vec![process_line_spans(proc, proc.name.len() + 1)];
                    }
                }
            }
        }
        return vec![];
    }

    let mut lines: Vec<RLine> = Vec::new();

    if data.detailed {
        let name_w = data.max_proc_name_width.max(data.max_svc_name_width);

        for name in &data.sorted_filter {
            let status = data.status_map.get(name);
            let agg = status
                .map(|s| aggregate_state(s))
                .unwrap_or(AggregateState::Off);
            let show_procs = status
                .map(|s| s.processes.len() > 1 || data.is_single_service)
                .unwrap_or(false);

            if show_procs {
                let sym = aggregate_symbol_span(agg);
                lines.push(RLine::from(vec![
                    sym,
                    Span::raw(" "),
                    Span::styled(name.clone(), bold),
                ]));
                if let Some(status) = status {
                    for proc in &status.processes {
                        lines.push(process_line_spans(proc, name_w));
                    }
                }
            } else if let Some(status) = status {
                if let Some(proc) = status.processes.first() {
                    let pstyle = process_state_style(proc);
                    let dur_str = format_state_duration(proc.state_since);
                    let cyan = Style::default().fg(Color::Cyan);
                    let (symbol, label, extra) = match &proc.state {
                        ProcessState::Running { .. } if is_port_pending(proc) => {
                            let extra = format!("{:<8}", dur_str);
                            (
                                Span::styled("◌", cyan),
                                Span::styled("starting", cyan),
                                extra,
                            )
                        }
                        ProcessState::Running { .. } => {
                            let ports = format_port_ranges(&proc.ports);
                            let extra = format!("{:<8} {}", dur_str, ports);
                            (Span::styled("●", green), Span::styled("on", green), extra)
                        }
                        ProcessState::Stopped if !proc.autostart => (
                            Span::styled("○", dim),
                            Span::styled("off", dim),
                            dur_str.clone(),
                        ),
                        ProcessState::Stopped => (
                            Span::styled("◻", dim),
                            Span::styled("off", dim),
                            dur_str.clone(),
                        ),
                        ProcessState::Crashed { exit_code, retries } => {
                            let dur_prefix = if dur_str.is_empty() {
                                String::new()
                            } else {
                                format!("{}  ", dur_str)
                            };
                            let extra =
                                format!("{}exit {}  retry {}", dur_prefix, exit_code, retries);
                            let y = Style::default().fg(Color::Yellow);
                            (Span::styled("⚠", y), Span::styled("crashed", y), extra)
                        }
                        ProcessState::Failed { exit_code } => {
                            let dur_prefix = if dur_str.is_empty() {
                                String::new()
                            } else {
                                format!("{}  ", dur_str)
                            };
                            let extra = format!("{}exit {}", dur_prefix, exit_code);
                            let r = Style::default().fg(Color::Red);
                            (Span::styled("✖", r), Span::styled("failed", r), extra)
                        }
                    };
                    let padded = format!("{:<w$}", name, w = name_w);
                    let mut spans = vec![
                        symbol,
                        Span::raw(" "),
                        Span::raw(padded),
                        Span::raw(" "),
                        label,
                    ];
                    if !extra.is_empty() {
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(extra.trim_end().to_string(), pstyle));
                    }
                    lines.push(RLine::from(spans));
                }
            } else {
                let padded = format!("{:<w$}", name, w = name_w);
                lines.push(RLine::from(vec![
                    Span::styled("◻", dim),
                    Span::raw(" "),
                    Span::raw(padded),
                    Span::raw(" "),
                    Span::styled("off", dim),
                ]));
            }
        }
    } else {
        let name_w = data.max_svc_name_width;

        for name in &data.sorted_filter {
            let status = data.status_map.get(name);
            let agg = status
                .map(|s| aggregate_state(s))
                .unwrap_or(AggregateState::Off);
            let agg_style = aggregate_state_style(agg);
            let sym = aggregate_symbol_span(agg);
            let label = aggregate_label_span(agg);

            let duration = status.and_then(|s| relevant_state_since(s, agg));
            let dur_str = format_state_duration(duration);

            let has_multi = status.map(|s| s.processes.len() > 1).unwrap_or(false);

            let ports_str: String = if let Some(status) = status {
                let all_ports: Vec<u16> = status
                    .processes
                    .iter()
                    .filter(|p| p.state.is_running())
                    .flat_map(|p| p.ports.iter().copied())
                    .collect();
                format_port_ranges(&all_ports)
            } else {
                String::new()
            };

            let padded = format!("{:<w$}", name, w = name_w);
            let mut spans: Vec<Span> = vec![
                sym,
                Span::raw(" "),
                Span::raw(padded),
                Span::raw("  "),
                label,
            ];

            if !dur_str.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(format!("{:>7}", dur_str), agg_style));
            } else if has_multi || !ports_str.is_empty() {
                spans.push(Span::raw(format!("  {:>7}", "")));
            }

            if has_multi {
                spans.push(Span::raw(" "));
                if let Some(status) = status {
                    for proc in &status.processes {
                        spans.push(process_mini_icon_span(proc));
                    }
                }
            }

            if !ports_str.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::raw(ports_str));
            }

            lines.push(RLine::from(spans));
        }
    }

    if data.show_extras {
        lines.push(RLine::from(""));
        if data.detailed {
            let name_w = data.max_proc_name_width.max(data.max_svc_name_width);
            if let Some(port) = data.http_port {
                let padded = format!("{:<w$}", "serve", w = name_w);
                lines.push(RLine::from(vec![
                    Span::styled("●", green),
                    Span::raw(" "),
                    Span::raw(padded),
                    Span::raw(" "),
                    Span::styled("on", green),
                    Span::raw(format!("  http://127.0.0.1:{}", port)),
                ]));
            } else {
                let padded = format!("{:<w$}", "serve", w = name_w);
                lines.push(RLine::from(vec![
                    Span::styled("○", dim),
                    Span::raw(" "),
                    Span::raw(padded),
                    Span::raw(" "),
                    Span::styled("off", dim),
                ]));
            }
        } else {
            let name_w = data.max_svc_name_width;
            if let Some(port) = data.http_port {
                let padded = format!("{:<w$}", "serve", w = name_w);
                lines.push(RLine::from(vec![
                    Span::styled("●", green),
                    Span::raw(" "),
                    Span::raw(padded),
                    Span::raw("  "),
                    Span::styled("on", green),
                    Span::raw(format!("  http://127.0.0.1:{}", port)),
                ]));
            } else {
                let padded = format!("{:<w$}", "serve", w = name_w);
                lines.push(RLine::from(vec![
                    Span::styled("○", dim),
                    Span::raw(" "),
                    Span::raw(padded),
                    Span::raw("  "),
                    Span::styled("off", dim),
                ]));
            }
        }

        if let Some(ref jobs) = data.cron_jobs {
            if !jobs.is_empty() {
                lines.push(RLine::from(""));
                let has_running = jobs.iter().any(|j| j.state == koku::JobState::Running);
                let cron_sym = if has_running {
                    Span::styled("●", green)
                } else {
                    Span::styled("○", dim)
                };
                lines.push(RLine::from(vec![
                    cron_sym,
                    Span::raw(" "),
                    Span::styled("cron", bold),
                ]));

                let max_name = jobs.iter().map(|j| j.name.len()).max().unwrap_or(0);
                for job in jobs {
                    let sym = koku_client::state_symbol(&job.state);
                    let state_str = job.state.to_string();
                    let (sym_style, state_style) = match job.state {
                        koku::JobState::Running => (green, green),
                        koku::JobState::Idle | koku::JobState::Paused => (dim, dim),
                        koku::JobState::Failing => {
                            let y = Style::default().fg(Color::Yellow);
                            (y, y)
                        }
                        koku::JobState::Stopped => {
                            let r = Style::default().fg(Color::Red);
                            (r, r)
                        }
                    };
                    let extra = match (&job.last_exit, &job.next_run) {
                        (Some(code), Some(next)) => format!("exit {}  next {}", code, next),
                        (Some(code), None) => format!("exit {}", code),
                        (None, Some(next)) => format!("next {}", next),
                        (None, None) => String::new(),
                    };
                    let extra_str = if extra.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", extra)
                    };
                    let padded_name = format!("{:<width$}", job.name, width = max_name);
                    lines.push(RLine::from(vec![
                        Span::raw("  "),
                        Span::styled(sym.to_string(), sym_style),
                        Span::raw(" "),
                        Span::raw(padded_name),
                        Span::raw(" "),
                        Span::styled(state_str, state_style),
                        Span::raw(extra_str),
                    ]));
                }
            }
        }
    }

    lines
}

fn build_status_lines<'a>(args: &[String]) -> (Vec<RLine<'a>>, StatusData) {
    let data = gather_status_data(args);
    let lines = status_data_to_lines(&data);
    (lines, data)
}

fn watch_status_satisfied(data: &StatusData, mode: WatchMode) -> bool {
    match mode {
        WatchMode::Stop => data.sorted_filter.iter().all(|name| {
            data.status_map.get(name).map_or(true, |s| {
                s.processes
                    .iter()
                    .all(|p| matches!(p.state, ProcessState::Stopped))
            })
        }),
        WatchMode::Start | WatchMode::Restart => data.sorted_filter.iter().all(|name| {
            data.status_map.get(name).map_or(false, |s| {
                s.processes.iter().all(|p| match &p.state {
                    ProcessState::Running { .. } => !is_port_pending(p),
                    ProcessState::Stopped if !p.autostart => true,
                    _ => false,
                })
            })
        }),
        WatchMode::Observe => false,
    }
}

fn watch_status_ok(data: &StatusData, mode: WatchMode) -> bool {
    match mode {
        WatchMode::Stop => data.sorted_filter.iter().all(|name| {
            data.status_map.get(name).map_or(true, |s| {
                s.processes
                    .iter()
                    .all(|p| matches!(p.state, ProcessState::Stopped))
            })
        }),
        WatchMode::Start | WatchMode::Restart => data.sorted_filter.iter().all(|name| {
            data.status_map.get(name).map_or(false, |s| {
                s.processes.iter().all(|p| match &p.state {
                    ProcessState::Running { .. } => true,
                    ProcessState::Stopped if !p.autostart => true,
                    _ => false,
                })
            })
        }),
        WatchMode::Observe => true,
    }
}

fn failed_processes(data: &StatusData) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for name in &data.sorted_filter {
        if let Some(status) = data.status_map.get(name) {
            for proc in &status.processes {
                if matches!(
                    proc.state,
                    ProcessState::Crashed { .. } | ProcessState::Failed { .. }
                ) {
                    result.push((name.clone(), proc.name.clone()));
                }
            }
        }
    }
    result
}

fn tail_log_lines_spans<'a>(service: &str, process: &str, n: usize) -> Vec<RLine<'a>> {
    let proc_filter = Some(process.to_string());
    let files = find_log_files(service, &proc_filter);
    if files.is_empty() {
        return vec![];
    }
    let latest = files.last().unwrap();
    let content = std::fs::read_to_string(latest).unwrap_or_default();
    let file_lines: Vec<&str> = content.lines().collect();
    let start = if file_lines.len() > n {
        file_lines.len() - n
    } else {
        0
    };

    let dim = Style::default().add_modifier(Modifier::DIM);
    let mut lines = Vec::new();

    let header = format!("── {}.{} ", service, process);
    let pad = if header.len() < 50 {
        "─".repeat(50 - header.len())
    } else {
        String::new()
    };
    lines.push(RLine::from(Span::styled(format!("{}{}", header, pad), dim)));

    for line in &file_lines[start..] {
        lines.push(RLine::from(Span::styled(line.to_string(), dim)));
    }
    lines
}

type PrevStates = std::collections::HashMap<(String, String), ProcessState>;

fn detect_transitions(
    data: &StatusData,
    prev: &PrevStates,
) -> std::collections::HashMap<(String, String), &'static str> {
    let mut transitions = std::collections::HashMap::new();
    for name in &data.sorted_filter {
        if let Some(status) = data.status_map.get(name) {
            for proc in &status.processes {
                let key = (name.clone(), proc.name.clone());
                if let Some(prev_state) = prev.get(&key) {
                    match (&proc.state, prev_state) {
                        (
                            ProcessState::Crashed { .. } | ProcessState::Failed { .. },
                            ProcessState::Running { .. },
                        ) => {
                            transitions.insert(key, " (just crashed)");
                        }
                        (
                            ProcessState::Running { .. },
                            ProcessState::Stopped
                            | ProcessState::Crashed { .. }
                            | ProcessState::Failed { .. },
                        ) => {
                            transitions.insert(key, " (just started)");
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    transitions
}

fn snapshot_states(data: &StatusData) -> PrevStates {
    let mut map = PrevStates::new();
    for name in &data.sorted_filter {
        if let Some(status) = data.status_map.get(name) {
            for proc in &status.processes {
                map.insert((name.clone(), proc.name.clone()), proc.state.clone());
            }
        }
    }
    map
}

fn watch_status(args: &[String], opts: &WatchOpts) -> bool {
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use crossterm::terminal;
    use ratatui::backend::CrosstermBackend;
    use ratatui::{Terminal, TerminalOptions, Viewport};
    use std::time::Duration;

    if !io::stdout().is_terminal() {
        return true;
    }

    let start = Instant::now();
    let mut prev_states: PrevStates = PrevStates::new();
    let mut satisfied_since: Option<Instant> = None;

    let (mut initial_lines, initial_data) = build_status_lines(args);
    let failed = failed_processes(&initial_data);
    if !failed.is_empty() {
        initial_lines.push(RLine::from(""));
        for (svc, proc) in &failed {
            initial_lines.extend(tail_log_lines_spans(svc, proc, 10));
        }
    }
    let viewport_height = (initial_lines.len() as u16).max(1);

    println!();
    terminal::enable_raw_mode().unwrap();
    let backend = CrosstermBackend::new(io::stdout());
    let mut term = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(viewport_height),
        },
    )
    .unwrap();

    let mut last_data: Option<StatusData> = None;

    loop {
        let (mut lines, data) = build_status_lines(args);

        let transitions = detect_transitions(&data, &prev_states);
        if !transitions.is_empty() {
            annotate_transitions(&mut lines, &transitions);
        }

        let failed = failed_processes(&data);
        if !failed.is_empty() {
            lines.push(RLine::from(""));
            for (svc, proc) in &failed {
                lines.extend(tail_log_lines_spans(svc, proc, 10));
            }
        }

        lines.truncate(viewport_height as usize);
        while (lines.len() as u16) < viewport_height {
            lines.push(RLine::from(""));
        }

        term.draw(|frame| {
            let text = ratatui::text::Text::from(lines);
            frame.render_widget(Paragraph::new(text), frame.area());
        })
        .unwrap();

        if opts.mode != WatchMode::Observe {
            let satisfied = watch_status_satisfied(&data, opts.mode);
            if satisfied {
                if satisfied_since.is_none() {
                    satisfied_since = Some(Instant::now());
                }
                if opts.mode == WatchMode::Stop {
                    break;
                }
                if opts.mode == WatchMode::Restart {
                    if let Some(since) = satisfied_since {
                        if since.elapsed().as_secs() >= 2 {
                            break;
                        }
                    }
                }
            } else {
                satisfied_since = None;
            }
        }

        if let Some(duration) = opts.duration {
            if start.elapsed().as_secs() >= duration {
                break;
            }
        }

        prev_states = snapshot_states(&data);
        last_data = Some(data);

        let poll_interval = if start.elapsed().as_secs() < 2 {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(opts.interval)
        };

        if event::poll(poll_interval).unwrap() {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }
    }

    terminal::disable_raw_mode().unwrap();

    last_data.map_or(true, |data| watch_status_ok(&data, opts.mode))
}

fn annotate_transitions(
    lines: &mut Vec<RLine<'_>>,
    transitions: &std::collections::HashMap<(String, String), &'static str>,
) {
    for line in lines.iter_mut() {
        let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        for ((_, proc_name), annotation) in transitions {
            let dot_name = format!(".{}", proc_name);
            if line_text.contains(&dot_name) {
                let style = if annotation.contains("crashed") {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                };
                line.spans.push(Span::styled(*annotation, style));
                break;
            }
        }
    }
}

// --- Formatting helpers ---

use utils::format_uptime;

fn parse_dot_target(name: &str) -> (&str, Option<&str>) {
    if let Some(dot) = name.find('.') {
        (&name[..dot], Some(&name[dot + 1..]))
    } else {
        (name, None)
    }
}

fn resolve_dot_target(
    name: &str,
    entries: &BTreeMap<String, ServiceEntry>,
) -> (String, Option<String>) {
    let (svc, proc) = parse_dot_target(name);
    if svc.is_empty() {
        if let Some(current) = get_current_project(entries) {
            (current, proc.map(|s| s.to_string()))
        } else {
            eprintln!("not in a registered project directory; use service.process syntax");
            std::process::exit(1);
        }
    } else {
        (svc.to_string(), proc.map(|s| s.to_string()))
    }
}

// --- Target resolution ---

fn is_all_flag(s: &str) -> bool {
    matches!(s, "--all" | "-a" | "all")
}

fn get_current_project(entries: &BTreeMap<String, ServiceEntry>) -> Option<String> {
    if let Ok(cwd) = std::env::current_dir() {
        let cwd = cwd.canonicalize().unwrap_or(cwd);
        for (name, entry) in entries {
            let entry_dir = entry.dir.canonicalize().unwrap_or(entry.dir.clone());
            if cwd == entry_dir {
                return Some(name.clone());
            }
        }
    }
    None
}

/// Resolve a list of CLI args into service names and per-service process filters.
/// Handles: dot notation, known service names, bare process names via CWD, --all.
/// If args is empty, falls back to CWD project or errors.
fn resolve_service_targets(
    args: &[String],
    entries: &BTreeMap<String, ServiceEntry>,
) -> (Vec<String>, plist_sync::ProcessFilters) {
    if args.is_empty() {
        if let Some(current) = get_current_project(entries) {
            return (vec![current], plist_sync::ProcessFilters::new());
        }
        eprintln!("no service specified and not in a registered project directory");
        eprintln!("use --all to target all services, or specify a name");
        if !entries.is_empty() {
            let names: Vec<&str> = entries.keys().map(|s| s.as_str()).collect();
            eprintln!("registered: {}", names.join(", "));
        }
        std::process::exit(1);
    }

    if args.len() == 1 && is_all_flag(&args[0]) {
        return (
            entries.keys().cloned().collect(),
            plist_sync::ProcessFilters::new(),
        );
    }

    let mut service_names: Vec<String> = Vec::new();
    let mut process_filters = plist_sync::ProcessFilters::new();

    for arg in args {
        if is_all_flag(arg) {
            continue;
        }
        let (svc, proc) = resolve_dot_target(arg, entries);
        if let Some(p) = proc {
            if !service_names.contains(&svc) {
                service_names.push(svc.clone());
            }
            let filters = process_filters.entry(svc).or_default();
            if !filters.contains(&p) {
                filters.push(p);
            }
        } else if entries.contains_key(&svc) {
            if !service_names.contains(&svc) {
                service_names.push(svc);
            }
        } else if let Some(current) = get_current_project(entries) {
            if !service_names.contains(&current) {
                service_names.push(current.clone());
            }
            let filters = process_filters.entry(current).or_default();
            if !filters.contains(&svc) {
                filters.push(svc);
            }
        } else {
            eprintln!("unknown service: {}", svc);
            eprintln!(
                "registered services: {}",
                entries.keys().cloned().collect::<Vec<_>>().join(", ")
            );
            std::process::exit(1);
        }
    }

    (service_names, process_filters)
}

/// Resolve CLI args to a single (service, Option<process>) target.
/// Second positional arg is treated as a process name fallback.
fn resolve_single_target(
    args: &[String],
    entries: &BTreeMap<String, ServiceEntry>,
) -> (String, Option<String>) {
    if args.is_empty() {
        if let Some(current) = get_current_project(entries) {
            return (current, None);
        }
        eprintln!("not in a registered project directory; specify a service or service.process");
        std::process::exit(1);
    }

    let (svc, proc) = resolve_dot_target(&args[0], entries);
    let process = proc.or_else(|| args.get(1).cloned());

    if process.is_some() {
        return (svc, process);
    }

    if entries.contains_key(&svc) {
        return (svc, None);
    }

    if let Some(current) = get_current_project(entries) {
        return (current, Some(svc));
    }

    eprintln!("unknown service: {}", svc);
    eprintln!(
        "registered services: {}",
        entries.keys().cloned().collect::<Vec<_>>().join(", ")
    );
    std::process::exit(1);
}

fn check_alias_hint() {
    if command_exists("lctl") {
        return;
    }

    let shell = detect_shell();
    let rc_file = shell_rc_file(&shell);

    eprintln!();
    eprintln!("tip: add to {}:", rc_file);
    eprintln!("  alias lctl='ky launchd'");
}

fn detect_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if let Some(name) = shell.rsplit('/').next() {
            return name.to_string();
        }
    }
    "bash".to_string()
}

fn shell_rc_file(shell: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
    match shell {
        "zsh" => format!("{}/.zshrc", home),
        "fish" => format!("{}/.config/fish/config.fish", home),
        "bash" => format!("{}/.bashrc", home),
        _ => format!("~/.{}rc", shell),
    }
}

fn command_exists(name: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {} >/dev/null 2>&1", name)])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::Span;

    fn test_proc(name: &str, state: ProcessState) -> ProcessStatus {
        ProcessStatus {
            name: name.to_string(),
            state,
            pid: None,
            autostart: true,
            service_type: ServiceType::Service,
            ports: vec![],
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        }
    }

    fn test_service(name: &str, processes: Vec<ProcessStatus>) -> ServiceStatus {
        ServiceStatus {
            name: name.to_string(),
            dir: "/tmp".into(),
            processes,
        }
    }

    fn test_status_data(services: Vec<ServiceStatus>) -> StatusData {
        let names: Vec<String> = services.iter().map(|s| s.name.clone()).collect();
        let max_w = services
            .iter()
            .flat_map(|s| s.processes.iter().map(|p| p.name.len() + 1))
            .max()
            .unwrap_or(0);
        let max_svc = names.iter().map(|n| n.len()).max().unwrap_or(0);
        let mut map = std::collections::HashMap::new();
        for s in services {
            map.insert(s.name.clone(), s);
        }
        StatusData {
            sorted_filter: names,
            status_map: map,
            process_filter: None,
            max_proc_name_width: max_w,
            max_svc_name_width: max_svc,
            show_extras: false,
            detailed: false,
            is_single_service: false,
            http_port: None,
            cron_jobs: None,
        }
    }

    fn test_entries(names: &[&str]) -> BTreeMap<String, ServiceEntry> {
        names
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    ServiceEntry {
                        name: name.to_string(),
                        dir: PathBuf::from("/tmp").join(name),
                        inline_command: None,
                        autostart: false,
                        depends_on: vec![],
                        urls: vec![],
                    },
                )
            })
            .collect()
    }

    fn span_text(line: &ratatui::text::Line) -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    }

    fn find_span<'a>(line: &'a ratatui::text::Line, content: &str) -> Option<&'a Span<'a>> {
        line.spans.iter().find(|s| s.content.as_ref() == content)
    }

    #[test]
    fn resolve_dot_target_scopes_process_to_service() {
        let entries = test_entries(&["jobs"]);
        let (services, processes) = resolve_service_targets(&["jobs.ui".to_string()], &entries);

        assert_eq!(services, vec!["jobs"]);
        assert_eq!(processes.get("jobs"), Some(&vec!["ui".to_string()]));
    }

    #[test]
    fn resolve_process_filters_do_not_cross_services() {
        let entries = test_entries(&["jobs", "admin"]);
        let args = vec!["jobs.ui".to_string(), "admin.worker".to_string()];
        let (services, processes) = resolve_service_targets(&args, &entries);

        assert_eq!(services, vec!["jobs", "admin"]);
        assert_eq!(processes.get("jobs"), Some(&vec!["ui".to_string()]));
        assert_eq!(processes.get("admin"), Some(&vec!["worker".to_string()]));
    }

    // --- format_uptime ---

    #[test]
    fn uptime_seconds() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(5), "5s");
        assert_eq!(format_uptime(59), "59s");
    }

    #[test]
    fn uptime_minutes() {
        assert_eq!(format_uptime(60), "1m");
        assert_eq!(format_uptime(192), "3m 12s");
        assert_eq!(format_uptime(300), "5m");
        assert_eq!(format_uptime(3599), "59m 59s");
    }

    #[test]
    fn uptime_hours() {
        assert_eq!(format_uptime(3600), "1h");
        assert_eq!(format_uptime(9000), "2h 30m");
        assert_eq!(format_uptime(86399), "23h 59m");
    }

    #[test]
    fn uptime_days() {
        assert_eq!(format_uptime(86400), "1d");
        assert_eq!(format_uptime(104400), "1d 5h");
        assert_eq!(format_uptime(172800), "2d");
    }

    // --- process_line_spans: running ---

    #[test]
    fn spans_running_basic() {
        let proc = test_proc(
            "web",
            ProcessState::Running {
                pid: 1234,
                uptime_secs: 65,
            },
        );
        let line = process_line_spans(&proc, 10);
        let text = span_text(&line);

        assert!(text.contains("•"));
        assert!(text.contains(".web"));
        assert!(text.contains("on"));

        let dot = find_span(&line, "•").unwrap();
        assert_eq!(dot.style, Style::default().fg(Color::Green));
        let on = find_span(&line, "on").unwrap();
        assert_eq!(on.style, Style::default().fg(Color::Green));
    }

    #[test]
    fn spans_running_with_ports() {
        let mut proc = test_proc(
            "web",
            ProcessState::Running {
                pid: 99,
                uptime_secs: 5,
            },
        );
        proc.ports = vec![3000, 3001];
        let line = process_line_spans(&proc, 5);
        let text = span_text(&line);

        assert!(text.contains("3000, 3001"));
    }

    #[test]
    fn spans_running_name_padding() {
        let proc = test_proc(
            "web",
            ProcessState::Running {
                pid: 1,
                uptime_secs: 0,
            },
        );
        let line = process_line_spans(&proc, 10);
        let text = span_text(&line);
        // ".web" padded to 10 chars
        assert!(text.contains(".web      "));
    }

    // --- process_line_spans: stopped ---

    #[test]
    fn spans_stopped_autostart() {
        let proc = test_proc("worker", ProcessState::Stopped);
        let line = process_line_spans(&proc, 8);
        let text = span_text(&line);

        assert!(text.contains("◦"));
        assert!(text.contains("off"));
        assert!(!text.contains("exit"));

        let sq = find_span(&line, "◦").unwrap();
        assert_eq!(sq.style, Style::default().add_modifier(Modifier::DIM));
    }

    #[test]
    fn spans_stopped_optional() {
        let mut proc = test_proc("optional-svc", ProcessState::Stopped);
        proc.autostart = false;
        let line = process_line_spans(&proc, 14);
        let text = span_text(&line);

        assert!(text.contains("◦"));
        assert!(text.contains("optional"));

        let circle = find_span(&line, "◦").unwrap();
        assert_eq!(circle.style, Style::default().add_modifier(Modifier::DIM));
    }

    // --- process_line_spans: crashed ---

    #[test]
    fn spans_crashed() {
        let proc = test_proc(
            "api",
            ProcessState::Crashed {
                exit_code: 137,
                retries: 2,
            },
        );
        let line = process_line_spans(&proc, 5);
        let text = span_text(&line);

        assert!(text.contains("⚠"));
        assert!(text.contains("crashed"));
        assert!(text.contains("exit 137"));
        assert!(text.contains("retry 2"));

        let warn = find_span(&line, "⚠").unwrap();
        assert_eq!(warn.style, Style::default().fg(Color::Yellow));
        let crashed = find_span(&line, "crashed").unwrap();
        assert_eq!(crashed.style, Style::default().fg(Color::Yellow));
    }

    // --- process_line_spans: failed ---

    #[test]
    fn spans_failed() {
        let proc = test_proc("bg", ProcessState::Failed { exit_code: 1 });
        let line = process_line_spans(&proc, 5);
        let text = span_text(&line);

        assert!(text.contains("✖"));
        assert!(text.contains("failed"));
        assert!(text.contains("exit 1"));

        let x = find_span(&line, "✖").unwrap();
        assert_eq!(x.style, Style::default().fg(Color::Red));
        let failed = find_span(&line, "failed").unwrap();
        assert_eq!(failed.style, Style::default().fg(Color::Red));
    }

    // --- status_data_to_lines: condensed (default) ---

    #[test]
    fn lines_condensed_running_service() {
        let svc = test_service(
            "myapp",
            vec![test_proc(
                "web",
                ProcessState::Running {
                    pid: 1,
                    uptime_secs: 10,
                },
            )],
        );
        let data = test_status_data(vec![svc]);
        let lines = status_data_to_lines(&data);

        assert_eq!(lines.len(), 1);
        let text = span_text(&lines[0]);
        assert!(text.contains("●"));
        assert!(text.contains("myapp"));
        assert!(text.contains("on"));

        let dot = find_span(&lines[0], "●").unwrap();
        assert_eq!(dot.style, Style::default().fg(Color::Green));
    }

    #[test]
    fn lines_condensed_stopped_service() {
        let svc = test_service("myapp", vec![test_proc("web", ProcessState::Stopped)]);
        let data = test_status_data(vec![svc]);
        let lines = status_data_to_lines(&data);

        assert_eq!(lines.len(), 1);
        let dot = find_span(&lines[0], "◻").unwrap();
        assert_eq!(dot.style, Style::default().add_modifier(Modifier::DIM));
    }

    #[test]
    fn lines_condensed_multiple_services() {
        let svc1 = test_service(
            "alpha",
            vec![
                test_proc(
                    "web",
                    ProcessState::Running {
                        pid: 1,
                        uptime_secs: 0,
                    },
                ),
                test_proc("worker", ProcessState::Stopped),
            ],
        );
        let svc2 = test_service(
            "beta",
            vec![test_proc("api", ProcessState::Failed { exit_code: 1 })],
        );
        let data = test_status_data(vec![svc1, svc2]);
        let lines = status_data_to_lines(&data);

        // condensed: 1 line per service
        assert_eq!(lines.len(), 2);
        assert!(span_text(&lines[0]).contains("alpha"));
        assert!(span_text(&lines[1]).contains("beta"));
    }

    #[test]
    fn lines_condensed_multi_proc_has_mini_icons() {
        let svc = test_service(
            "myapp",
            vec![
                test_proc(
                    "web",
                    ProcessState::Running {
                        pid: 1,
                        uptime_secs: 0,
                    },
                ),
                test_proc("worker", ProcessState::Stopped),
            ],
        );
        let data = test_status_data(vec![svc]);
        let lines = status_data_to_lines(&data);

        assert_eq!(lines.len(), 1);
        let text = span_text(&lines[0]);
        assert!(text.contains("•"));
        assert!(text.contains("◦"));
    }

    // --- status_data_to_lines: detailed ---

    #[test]
    fn lines_detailed_running_service() {
        let svc = test_service(
            "myapp",
            vec![
                test_proc(
                    "web",
                    ProcessState::Running {
                        pid: 1,
                        uptime_secs: 10,
                    },
                ),
                test_proc("worker", ProcessState::Stopped),
            ],
        );
        let mut data = test_status_data(vec![svc]);
        data.detailed = true;
        let lines = status_data_to_lines(&data);

        // header + 2 processes
        assert_eq!(lines.len(), 3);
        let header_text = span_text(&lines[0]);
        assert!(header_text.contains("myapp"));

        let dot = find_span(&lines[0], "●").unwrap();
        assert_eq!(dot.style, Style::default().fg(Color::Green));
    }

    #[test]
    fn lines_detailed_single_proc_inlined() {
        let svc = test_service("myapp", vec![test_proc("web", ProcessState::Stopped)]);
        let mut data = test_status_data(vec![svc]);
        data.detailed = true;
        let lines = status_data_to_lines(&data);

        // single proc, not is_single_service => inlined to 1 line
        assert_eq!(lines.len(), 1);
        assert!(span_text(&lines[0]).contains("myapp"));
        assert!(span_text(&lines[0]).contains("off"));
    }

    // --- status_data_to_lines: show_extras ---

    #[test]
    fn lines_show_extras_with_http() {
        let mut data = test_status_data(vec![]);
        data.show_extras = true;
        data.http_port = Some(13369);
        let lines = status_data_to_lines(&data);

        assert_eq!(lines.len(), 2);
        let serve_text = span_text(&lines[1]);
        assert!(serve_text.contains("serve"));
        assert!(serve_text.contains("http://127.0.0.1:13369"));

        let dot = find_span(&lines[1], "●").unwrap();
        assert_eq!(dot.style, Style::default().fg(Color::Green));
    }

    #[test]
    fn lines_show_extras_no_http() {
        let mut data = test_status_data(vec![]);
        data.show_extras = true;
        data.http_port = None;
        let lines = status_data_to_lines(&data);

        assert_eq!(lines.len(), 2);
        let serve_text = span_text(&lines[1]);
        assert!(serve_text.contains("off"));

        let circle = find_span(&lines[1], "○").unwrap();
        assert_eq!(circle.style, Style::default().add_modifier(Modifier::DIM));
    }

    // --- status_data_to_lines: process_filter ---

    #[test]
    fn lines_process_filter_found() {
        let svc = test_service(
            "myapp",
            vec![
                test_proc(
                    "web",
                    ProcessState::Running {
                        pid: 1,
                        uptime_secs: 0,
                    },
                ),
                test_proc("worker", ProcessState::Stopped),
            ],
        );
        let mut data = test_status_data(vec![svc]);
        data.process_filter = Some("worker".to_string());
        let lines = status_data_to_lines(&data);

        assert_eq!(lines.len(), 1);
        assert!(span_text(&lines[0]).contains("off"));
    }

    #[test]
    fn lines_process_filter_not_found() {
        let svc = test_service(
            "myapp",
            vec![test_proc(
                "web",
                ProcessState::Running {
                    pid: 1,
                    uptime_secs: 0,
                },
            )],
        );
        let mut data = test_status_data(vec![svc]);
        data.process_filter = Some("nonexistent".to_string());
        let lines = status_data_to_lines(&data);

        assert!(lines.is_empty());
    }

    // --- status_data_to_lines: cron jobs ---

    #[test]
    fn lines_cron_jobs() {
        let mut data = test_status_data(vec![]);
        data.show_extras = true;
        data.http_port = None;
        data.cron_jobs = Some(vec![
            koku::JobStatus {
                name: "backup".to_string(),
                state: koku::JobState::Idle,
                last_run: None,
                last_exit: None,
                next_run: Some("2026-03-01T00:00:00".to_string()),
            },
            koku::JobStatus {
                name: "sync".to_string(),
                state: koku::JobState::Running,
                last_run: None,
                last_exit: None,
                next_run: None,
            },
        ]);
        let lines = status_data_to_lines(&data);

        // "" + serve + "" + cron-header + backup + sync = 6
        assert_eq!(lines.len(), 6);
        let cron_header = span_text(&lines[3]);
        assert!(cron_header.contains("cron"));
        let dot = find_span(&lines[3], "●").unwrap();
        assert_eq!(dot.style, Style::default().fg(Color::Green));

        let backup_text = span_text(&lines[4]);
        assert!(backup_text.contains("backup"));
        assert!(backup_text.contains("idle"));
        assert!(backup_text.contains("next 2026-03-01T00:00:00"));

        let sync_text = span_text(&lines[5]);
        assert!(sync_text.contains("sync"));
        assert!(sync_text.contains("running"));
    }

    #[test]
    fn lines_cron_no_running_jobs() {
        let mut data = test_status_data(vec![]);
        data.show_extras = true;
        data.http_port = None;
        data.cron_jobs = Some(vec![koku::JobStatus {
            name: "cleanup".to_string(),
            state: koku::JobState::Paused,
            last_run: None,
            last_exit: Some(0),
            next_run: None,
        }]);
        let lines = status_data_to_lines(&data);

        // "" + serve + "" + cron-header + cleanup = 5
        assert_eq!(lines.len(), 5);
        let cron_header = &lines[3];
        let circle = find_span(cron_header, "○").unwrap();
        assert_eq!(circle.style, Style::default().add_modifier(Modifier::DIM));
    }

    // --- parity: print_process_line text matches process_line_spans text ---

    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // skip until 'm'
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == 'm' {
                        break;
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Mirror of print_process_line for test assertions (must stay in sync).
    fn capture_print_process_line(proc: &ProcessStatus, width: usize) -> String {
        let pcolor = process_state_color(proc);
        let dur_str = format_state_duration(proc.state_since);
        let (symbol, label, extra) = match &proc.state {
            ProcessState::Running { .. } if is_port_pending(proc) => {
                let extra = format!("{:<8}", color_duration(&dur_str, pcolor));
                ("◌".cyan().to_string(), "starting".cyan().to_string(), extra)
            }
            ProcessState::Running { .. } => {
                let ports = if proc.ports.is_empty() {
                    String::new()
                } else {
                    proc.ports
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let extra = format!("{:<8} {}", color_duration(&dur_str, pcolor), ports);
                ("•".green().to_string(), "on".green().to_string(), extra)
            }
            ProcessState::Stopped if !proc.autostart => {
                let extra = if dur_str.is_empty() {
                    String::new()
                } else {
                    color_duration(&dur_str, pcolor)
                };
                (
                    "◦".dimmed().to_string(),
                    "optional".dimmed().to_string(),
                    extra,
                )
            }
            ProcessState::Stopped => {
                let extra = if dur_str.is_empty() {
                    String::new()
                } else {
                    color_duration(&dur_str, pcolor)
                };
                ("◦".dimmed().to_string(), "off".dimmed().to_string(), extra)
            }
            ProcessState::Crashed { exit_code, retries } => {
                let dur_prefix = if dur_str.is_empty() {
                    String::new()
                } else {
                    format!("{}  ", color_duration(&dur_str, pcolor))
                };
                let extra = format!("{}exit {}  retry {}", dur_prefix, exit_code, retries);
                (
                    "⚠".yellow().to_string(),
                    "crashed".yellow().to_string(),
                    extra,
                )
            }
            ProcessState::Failed { exit_code } => {
                let dur_prefix = if dur_str.is_empty() {
                    String::new()
                } else {
                    format!("{}  ", color_duration(&dur_str, pcolor))
                };
                let extra = format!("{}exit {}", dur_prefix, exit_code);
                ("✖".red().to_string(), "failed".red().to_string(), extra)
            }
        };
        let dotname = format!(".{}", proc.name);
        let extra_str = if extra.is_empty() {
            String::new()
        } else {
            format!("  {}", extra.trim_end())
        };
        format!(
            "{} {:<w$} {}{}",
            symbol,
            dotname,
            label,
            extra_str,
            w = width
        )
    }

    #[test]
    fn parity_running() {
        let proc = test_proc(
            "web",
            ProcessState::Running {
                pid: 42,
                uptime_secs: 3661,
            },
        );
        let println_out = strip_ansi(&capture_print_process_line(&proc, 10));
        let ratatui_out = span_text(&process_line_spans(&proc, 10));
        assert_eq!(println_out, ratatui_out);
    }

    #[test]
    fn parity_stopped() {
        let proc = test_proc("worker", ProcessState::Stopped);
        let println_out = strip_ansi(&capture_print_process_line(&proc, 8));
        let ratatui_out = span_text(&process_line_spans(&proc, 8));
        assert_eq!(println_out, ratatui_out);
    }

    #[test]
    fn parity_optional() {
        let mut proc = test_proc("opt", ProcessState::Stopped);
        proc.autostart = false;
        let println_out = strip_ansi(&capture_print_process_line(&proc, 6));
        let ratatui_out = span_text(&process_line_spans(&proc, 6));
        assert_eq!(println_out, ratatui_out);
    }

    #[test]
    fn parity_crashed() {
        let proc = test_proc(
            "api",
            ProcessState::Crashed {
                exit_code: 1,
                retries: 3,
            },
        );
        let println_out = strip_ansi(&capture_print_process_line(&proc, 6));
        let ratatui_out = span_text(&process_line_spans(&proc, 6));
        assert_eq!(println_out, ratatui_out);
    }

    #[test]
    fn parity_failed() {
        let proc = test_proc("bg", ProcessState::Failed { exit_code: 127 });
        let println_out = strip_ansi(&capture_print_process_line(&proc, 5));
        let ratatui_out = span_text(&process_line_spans(&proc, 5));
        assert_eq!(println_out, ratatui_out);
    }

    #[test]
    fn parity_running_with_ports() {
        let mut proc = test_proc(
            "web",
            ProcessState::Running {
                pid: 100,
                uptime_secs: 30,
            },
        );
        proc.ports = vec![8080, 8443];
        let println_out = strip_ansi(&capture_print_process_line(&proc, 7));
        let ratatui_out = span_text(&process_line_spans(&proc, 7));
        assert_eq!(println_out, ratatui_out);
    }

    // --- aggregate_state ---

    #[test]
    fn aggregate_all_running() {
        let svc = test_service(
            "app",
            vec![test_proc(
                "web",
                ProcessState::Running {
                    pid: 1,
                    uptime_secs: 0,
                },
            )],
        );
        assert_eq!(aggregate_state(&svc), AggregateState::On);
    }

    #[test]
    fn aggregate_all_stopped() {
        let svc = test_service("app", vec![test_proc("web", ProcessState::Stopped)]);
        assert_eq!(aggregate_state(&svc), AggregateState::Off);
    }

    #[test]
    fn aggregate_all_failed() {
        let svc = test_service(
            "app",
            vec![test_proc("web", ProcessState::Failed { exit_code: 1 })],
        );
        assert_eq!(aggregate_state(&svc), AggregateState::Err);
    }

    #[test]
    fn aggregate_mixed_running_failed() {
        let svc = test_service(
            "app",
            vec![
                test_proc(
                    "web",
                    ProcessState::Running {
                        pid: 1,
                        uptime_secs: 0,
                    },
                ),
                test_proc("worker", ProcessState::Failed { exit_code: 1 }),
            ],
        );
        assert_eq!(aggregate_state(&svc), AggregateState::Degraded);
    }

    // ── remove_project_entry tests ───────────────────────────────────────────

    #[test]
    fn remove_simple_entry() {
        let content = "foo = \"/dev/foo\"\nbar = \"/dev/bar\"\nbaz = \"/dev/baz\"\n";
        let result = remove_project_entry(content, "bar").unwrap();
        assert_eq!(result, "foo = \"/dev/foo\"\n\nbaz = \"/dev/baz\"\n");
    }

    #[test]
    fn remove_table_entry() {
        let content = "foo = \"/dev/foo\"\n\n[tunnel]\nrun = \"ssh -N server\"\nrestart = true\n\n[other]\nrun = \"sleep 999\"\n";
        let result = remove_project_entry(content, "tunnel").unwrap();
        assert_eq!(
            result,
            "foo = \"/dev/foo\"\n\n[other]\nrun = \"sleep 999\"\n"
        );
    }

    #[test]
    fn remove_table_entry_at_end() {
        let content = "foo = \"/dev/foo\"\n\n[tunnel]\nrun = \"ssh -N server\"\n";
        let result = remove_project_entry(content, "tunnel").unwrap();
        assert_eq!(result, "foo = \"/dev/foo\"\n");
    }

    #[test]
    fn remove_only_simple_entry() {
        let content = "foo = \"/dev/foo\"\n";
        let result = remove_project_entry(content, "foo").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn remove_only_table_entry() {
        let content = "[tunnel]\nrun = \"ssh server\"\n";
        let result = remove_project_entry(content, "tunnel").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let content = "foo = \"/dev/foo\"\n";
        assert!(remove_project_entry(content, "bar").is_none());
    }

    #[test]
    fn remove_preserves_other_entries() {
        let content =
            "a = \"/dev/a\"\nb = \"/dev/b\"\nc = \"/dev/c\"\n\n[daemon]\nrun = \"sleep 999\"\n";
        let result = remove_project_entry(content, "b").unwrap();
        assert!(result.contains("a = \"/dev/a\""));
        assert!(!result.contains("b = "));
        assert!(result.contains("c = \"/dev/c\""));
        assert!(result.contains("[daemon]"));
        assert!(result.contains("run = \"sleep 999\""));
    }

    #[test]
    fn remove_table_preserves_simple_entries() {
        let content = "a = \"/dev/a\"\nb = \"/dev/b\"\n\n[daemon]\nrun = \"sleep 999\"\n";
        let result = remove_project_entry(content, "daemon").unwrap();
        assert!(result.contains("a = \"/dev/a\""));
        assert!(result.contains("b = \"/dev/b\""));
        assert!(!result.contains("[daemon]"));
        assert!(!result.contains("sleep 999"));
    }

    // ── insert_before_first_table tests ──────────────────────────────────────

    #[test]
    fn insert_before_table_header() {
        let content = "[tunnel]\nrun = \"ssh server\"\n";
        let result = insert_before_first_table(content, "foo = \"/dev/foo\"");
        assert!(result.starts_with("foo = \"/dev/foo\""));
        assert!(result.contains("[tunnel]"));
    }

    #[test]
    fn insert_with_existing_simple_entries() {
        let content = "bar = \"/dev/bar\"\n\n[tunnel]\nrun = \"ssh\"\n";
        let result = insert_before_first_table(content, "foo = \"/dev/foo\"");
        // foo should come after bar but before [tunnel]
        let foo_pos = result.find("foo =").unwrap();
        let bar_pos = result.find("bar =").unwrap();
        let tunnel_pos = result.find("[tunnel]").unwrap();
        assert!(bar_pos < foo_pos);
        assert!(foo_pos < tunnel_pos);
    }

    #[test]
    fn insert_into_empty_file() {
        let result = insert_before_first_table("", "foo = \"/dev/foo\"");
        assert_eq!(result, "foo = \"/dev/foo\"");
    }

    #[test]
    fn insert_no_tables() {
        let content = "bar = \"/dev/bar\"\n";
        let result = insert_before_first_table(content, "foo = \"/dev/foo\"");
        assert!(result.contains("bar = \"/dev/bar\""));
        assert!(result.contains("foo = \"/dev/foo\""));
    }
}
