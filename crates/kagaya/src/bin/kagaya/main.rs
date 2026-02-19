mod autostart;
mod config;
mod daemon;
mod koku_client;
mod launchd;
mod logs;
mod migrate;
mod protocol;
mod self_update;
mod utils;

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use config::ServiceEntry;
use protocol::{Request, Response};
use kagaya::*;
use owo_colors::OwoColorize;
use toml;

fn daemon_paths() -> muzan::DaemonPaths {
	muzan::DaemonPaths::new("kagaya")
}

fn main() {
	let args: Vec<String> = std::env::args().skip(1).collect();

	if args.is_empty() {
		print_usage();
		if connect_daemon().is_some() {
			eprintln!();
			render_status(&[]);
		}
		check_alias_hint();
		return;
	}

	match args[0].as_str() {
		"help" | "--help" | "-h" => print_usage(),
		"version" | "--version" | "-V" => println!("kagaya {}", env!("CARGO_PKG_VERSION")),
		"init" => cmd_init(),
		"add" => cmd_add(&args[1..]),
		"remove" | "rm" => cmd_remove(&args[1..]),
		"status" | "st" => cmd_status(&args[1..]),
		"all" => cmd_status(&["all".to_string()]),
		"start" => cmd_start(&args[1..]),
		"stop" => cmd_stop(&args[1..]),
		"reload" => cmd_reload(&args[1..]),
		"restart" => cmd_restart(&args[1..]),
		"logs" => cmd_logs(&args[1..]),
		"tail" => cmd_tail(&args[1..]),
		"echo" => cmd_echo(&args[1..]),
		"show" => cmd_show(&args[1..]),
		"daemon" => cmd_daemon(&args[1..]),
		"serve" => cmd_serve(&args[1..]),
		"migrate" => {
			let force = args[1..].iter().any(|a| a == "--force" || a == "-f");
			migrate::cmd_migrate(force);
		}
		"cron" => cmd_cron(&args[1..]),
		"autostart" => autostart::cmd_autostart(&args[1..]),
		"launchd" | "launch" | "lctl" => launchd::cmd_launchd(&args[1..]),
		"self" => {
			match args.get(1).map(|s| s.as_str()) {
				Some("update") => self_update::cmd_self_update(),
				_ => {
					eprintln!("usage: ky self update");
					std::process::exit(1);
				}
			}
		}
		name => {
			let services = config::load_service_entries();
			let base_name = name.split('.').next().unwrap_or(name);
			if services.contains_key(base_name) && args.len() > 1 {
				match args[1].as_str() {
					"start" => cmd_start(&[args[0].clone()]),
					"stop" => cmd_stop(&[args[0].clone()]),
					"reload" => cmd_reload(&[args[0].clone()]),
					"status" | "st" => cmd_status(&[args[0].clone()]),
					"logs" => cmd_logs(&args),
					"tail" => cmd_tail(&args),
					"echo" => cmd_echo(&args),
					"show" => cmd_show(&args),
					"restart" => {
						if args.len() > 2 {
							cmd_restart(&[args[0].clone(), args[2].clone()]);
						} else {
							cmd_reload(&[args[0].clone()]);
						}
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
				let names: Vec<&str> = services.keys().map(|s| s.as_str()).collect();
				if !names.is_empty() {
					eprintln!("registered services: {}", names.join(", "));
				}
				eprintln!("run 'ky help' for usage");
				std::process::exit(1);
			}
		}
	}
}

fn print_usage() {
	eprintln!("{} {} — process daemon manager", "ky".bold(), env!("CARGO_PKG_VERSION"));
	eprintln!();
	eprintln!("usage: {} [command] [service] [options]", "ky".bold());
	eprintln!();

	eprintln!("{}", "services".cyan().bold());
	eprintln!("  {} [name|--all]          Show status (default command)", "status".bold());
	eprintln!("  {} [name|--all]           Start service(s)", "start".bold());
	eprintln!("  {} [name|--all]            Stop service(s)", "stop".bold());
	eprintln!("  {} [name|--all]          Reload (stop + start)", "reload".bold());
	eprintln!("  {} [name] [process]     Restart a single process", "restart".bold());
	eprintln!();

	eprintln!("{}", "logs".cyan().bold());
	eprintln!("  {} <name> [process]        Last 100 lines of log file", "logs".bold());
	eprintln!("  {} <name> [process]        Follow log file (tail -f)", "tail".bold());
	eprintln!("  {} <name> [process]        Live output stream from daemon", "echo".bold());
	eprintln!();

	eprintln!("{}", "config".cyan().bold());
	eprintln!("  {} [name] [process]        Show services.toml or process command", "show".bold());
	eprintln!("  {} [name] [dir]             Register a project", "add".bold());
	eprintln!("  {} <name>                Unregister a project", "remove".bold());
	eprintln!("  {}                         Create config files", "init".bold());
	eprintln!("  {} [--force]             Migrate ubermind Procfiles to kagaya TOML", "migrate".bold());
	eprintln!();

	eprintln!("{}", "cron (via koku)".cyan().bold());
	eprintln!("  {} [status|--json]       Show cron job status", "cron".bold());
	eprintln!("  {} run <name>             Trigger a one-off run", "cron".bold());
	eprintln!("  {} pause <name>           Pause a cron job", "cron".bold());
	eprintln!("  {} resume <name>          Resume a cron job", "cron".bold());
	eprintln!("  {} reload                 Reload koku config", "cron".bold());
	eprintln!();

	eprintln!("{}", "system".cyan().bold());
	eprintln!("  {} [on|off|status]   Start services on login", "autostart".bold());
	eprintln!("  {} [start|stop|status]   Manage the daemon", "daemon".bold());
	eprintln!("  {} [-d|--stop|--status]   HTTP server for web UI", "serve".bold());
	eprintln!("  {} [command]            macOS launchd agents", "launchd".bold());
	eprintln!("  {}                  Update to latest version", "self update".bold());
	eprintln!();

	eprintln!("{}", "targeting".cyan().bold());
	eprintln!("  Use {} dot syntax to target a specific process:", "name.process".bold());
	eprintln!("    ky status matrix.automation");
	eprintln!("  Context-aware: run from a project dir to auto-target it");
	eprintln!("    ky restart api             restart 'api' in current project");
	eprintln!("    ky restart appligator api  target a specific project");
	eprintln!();

	eprintln!("{}", "shortcuts".cyan().bold());
	eprintln!("    ky                         status (current project or all)");
	eprintln!("    ky all                     status --all");
	eprintln!("    ky --watch                 status --watch (live refresh)");
}

// --- Config management (no daemon needed) ---

fn cmd_init() {
	let config_dir = protocol::config_dir();
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

fn cmd_add(args: &[String]) {
	let config_dir = protocol::config_dir();
	let _ = std::fs::create_dir_all(&config_dir);
	let projects_file = config_dir.join("projects.toml");

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

	// Check for duplicate in existing projects.toml
	if let Ok(content) = std::fs::read_to_string(&projects_file) {
		if let Ok(table) = toml::from_str::<toml::Value>(&content) {
			if let Some(map) = table.as_table() {
				if map.contains_key(&name) {
					eprintln!("{}: already registered", name);
					return;
				}
			}
		}
	}

	let services_toml = dir.join("services.toml");
	if !services_toml.exists() {
		eprintln!("note: no services.toml found in {}", dir.display());
		eprintln!("create one with service definitions, e.g.:");
		eprintln!("  web = \"npm run dev\"");
	}

	let mut file = std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open(&projects_file)
		.unwrap();
	writeln!(file, "{} = {:?}", name, dir.display().to_string()).unwrap();
	eprintln!("{}: added ({})", name, dir.display());
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

	let config_dir = protocol::config_dir();
	let projects_file = config_dir.join("projects.toml");

	let content = match std::fs::read_to_string(&projects_file) {
		Ok(c) => c,
		Err(_) => {
			eprintln!("no projects.toml found");
			std::process::exit(1);
		}
	};

	let mut table: toml::Table = match toml::from_str(&content) {
		Ok(t) => t,
		Err(e) => {
			eprintln!("failed to parse projects.toml: {}", e);
			std::process::exit(1);
		}
	};

	if table.remove(&name).is_none() {
		eprintln!("{}: not found in projects.toml", name);
		std::process::exit(1);
	}

	let new_content = toml::to_string_pretty(&table).unwrap();
	std::fs::write(&projects_file, new_content).unwrap();
	eprintln!("{}: removed", name);
}

// --- Daemon communication ---

fn connect_daemon() -> Option<muzan::DaemonClient<Request, Response>> {
	muzan::DaemonClient::connect(&daemon_paths()).ok()
}

fn send_request(request: &Request) -> Response {
	let paths = daemon_paths();
	let mut client = match muzan::ensure_daemon_with_args::<Request, Response>(
		&paths,
		&["daemon", "run"],
	) {
		Ok(c) => c,
		Err(e) => {
			eprintln!("error: {}", e);
			std::process::exit(1);
		}
	};

	match client.send(request) {
		Ok(resp) => resp,
		Err(e) => {
			eprintln!("error: {}", e);
			std::process::exit(1);
		}
	}
}

// --- Commands that talk to daemon ---

fn cmd_status(args: &[String]) {
	let (watch, rest) = parse_watch_opts(args, None);
	if watch.enabled {
		watch_status(&rest, &watch);
	} else {
		render_status(&rest);
	}
}

fn print_process_line(proc: &ProcessStatus, name_width: usize) {
	let (symbol, label, extra) = match &proc.state {
		ProcessState::Running { pid, uptime_secs } => {
			let ports = if proc.ports.is_empty() {
				String::new()
			} else {
				proc.ports.iter().map(|p| format!(":{}", p)).collect::<Vec<_>>().join(",")
			};
			let extra = format!("{:<8} {:<8} {}", format_uptime(*uptime_secs), pid, ports);
			("●".green().to_string(), "on".green().to_string(), extra)
		}
		ProcessState::Stopped if !proc.autostart => {
			("○".dimmed().to_string(), "optional".dimmed().to_string(), String::new())
		}
		ProcessState::Stopped => {
			("◻".dimmed().to_string(), "off".dimmed().to_string(), String::new())
		}
		ProcessState::Crashed { exit_code, retries } => {
			let extra = format!("exit {}  retry {}", exit_code, retries);
			("⚠".yellow().to_string(), "crashed".yellow().to_string(), extra)
		}
		ProcessState::Failed { exit_code } => {
			let extra = format!("exit {}", exit_code);
			("✖".red().to_string(), "failed".red().to_string(), extra)
		}
	};
	let extra_str = if extra.is_empty() { String::new() } else { format!("  {}", extra.trim_end()) };
	println!("  {} {:<width$} {}{}", symbol, proc.name, label, extra_str, width = name_width);
}

fn cmd_start(args: &[String]) {
	let (mut watch, rest) = parse_watch_opts(args, Some(4));
	let entries = config::load_service_entries();

	let autostart_only = rest.iter().any(|a| a == "--autostart");
	let start_all = rest.iter().any(|a| is_all_flag(a));
	let rest: Vec<String> = rest.into_iter().filter(|a| !is_all_flag(a) && a != "--autostart").collect();

	if autostart_only {
		let names = config::autostart_project_names();
		if names.is_empty() {
			eprintln!("no projects with autostart = true");
			return;
		}
		let response = send_request(&Request::Start {
			names: names.clone(),
			all: true,
			processes: vec![],
		});
		match response {
			Response::Ok { message } => {
				if let Some(msg) = message {
					for line in msg.lines() {
						eprintln!("{}", line);
					}
				}
			}
			Response::Error { message } => {
				eprintln!("error: {}", message);
				std::process::exit(1);
			}
			_ => {}
		}
		return;
	}

	let mut target_processes: Vec<String> = Vec::new();
	let resolved: Vec<String> = if start_all && rest.is_empty() {
		entries.keys().cloned().collect()
	} else if rest.is_empty() {
		resolve_target_names(&[], &entries)
	} else {
		let mut service_names = Vec::new();
		for arg in &rest {
			let (svc, proc) = resolve_dot_target(arg, &entries);
			if let Some(p) = proc {
				if !service_names.contains(&svc) {
					service_names.push(svc);
				}
				if !target_processes.contains(&p) {
					target_processes.push(p);
				}
			} else if entries.contains_key(&svc) {
				if !service_names.contains(&svc) {
					service_names.push(svc);
				}
			} else if let Some(current) = get_current_project(&entries) {
				if !service_names.contains(&current) {
					service_names.push(current);
				}
				if !target_processes.contains(&svc) {
					target_processes.push(svc);
				}
			} else {
				eprintln!("unknown service: {}", svc);
				eprintln!("registered services: {}", entries.keys().cloned().collect::<Vec<_>>().join(", "));
				std::process::exit(1);
			}
		}
		service_names
	};

	if resolved.is_empty() {
		eprintln!("no services to start");
		std::process::exit(1);
	}

	let response = send_request(&Request::Start {
		names: resolved.clone(),
		all: start_all || !target_processes.is_empty(),
		processes: target_processes,
	});
	match response {
		Response::Ok { message } => {
			if let Some(msg) = message {
				for line in msg.lines() {
					eprintln!("{}", line);
				}
			}
			std::thread::sleep(std::time::Duration::from_millis(500));

			if !watch.enabled {
				watch.enabled = true;
				watch.duration = Some(4);
			}
			watch_status(&resolved, &watch);
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

	let stop_all = rest.iter().any(|a| is_all_flag(a));
	let rest: Vec<String> = rest.into_iter().filter(|a| !is_all_flag(a)).collect();

	let mut target_processes: Vec<String> = Vec::new();
	let names: Vec<String> = if stop_all && rest.is_empty() {
		entries.keys().cloned().collect()
	} else if rest.is_empty() {
		resolve_target_names(&[], &entries)
	} else {
		let mut service_names = Vec::new();
		for arg in &rest {
			let (svc, proc) = resolve_dot_target(arg, &entries);
			if let Some(p) = proc {
				if !service_names.contains(&svc) {
					service_names.push(svc);
				}
				if !target_processes.contains(&p) {
					target_processes.push(p);
				}
			} else if entries.contains_key(&svc) {
				if !service_names.contains(&svc) {
					service_names.push(svc);
				}
			} else if let Some(current) = get_current_project(&entries) {
				if !service_names.contains(&current) {
					service_names.push(current);
				}
				if !target_processes.contains(&svc) {
					target_processes.push(svc);
				}
			} else {
				eprintln!("unknown service: {}", svc);
				std::process::exit(1);
			}
		}
		service_names
	};

	if names.is_empty() {
		eprintln!("no services to stop");
		std::process::exit(1);
	}

	let response = send_request(&Request::Stop {
		names: names.clone(),
		processes: target_processes,
	});
	match response {
		Response::Ok { message } => {
			if let Some(msg) = message {
				for line in msg.lines() {
					eprintln!("{}", line);
				}
			}
			std::thread::sleep(std::time::Duration::from_millis(500));

			if !watch.enabled {
				watch.enabled = true;
				watch.duration = Some(4);
			}
			watch_status(&names, &watch);
		}
		Response::Error { message } => {
			eprintln!("error: {}", message);
			std::process::exit(1);
		}
		_ => {}
	}
}

fn cmd_reload(args: &[String]) {
	let (mut watch, rest) = parse_watch_opts(args, Some(4));
	let entries = config::load_service_entries();

	let reload_all = rest.iter().any(|a| is_all_flag(a));
	let rest: Vec<String> = rest.into_iter().filter(|a| !is_all_flag(a)).collect();

	let mut target_processes: Vec<String> = Vec::new();
	let names: Vec<String> = if rest.is_empty() {
		resolve_target_names(&[], &entries)
	} else {
		let mut service_names = Vec::new();
		for arg in &rest {
			let (svc, proc) = resolve_dot_target(arg, &entries);
			if let Some(p) = proc {
				if !service_names.contains(&svc) {
					service_names.push(svc);
				}
				if !target_processes.contains(&p) {
					target_processes.push(p);
				}
			} else if entries.contains_key(&svc) {
				if !service_names.contains(&svc) {
					service_names.push(svc);
				}
			} else if let Some(current) = get_current_project(&entries) {
				if !service_names.contains(&current) {
					service_names.push(current);
				}
				if !target_processes.contains(&svc) {
					target_processes.push(svc);
				}
			} else {
				eprintln!("unknown service: {}", svc);
				std::process::exit(1);
			}
		}
		service_names
	};

	if names.is_empty() {
		eprintln!("no services to reload");
		std::process::exit(1);
	}

	let response = send_request(&Request::Reload {
		names: names.clone(),
		all: reload_all,
		processes: target_processes,
	});
	match response {
		Response::Ok { message } => {
			if let Some(msg) = message {
				for line in msg.lines() {
					eprintln!("{}", line);
				}
			}
			std::thread::sleep(std::time::Duration::from_millis(500));

			if !watch.enabled {
				watch.enabled = true;
				watch.duration = Some(4);
			}
			watch_status(&names, &watch);
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

	if !watch.enabled {
		watch.enabled = true;
		watch.duration = Some(4);
	}

	let mut reload_extra: Vec<String> = Vec::new();
	reload_extra.push("--watch".to_string());
	if let Some(d) = watch.duration {
		reload_extra.push(d.to_string());
	}
	if watch.interval != 1 {
		reload_extra.push("--watch-interval".to_string());
		reload_extra.push(watch.interval.to_string());
	}

	let (service, process) = if rest.is_empty() {
		if let Some(current) = get_current_project(&entries) {
			let mut reload_args = vec![current];
			reload_args.extend(reload_extra);
			return cmd_reload(&reload_args);
		} else {
			eprintln!("usage: ky restart <service> [process]");
			eprintln!("or run from a registered project directory");
			std::process::exit(1);
		}
	} else if rest.len() == 1 {
		let (svc, proc) = resolve_dot_target(&rest[0], &entries);
		if let Some(proc_name) = proc {
			(svc, Some(proc_name))
		} else if entries.contains_key(&svc) {
			let mut reload_args = vec![svc];
			reload_args.extend(reload_extra);
			return cmd_reload(&reload_args);
		} else if let Some(current) = get_current_project(&entries) {
			(current, Some(svc))
		} else {
			eprintln!("unknown service: {}", rest[0]);
			eprintln!("registered services: {}", entries.keys().cloned().collect::<Vec<_>>().join(", "));
			std::process::exit(1);
		}
	} else {
		let (svc, proc) = resolve_dot_target(&rest[0], &entries);
		(svc, proc.or_else(|| Some(rest[1].clone())))
	};

	if let Some(process_name) = process {
		let response = send_request(&Request::Restart {
			service: service.clone(),
			process: process_name.clone(),
		});
		match response {
			Response::Ok { message } => {
				if let Some(msg) = message {
					eprintln!("{}", msg);
				}
				std::thread::sleep(std::time::Duration::from_millis(500));
				watch_status(&[service], &watch);
			}
			Response::Error { message } => {
				eprintln!("error: {}", message);
				std::process::exit(1);
			}
			_ => {}
		}
	} else {
		let mut reload_args = vec![service];
		reload_args.extend(reload_extra);
		cmd_reload(&reload_args);
	}
}

fn cmd_logs(args: &[String]) {
	let svc_entries = config::load_service_entries();

	let (service, process) = if args.is_empty() {
		if let Some(current) = get_current_project(&svc_entries) {
			(current, None)
		} else {
			eprintln!("usage: ky logs <service> [process]");
			eprintln!("       ky logs <service.process>");
			std::process::exit(1);
		}
	} else {
		let (svc, proc) = resolve_dot_target(&args[0], &svc_entries);
		(svc, proc.or_else(|| args.get(1).map(|s| s.to_string())))
	};

	let log_dir = logs::service_log_dir(&service);
	if !log_dir.exists() {
		eprintln!("no logs for {}", service);
		std::process::exit(1);
	}

	let mut files: Vec<PathBuf> = Vec::new();
	if let Ok(dir_entries) = std::fs::read_dir(&log_dir) {
		for entry in dir_entries.flatten() {
			let path = entry.path();
			let name = path
				.file_name()
				.unwrap_or_default()
				.to_string_lossy()
				.to_string();
			if !name.ends_with(".log") {
				continue;
			}
			if let Some(ref proc_filter) = process {
				if !name.starts_with(proc_filter.as_str()) {
					continue;
				}
			}
			files.push(path);
		}
	}

	files.sort();

	if files.is_empty() {
		eprintln!("no log files found");
		std::process::exit(1);
	}

	let latest = files.last().unwrap();
	let content = std::fs::read_to_string(latest).unwrap_or_default();

	let lines: Vec<&str> = content.lines().collect();
	let start = if lines.len() > 100 {
		lines.len() - 100
	} else {
		0
	};
	for line in &lines[start..] {
		println!("{}", line);
	}
}

fn cmd_tail(args: &[String]) {
	let svc_entries = config::load_service_entries();

	let (service, process) = if args.is_empty() {
		if let Some(current) = get_current_project(&svc_entries) {
			(current, None)
		} else {
			eprintln!("usage: ky tail <service> [process]");
			eprintln!("       ky tail <service.process>");
			std::process::exit(1);
		}
	} else {
		let (svc, proc) = resolve_dot_target(&args[0], &svc_entries);
		(svc, proc.or_else(|| args.get(1).cloned()))
	};

	let log_dir = logs::service_log_dir(&service);
	if !log_dir.exists() {
		eprintln!("no logs for {}", service);
		std::process::exit(1);
	}

	let mut files: Vec<PathBuf> = Vec::new();
	if let Ok(dir_entries) = std::fs::read_dir(&log_dir) {
		for entry in dir_entries.flatten() {
			let path = entry.path();
			let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
			if !name.ends_with(".log") {
				continue;
			}
			if let Some(ref proc_filter) = process {
				if !name.starts_with(proc_filter.as_str()) {
					continue;
				}
			}
			files.push(path);
		}
	}

	files.sort();

	if files.is_empty() {
		eprintln!("no log files found");
		std::process::exit(1);
	}

	let latest = files.last().unwrap();
	let mut cmd = Command::new("tail");
	cmd.args(["-f", "-n", "100"]);
	cmd.arg(latest);
	let status = cmd.status().unwrap_or_else(|e| {
		eprintln!("error: {}", e);
		std::process::exit(1);
	});
	std::process::exit(status.code().unwrap_or(1));
}

fn cmd_echo(args: &[String]) {
	let svc_entries = config::load_service_entries();

	let (service, process) = if args.is_empty() {
		if let Some(current) = get_current_project(&svc_entries) {
			(current, None)
		} else {
			eprintln!("usage: ky echo <service> [process]");
			eprintln!("       ky echo <service.process>");
			std::process::exit(1);
		}
	} else {
		let (svc, proc) = resolve_dot_target(&args[0], &svc_entries);
		(svc, proc.or_else(|| args.get(1).cloned()))
	};

	let mut offset = 0u64;
	loop {
		let response = send_request(&Request::Logs {
			service: service.clone(),
			process: process.clone(),
			follow: true,
			offset,
		});

		match response {
			Response::Log { line, offset: new_offset } => {
				if !line.is_empty() {
					print!("{}", line);
					let _ = io::stdout().flush();
				}
				offset = new_offset;
			}
			Response::Error { message } => {
				eprintln!("error: {}", message);
				std::process::exit(1);
			}
			_ => {}
		}

		std::thread::sleep(std::time::Duration::from_millis(100));
	}
}

fn cmd_show(args: &[String]) {
	let entries = config::load_service_entries();

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
			let projects_path = protocol::config_dir().join("projects");
			eprintln!("{}", projects_path.display().to_string().dimmed());
			for (name, entry) in &entries {
				eprintln!("{}: {}", name.bold(), entry.dir.display());
			}
			std::process::exit(0);
		}
	} else if filtered_args.len() == 1 {
		let (svc, proc) = resolve_dot_target(&filtered_args[0], &entries);
		if proc.is_some() {
			(svc, proc)
		} else if entries.contains_key(&svc) {
			(svc, None)
		} else if let Some(current) = get_current_project(&entries) {
			(current, Some(svc))
		} else {
			eprintln!("unknown service: {}", filtered_args[0]);
			eprintln!("registered services: {}", entries.keys().cloned().collect::<Vec<_>>().join(", "));
			std::process::exit(1);
		}
	} else {
		let (svc, proc) = resolve_dot_target(&filtered_args[0], &entries);
		(svc, proc.or_else(|| Some(filtered_args[1].clone())))
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
		let services_path = service_entry.dir.join("services.toml");
		eprintln!("no services defined ({})", services_path.display());
		std::process::exit(1);
	}

	if let Some(proc_name) = process_name {
		if let Some(proc) = service.processes.iter().find(|p| p.name == proc_name) {
			println!("{}", proc.command);
		} else {
			eprintln!("process '{}' not found in {}", proc_name, service_name);
			std::process::exit(1);
		}
	} else {
		let services_path = service_entry.dir.join("services.toml");
		println!("{}", services_path.display().to_string().dimmed());
		println!();
		for proc in &service.processes {
			let type_tag = match proc.service_type {
				ServiceType::Task => " (task)".dimmed().to_string(),
				ServiceType::Service => String::new(),
			};
			let optional = if !proc.autostart { " (optional)".dimmed().to_string() } else { String::new() };
			println!("{}{}{} {}", proc.name.cyan(), type_tag, optional, proc.command.dimmed());
		}
	}
}

fn cmd_cron(args: &[String]) {
	let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");

	match subcmd {
		"status" | "st" => {
			let json = args.iter().any(|a| a == "--json");
			match koku_client::fetch_status() {
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
							println!("  {} {:<width$} {}{}", sym, job.name, job.state,
								if extra.is_empty() { String::new() } else { format!("  {}", extra) },
								width = max_name);
						}
					}
				}
				None => {
					eprintln!("koku daemon not running");
					std::process::exit(1);
				}
			}
		}
		"run" => {
			let name = args.get(1).unwrap_or_else(|| {
				eprintln!("usage: ky cron run <name>");
				std::process::exit(1);
			});
			match koku_client::run_job(name) {
				Ok(msg) => eprintln!("{}", msg),
				Err(e) => {
					eprintln!("error: {}", e);
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
				Ok(msg) => eprintln!("{}", msg),
				Err(e) => {
					eprintln!("error: {}", e);
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
				Ok(msg) => eprintln!("{}", msg),
				Err(e) => {
					eprintln!("error: {}", e);
					std::process::exit(1);
				}
			}
		}
		"reload" => {
			match koku_client::reload() {
				Ok(msg) => eprintln!("{}", msg),
				Err(e) => {
					eprintln!("error: {}", e);
					std::process::exit(1);
				}
			}
		}
		_ => {
			eprintln!("usage: ky cron [status|run|pause|resume|reload]");
			std::process::exit(1);
		}
	}
}

fn cmd_daemon(args: &[String]) {
	let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
	let paths = daemon_paths();

	match subcmd {
		"run" => {
			let daemon_args: Vec<String> = args[1..].to_vec();
			tokio::runtime::Runtime::new()
				.unwrap()
				.block_on(daemon::run(&daemon_args));
		}
		"start" => {
			if muzan::client::is_running(&paths) {
				eprintln!("daemon already running");
				return;
			}
			let mut spawn_args: Vec<String> = vec!["daemon".to_string(), "run".to_string()];
			spawn_args.extend(args[1..].iter().cloned());
			let spawn_refs: Vec<&str> = spawn_args.iter().map(|s| s.as_str()).collect();
			let daemon = muzan::Daemon::new("kagaya");
			match daemon.start_background_with_args(&spawn_refs) {
				Ok(_) => eprintln!("daemon started"),
				Err(e) => {
					eprintln!("error: {}", e);
					std::process::exit(1);
				}
			}
		}
		"stop" => {
			let response = send_request(&Request::Shutdown);
			match response {
				Response::Ok { message } => {
					eprintln!("daemon: {}", message.unwrap_or_default());
				}
				_ => eprintln!("daemon not running"),
			}
		}
		"status" => {
			if muzan::client::is_running(&paths) {
				if let Some(pid) = muzan::client::read_pid(&paths) {
					eprintln!("daemon running (pid {})", pid);
				} else {
					eprintln!("daemon running");
				}
			} else {
				eprintln!("daemon not running");
			}
		}
		_ => {
			eprintln!("usage: ky daemon [start|stop|status|run]");
		}
	}
}

fn cmd_serve(args: &[String]) {
	let has_stop = args.iter().any(|a| a == "--stop");
	let has_status = args.iter().any(|a| a == "--status");
	let has_daemon = args.iter().any(|a| a == "-d" || a == "--daemon");

	if has_stop {
		cmd_daemon(&["stop".to_string()].to_vec());
	} else if has_status {
		cmd_daemon(&["status".to_string()].to_vec());
	} else if has_daemon {
		cmd_daemon(&vec!["start".to_string(), "--http".to_string()]);
	} else {
		// Foreground: run daemon in-process with --http
		cmd_daemon(&vec!["run".to_string(), "--foreground".to_string(), "--http".to_string()]);
	}
}

// --- Watch support ---

struct WatchOpts {
	duration: Option<u64>,
	interval: u64,
	enabled: bool,
}

fn parse_watch_opts(args: &[String], default_duration: Option<u64>) -> (WatchOpts, Vec<String>) {
	let mut opts = WatchOpts {
		duration: None,
		interval: 1,
		enabled: false,
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
	let response = send_request(&Request::Status);
	match response {
		Response::Status { services, http_port } => (services, http_port),
		Response::Error { message } => {
			eprintln!("error: {}", message);
			std::process::exit(1);
		}
		_ => {
			eprintln!("unexpected response from daemon");
			std::process::exit(1);
		}
	}
}

fn render_status(args: &[String]) -> usize {
	let (services, http_port) = fetch_status();
	let entries = config::load_service_entries();

	let (process_filter, resolved_args) = if let Some(first) = args.first() {
		let (svc, proc) = resolve_dot_target(first, &entries);
		let rest: Vec<String> = std::iter::once(svc).chain(args[1..].iter().cloned()).collect();
		(proc, rest)
	} else {
		(None, args.to_vec())
	};

	let show_all = !resolved_args.is_empty() && (resolved_args.len() == 1 && is_all_flag(&resolved_args[0]) || resolved_args.iter().any(|a| is_all_flag(a)));
	let current_project = get_current_project(&entries);

	let filter = if resolved_args.is_empty() {
		if let Some(ref current) = current_project {
			vec![current.clone()]
		} else {
			entries.keys().cloned().collect()
		}
	} else if show_all {
		entries.keys().cloned().collect()
	} else {
		resolve_target_names(&resolved_args, &entries)
	};

	let mut status_map: std::collections::HashMap<String, &ServiceStatus> =
		std::collections::HashMap::new();
	for s in &services {
		status_map.insert(s.name.clone(), s);
	}

	let mut sorted_filter = filter.clone();
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

	if let Some(ref proc_name) = process_filter {
		for name in &sorted_filter {
			if let Some(status) = status_map.get(name) {
				for proc in &status.processes {
					if proc.name == *proc_name {
						print_process_line(proc, proc.name.len());
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

	let max_proc_name_width = sorted_filter
		.iter()
		.filter_map(|name| status_map.get(name))
		.flat_map(|s| s.processes.iter().map(|p| p.name.len()))
		.max()
		.unwrap_or(0);

	let mut lines = 0usize;
	for name in &sorted_filter {
		let status = status_map.get(name);
		let running = status.map(|s| s.is_running()).unwrap_or(false);

		let symbol = if running { "●".green().to_string() } else { "◻".dimmed().to_string() };
		println!("{} {}", symbol, name.bold());
		lines += 1;

		if let Some(status) = status {
			for proc in &status.processes {
				print_process_line(proc, max_proc_name_width);
				lines += 1;
			}
		}
	}

	if show_all || (resolved_args.is_empty() && current_project.is_none()) {
		println!();
		lines += 1;
		if let Some(port) = http_port {
			println!("{} {}  http://127.0.0.1:{}", "●".green(), "serve".bold(), port);
		} else {
			println!("{} {}  not running", "○".dimmed(), "serve".bold());
		}
		lines += 1;

		lines += render_cron_status();
	}

	lines
}

fn render_cron_status() -> usize {
	let jobs = match koku_client::fetch_status() {
		Some(jobs) if !jobs.is_empty() => jobs,
		_ => return 0,
	};

	let mut lines = 0;
	println!();
	lines += 1;

	let has_running = jobs.iter().any(|j| j.state == koku::JobState::Running);
	let symbol = if has_running { "●".green().to_string() } else { "○".dimmed().to_string() };
	println!("{} {}", symbol, "cron".bold());
	lines += 1;

	let max_name = jobs.iter().map(|j| j.name.len()).max().unwrap_or(0);

	for job in &jobs {
		let sym = koku_client::state_symbol(&job.state);
		let state_str = job.state.to_string();
		let (sym_colored, state_colored) = match job.state {
			koku::JobState::Running => (sym.green().to_string(), state_str.green().to_string()),
			koku::JobState::Idle => (sym.dimmed().to_string(), state_str.dimmed().to_string()),
			koku::JobState::Paused => (sym.dimmed().to_string(), state_str.dimmed().to_string()),
			koku::JobState::Failing => (sym.yellow().to_string(), state_str.yellow().to_string()),
			koku::JobState::Stopped => (sym.red().to_string(), state_str.red().to_string()),
		};

		let extra = match (&job.last_exit, &job.next_run) {
			(Some(code), Some(next)) => format!("exit {}  next {}", code, next),
			(Some(code), None) => format!("exit {}", code),
			(None, Some(next)) => format!("next {}", next),
			(None, None) => String::new(),
		};

		let extra_str = if extra.is_empty() { String::new() } else { format!("  {}", extra) };
		println!("  {} {:<width$} {}{}", sym_colored, job.name, state_colored, extra_str, width = max_name);
		lines += 1;
	}

	lines
}

fn watch_status(args: &[String], opts: &WatchOpts) {
	let start = Instant::now();
	let mut prev_lines = 0usize;
	let stdout = io::stdout();

	loop {
		if prev_lines > 0 {
			print!("\x1b[{}A\x1b[J", prev_lines);
			let _ = stdout.lock().flush();
		}

		prev_lines = render_status(args);
		let _ = stdout.lock().flush();

		if let Some(duration) = opts.duration {
			if start.elapsed().as_secs() >= duration {
				return;
			}
		}

		std::thread::sleep(std::time::Duration::from_secs(opts.interval));
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

fn resolve_dot_target(name: &str, entries: &BTreeMap<String, ServiceEntry>) -> (String, Option<String>) {
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

fn resolve_target_names(args: &[String], entries: &BTreeMap<String, ServiceEntry>) -> Vec<String> {
	if args.is_empty() {
		if let Ok(cwd) = std::env::current_dir() {
			let cwd = cwd.canonicalize().unwrap_or(cwd);
			for (name, entry) in entries {
				let entry_dir = entry.dir.canonicalize().unwrap_or(entry.dir.clone());
				if cwd == entry_dir {
					return vec![name.clone()];
				}
			}
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
		return entries.keys().cloned().collect();
	}

	args.iter().filter(|a| !is_all_flag(a)).cloned().collect()
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
	// Check SHELL env var
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
