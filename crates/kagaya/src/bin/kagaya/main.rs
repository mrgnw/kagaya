mod autostart;
mod cli;
mod config;
mod daemon;
mod format;
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
use std::time::Instant;
use cli::{Cli, Cmd, OutputFormat, output_format, set_output_format};
use config::ServiceEntry;
use protocol::{Request, Response};
use kagaya::*;
use owo_colors::OwoColorize;
use clap::Parser;

fn daemon_paths() -> muzan::DaemonPaths {
	muzan::DaemonPaths::new("kagaya")
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
				print_usage();
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
						print_usage();
						if connect_daemon().is_some() {
							eprintln!();
							render_status(&[]);
						}
						check_alias_hint();
					}
				}
				Some(Cmd::Status { names, all, watch, watch_interval }) => {
					let mut args = names;
					if all { args.push("--all".to_string()); }
					if watch || cli.watch { args.push("--watch".to_string()); }
					if let Some(iv) = watch_interval {
						args.push("--watch-interval".to_string());
						args.push(iv.to_string());
					}
					cmd_status(&args);
				}
			Some(Cmd::Start { names, all, autostart, echo, watch, watch_interval }) => {
				let mut args = names.clone();
				if all { args.push("--all".to_string()); }
				if autostart { args.push("--autostart".to_string()); }
				if watch || cli.watch { args.push("--watch".to_string()); }
				if let Some(iv) = watch_interval {
					args.push("--watch-interval".to_string());
					args.push(iv.to_string());
				}
				cmd_start(&args);
				if echo { echo_after_action(&names, None); }
			}
			Some(Cmd::Stop { names, all, echo, watch, watch_interval }) => {
				let mut args = names.clone();
				if all { args.push("--all".to_string()); }
				if watch || cli.watch { args.push("--watch".to_string()); }
				if let Some(iv) = watch_interval {
					args.push("--watch-interval".to_string());
					args.push(iv.to_string());
				}
				cmd_stop(&args);
				if echo { echo_after_stop(&names); }
			}
			Some(Cmd::Restart { target, all, echo, watch, watch_interval }) => {
				let mut args = target.clone();
				if all { args.push("--all".to_string()); }
				if watch || cli.watch { args.push("--watch".to_string()); }
				if let Some(iv) = watch_interval {
					args.push("--watch-interval".to_string());
					args.push(iv.to_string());
				}
				cmd_restart(&args);
				if echo { echo_after_action(&target, None); }
			}
				Some(Cmd::Logs { args }) => cmd_logs(&args),
				Some(Cmd::Tail { args }) => cmd_tail(&args),
				Some(Cmd::Echo { args }) => cmd_echo(&args),
				Some(Cmd::Show { args }) => cmd_show(&args),
				Some(Cmd::Cron { args }) => cmd_cron(&args),
				Some(Cmd::Daemon { args }) => cmd_daemon(&args),
				Some(Cmd::Serve { args }) => cmd_serve(&args),
				Some(Cmd::Add { args }) => cmd_add(&args),
				Some(Cmd::Remove { args }) => cmd_remove(&args),
				Some(Cmd::Init) => cmd_init(),
				Some(Cmd::Migrate { force }) => migrate::cmd_migrate(force),
				Some(Cmd::Autostart { args }) => autostart::cmd_autostart(&args),
				Some(Cmd::Launchd { args }) => launchd::cmd_launchd(&args),
				Some(Cmd::SelfCmd { args }) => {
					match args.first().map(|s| s.as_str()) {
						Some("update") => self_update::cmd_self_update(),
						_ => {
							eprintln!("usage: ky self update");
							std::process::exit(1);
						}
					}
				}
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
		let names: Vec<&str> = services.keys().map(|s| s.as_str()).collect();
		if !names.is_empty() {
			eprintln!("registered services: {}", names.join(", "));
		}
		eprintln!("run 'ky help' for usage");
		std::process::exit(1);
	}
}

fn print_usage() {
	eprintln!("{} {} — process daemon manager", "ky".bold(), env!("CARGO_PKG_VERSION"));
	eprintln!();
	eprintln!("usage: {} [command] [service] [options]", "ky".bold());
	eprintln!();

	eprintln!("{}", "services".cyan().bold());
	eprintln!("  {} [name|--all]          Show status (default command)", "status".bold());
	eprintln!("  {} [name|--all] [-e]      Start service(s)", "start".bold());
	eprintln!("  {} [name|--all] [-e]       Stop service(s)", "stop".bold());
	eprintln!("  {} [name|--all] [-e]    Restart service(s) or a single process", "restart".bold());
	eprintln!();

	eprintln!("{}", "logs".cyan().bold());
	eprintln!("  {} <name> [process]        Show log file paths", "logs".bold());
	eprintln!("  {} <name> [process] [-n N]  Tail + stream live output", "echo".bold());
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
	eprintln!("  {} [start|stop|restart|status]   Manage the daemon", "daemon".bold());
	eprintln!("  {} [-d|--stop|--status]   HTTP server for web UI", "serve".bold());
	eprintln!("  {} [command]            macOS launchd agents", "launchd".bold());
	eprintln!("  {}                  Update to latest version", "self update".bold());
	eprintln!();

	eprintln!("{}", "output".cyan().bold());
	eprintln!("  {}                       Output as JSON", "--json".bold());
	eprintln!("  {}                        Output as TSV", "--tsv".bold());
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
			if output_format() == OutputFormat::Json {
				format::json_error(&format!("{}", e));
				std::process::exit(1);
			}
			eprintln!("error: {}", e);
			std::process::exit(1);
		}
	};

	match client.send(request) {
		Ok(resp) => resp,
		Err(e) => {
			if output_format() == OutputFormat::Json {
				format::json_error(&format!("{}", e));
				std::process::exit(1);
			}
			eprintln!("error: {}", e);
			std::process::exit(1);
		}
	}
}

// --- Commands that talk to daemon ---

fn cmd_status(args: &[String]) {
	let (watch, rest) = parse_watch_opts(args, None);
	if watch.enabled && !output_format().is_plain() {
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
	let rest: Vec<String> = rest.into_iter().filter(|a| !is_all_flag(a) && a != "--autostart").collect();

	if autostart_only {
		let names = config::autostart_project_names();
		if names.is_empty() {
			if plain {
				format::json_error("no projects with autostart = true");
			} else {
				eprintln!("no projects with autostart = true");
			}
			return;
		}
		let response = send_request(&Request::Start {
			names: names.clone(),
			all: true,
			processes: vec![],
		});
		handle_action_response(&response);
		return;
	}

	let args_for_resolve: Vec<String> = if start_all && rest.is_empty() {
		vec!["--all".to_string()]
	} else {
		rest.clone()
	};
	let (resolved, target_processes) = resolve_service_targets(&args_for_resolve, &entries);

	if resolved.is_empty() {
		eprintln!("no services to start");
		std::process::exit(1);
	}

	let response = send_request(&Request::Start {
		names: resolved.clone(),
		all: start_all || !target_processes.is_empty(),
		processes: target_processes,
	});

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
	let plain = output_format().is_plain();

	let stop_all = rest.iter().any(|a| is_all_flag(a));
	let rest: Vec<String> = rest.into_iter().filter(|a| !is_all_flag(a)).collect();

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

	let response = send_request(&Request::Stop {
		names: names.clone(),
		processes: target_processes,
	});

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
	let plain = output_format().is_plain();

	let restart_all = rest.iter().any(|a| is_all_flag(a));
	let rest: Vec<String> = rest.into_iter().filter(|a| !is_all_flag(a)).collect();

	if !watch.enabled && !plain {
		watch.enabled = true;
		watch.duration = Some(4);
	}

	// If --all or multiple services, do a full reload (stop+start all processes)
	if restart_all || rest.is_empty() || rest.len() > 1 {
		let (names, target_processes) = resolve_service_targets(&rest, &entries);
		if names.is_empty() {
			eprintln!("no services to restart");
			std::process::exit(1);
		}

		let response = send_request(&Request::Reload {
			names: names.clone(),
			all: restart_all,
			processes: target_processes,
		});

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
			watch_status(&names, &watch);
		}
		Response::Error { message } => {
			eprintln!("error: {}", message);
			std::process::exit(1);
		}
		_ => {}
	}
	return;
	}

	// Single target: could be "service" or "service process"
	let (service, process) = resolve_single_target(&rest, &entries);

	// No process name means restart all processes in the service
	if process.is_none() {
		let response = send_request(&Request::Reload {
			names: vec![service.clone()],
			all: false,
			processes: vec![],
		});

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
			watch_status(&[service], &watch);
		}
		Response::Error { message } => {
			eprintln!("error: {}", message);
			std::process::exit(1);
		}
		_ => {}
	}
	return;
	}

	// Restart a single process
	let process_name = process.unwrap();
	let response = send_request(&Request::Restart {
		service: service.clone(),
		process: process_name.clone(),
	});

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
			watch_status(&[service], &watch);
		}
		Response::Error { message } => {
			eprintln!("error: {}", message);
			std::process::exit(1);
		}
		_ => {}
	}
}

fn find_log_files(service: &str, process: &Option<String>) -> Vec<PathBuf> {
	let log_dir = logs::service_log_dir(service);
	if !log_dir.exists() {
		return Vec::new();
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
	files
}

fn tail_log_lines(service: &str, process: &Option<String>, n: usize) {
	let files = find_log_files(service, process);
	if files.is_empty() {
		return;
	}
	let latest = files.last().unwrap();
	let content = std::fs::read_to_string(latest).unwrap_or_default();
	let lines: Vec<&str> = content.lines().collect();
	let start = if lines.len() > n { lines.len() - n } else { 0 };
	for line in &lines[start..] {
		println!("{}", line);
	}
}

fn cmd_logs(args: &[String]) {
	let svc_entries = config::load_service_entries();
	let json = output_format() == OutputFormat::Json;

	let (service, process) = resolve_single_target(args, &svc_entries);

	let log_dir = logs::service_log_dir(&service);
	if !log_dir.exists() {
		eprintln!("no logs for {}", service);
		std::process::exit(1);
	}

	let files = find_log_files(&service, &process);

	if files.is_empty() {
		eprintln!("no log files found");
		std::process::exit(1);
	}

	if json {
		let paths: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
		format::json_value(&paths);
	} else {
		eprintln!("{}", log_dir.display().to_string().dimmed());
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

	// Tail last N lines from log file first
	tail_log_lines(&service, &process, tail_lines);

	// Stream live output from daemon
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
					if json {
						format::json_log_line(&line.trim_end(), new_offset);
					} else {
						print!("{}", line);
						let _ = io::stdout().flush();
					}
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
			Response::Error { .. } => break,
			_ => {}
		}

		std::thread::sleep(std::time::Duration::from_millis(100));
	}
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
			let projects_path = protocol::config_dir().join("projects");
			if json {
				let map: BTreeMap<&String, &PathBuf> = entries.iter().map(|(n, e)| (n, &e.dir)).collect();
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
		let services_path = service_entry.dir.join("services.toml");
		eprintln!("no services defined ({})", services_path.display());
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
	let json = output_format() == OutputFormat::Json || args.iter().any(|a| a == "--json");

	match subcmd {
		"status" | "st" => {
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
				Ok(msg) => {
					if json { format::json_ok(Some(msg)); } else { eprintln!("{}", msg); }
				}
				Err(e) => {
					if json { format::json_error(&e); } else { eprintln!("error: {}", e); }
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
					if json { format::json_ok(Some(msg)); } else { eprintln!("{}", msg); }
				}
				Err(e) => {
					if json { format::json_error(&e); } else { eprintln!("error: {}", e); }
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
					if json { format::json_ok(Some(msg)); } else { eprintln!("{}", msg); }
				}
				Err(e) => {
					if json { format::json_error(&e); } else { eprintln!("error: {}", e); }
					std::process::exit(1);
				}
			}
		}
		"reload" => {
			match koku_client::reload() {
				Ok(msg) => {
					if json { format::json_ok(Some(msg)); } else { eprintln!("{}", msg); }
				}
				Err(e) => {
					if json { format::json_error(&e); } else { eprintln!("error: {}", e); }
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
	let json = output_format() == OutputFormat::Json;

	match subcmd {
		"run" => {
			let daemon_args: Vec<String> = args[1..].to_vec();
			tokio::runtime::Runtime::new()
				.unwrap()
				.block_on(daemon::run(&daemon_args));
		}
		"start" => {
			if muzan::client::is_running(&paths) {
				if json { format::json_ok(Some("daemon already running".into())); }
				else { eprintln!("daemon already running"); }
				return;
			}
			let mut spawn_args: Vec<String> = vec!["daemon".to_string(), "run".to_string()];
			spawn_args.extend(args[1..].iter().cloned());
			let spawn_refs: Vec<&str> = spawn_args.iter().map(|s| s.as_str()).collect();
			let daemon = muzan::Daemon::new("kagaya");
			match daemon.start_background_with_args(&spawn_refs) {
				Ok(_) => {
					if json { format::json_ok(Some("daemon started".into())); }
					else { eprintln!("daemon started"); }
				}
				Err(e) => {
					if json { format::json_error(&format!("{}", e)); }
					else { eprintln!("error: {}", e); }
					std::process::exit(1);
				}
			}
		}
		"stop" => {
			let response = send_request(&Request::Shutdown);
			if json {
				handle_action_response(&response);
			} else {
				match response {
					Response::Ok { message } => {
						eprintln!("daemon: {}", message.unwrap_or_default());
					}
					_ => eprintln!("daemon not running"),
				}
			}
		}
		"status" => {
			if json {
				#[derive(serde::Serialize)]
				struct DaemonStatus { running: bool, pid: Option<u32> }
				let running = muzan::client::is_running(&paths);
				let pid = if running { muzan::client::read_pid(&paths) } else { None };
				format::json_value(&DaemonStatus { running, pid });
			} else if muzan::client::is_running(&paths) {
				if let Some(pid) = muzan::client::read_pid(&paths) {
					eprintln!("daemon running (pid {})", pid);
				} else {
					eprintln!("daemon running");
				}
			} else {
				eprintln!("daemon not running");
			}
		}
		"restart" => {
			// Stop if running
			if muzan::client::is_running(&paths) {
				let _ = send_request(&Request::Shutdown);
				// Wait for daemon to die (up to 3s)
				for _ in 0..30 {
					if !muzan::client::is_running(&paths) { break; }
					std::thread::sleep(std::time::Duration::from_millis(100));
				}
			}
			// Start
			let mut spawn_args: Vec<String> = vec!["daemon".to_string(), "run".to_string()];
			spawn_args.extend(args[1..].iter().cloned());
			let spawn_refs: Vec<&str> = spawn_args.iter().map(|s| s.as_str()).collect();
			let daemon = muzan::Daemon::new("kagaya");
			match daemon.start_background_with_args(&spawn_refs) {
				Ok(_) => {
					if json { format::json_ok(Some("daemon restarted".into())); }
					else { eprintln!("daemon restarted"); }
				}
				Err(e) => {
					if json { format::json_error(&format!("{}", e)); }
					else { eprintln!("error: {}", e); }
					std::process::exit(1);
				}
			}
		}
		_ => {
			eprintln!("usage: ky daemon [start|stop|restart|status|run]");
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
			if output_format() == OutputFormat::Json {
				format::json_error(&message);
			} else {
				eprintln!("error: {}", message);
			}
			std::process::exit(1);
		}
		_ => {
			eprintln!("unexpected response from daemon");
			std::process::exit(1);
		}
	}
}

struct StatusData {
	sorted_filter: Vec<String>,
	status_map: std::collections::HashMap<String, ServiceStatus>,
	process_filter: Option<String>,
	max_proc_name_width: usize,
	show_extras: bool,
	http_port: Option<u16>,
	cron_jobs: Option<Vec<koku::JobStatus>>,
}

fn gather_status_data(args: &[String]) -> StatusData {
	let (services, http_port) = fetch_status();
	let entries = config::load_service_entries();

	let show_all = args.iter().any(|a| is_all_flag(a));
	let current_project = get_current_project(&entries);

	let (filter, process_filter) = if args.is_empty() {
		let svcs = if let Some(ref current) = current_project {
			vec![current.clone()]
		} else {
			entries.keys().cloned().collect()
		};
		(svcs, None)
	} else if show_all {
		(entries.keys().cloned().collect(), None)
	} else {
		let (svcs, procs) = resolve_service_targets(args, &entries);
		let proc_filter = procs.into_iter().next();
		(svcs, proc_filter)
	};

	let mut status_map: std::collections::HashMap<String, ServiceStatus> =
		std::collections::HashMap::new();
	for s in services {
		status_map.insert(s.name.clone(), s);
	}

	let fmt = output_format();

	if fmt == OutputFormat::Json {
		let filtered: Vec<ServiceStatus> = filter.iter()
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
		let filtered: Vec<ServiceStatus> = filter.iter()
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
		.flat_map(|s| s.processes.iter().map(|p| p.name.len()))
		.max()
		.unwrap_or(0);

	let show_extras = show_all || (args.is_empty() && current_project.is_none());
	let cron_jobs = if show_extras { koku_client::fetch_status() } else { None };

	StatusData {
		sorted_filter,
		status_map,
		process_filter,
		max_proc_name_width,
		show_extras,
		http_port,
		cron_jobs,
	}
}

fn render_status(args: &[String]) -> usize {
	let data = gather_status_data(args);

	if let Some(ref proc_name) = data.process_filter {
		if let Some(name) = data.sorted_filter.first() {
			if let Some(status) = data.status_map.get(name) {
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

	let mut lines = 0usize;
	for name in &data.sorted_filter {
		let status = data.status_map.get(name);
		let running = status.map(|s| s.is_running()).unwrap_or(false);

		let symbol = if running { "●".green().to_string() } else { "◻".dimmed().to_string() };
		println!("{} {}", symbol, name.bold());
		lines += 1;

		if let Some(status) = status {
			for proc in &status.processes {
				print_process_line(proc, data.max_proc_name_width);
				lines += 1;
			}
		}
	}

	if data.show_extras {
		println!();
		lines += 1;
		if let Some(port) = data.http_port {
			println!("{} {}  http://127.0.0.1:{}", "●".green(), "serve".bold(), port);
		} else {
			println!("{} {}  not running", "○".dimmed(), "serve".bold());
		}
		lines += 1;

		if let Some(ref jobs) = data.cron_jobs {
			if !jobs.is_empty() {
				println!();
				lines += 1;

				let has_running = jobs.iter().any(|j| j.state == koku::JobState::Running);
				let symbol = if has_running { "●".green().to_string() } else { "○".dimmed().to_string() };
				println!("{} {}", symbol, "cron".bold());
				lines += 1;

				let max_name = jobs.iter().map(|j| j.name.len()).max().unwrap_or(0);

				for job in jobs {
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
			}
		}
	}

	lines
}

// --- Ratatui watch mode ---

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as RLine, Span};
use ratatui::widgets::Paragraph;

fn process_line_spans<'a>(proc: &ProcessStatus, name_width: usize) -> RLine<'a> {
	let green = Style::default().fg(Color::Green);
	let dim = Style::default().add_modifier(Modifier::DIM);
	let yellow = Style::default().fg(Color::Yellow);
	let red = Style::default().fg(Color::Red);

	let (symbol, label, extra) = match &proc.state {
		ProcessState::Running { pid, uptime_secs } => {
			let ports = if proc.ports.is_empty() {
				String::new()
			} else {
				proc.ports.iter().map(|p| format!(":{}", p)).collect::<Vec<_>>().join(",")
			};
			let extra = format!("{:<8} {:<8} {}", format_uptime(*uptime_secs), pid, ports);
			(Span::styled("●", green), Span::styled("on", green), extra)
		}
		ProcessState::Stopped if !proc.autostart => {
			(Span::styled("○", dim), Span::styled("optional", dim), String::new())
		}
		ProcessState::Stopped => {
			(Span::styled("◻", dim), Span::styled("off", dim), String::new())
		}
		ProcessState::Crashed { exit_code, retries } => {
			let extra = format!("exit {}  retry {}", exit_code, retries);
			(Span::styled("⚠", yellow), Span::styled("crashed", yellow), extra)
		}
		ProcessState::Failed { exit_code } => {
			let extra = format!("exit {}", exit_code);
			(Span::styled("✖", red), Span::styled("failed", red), extra)
		}
	};
	let extra_str = if extra.is_empty() { String::new() } else { format!("  {}", extra.trim_end()) };
	let padded_name = format!("{:<width$}", proc.name, width = name_width);
	RLine::from(vec![
		Span::raw("  "),
		symbol,
		Span::raw(" "),
		Span::raw(padded_name),
		Span::raw(" "),
		label,
		Span::raw(extra_str),
	])
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
						return vec![process_line_spans(proc, proc.name.len())];
					}
				}
			}
		}
		return vec![];
	}

	let mut lines: Vec<RLine> = Vec::new();

	for name in &data.sorted_filter {
		let status = data.status_map.get(name);
		let running = status.map(|s| s.is_running()).unwrap_or(false);

		let symbol = if running {
			Span::styled("●", green)
		} else {
			Span::styled("◻", dim)
		};
		lines.push(RLine::from(vec![symbol, Span::raw(" "), Span::styled(name.clone(), bold)]));

		if let Some(status) = status {
			for proc in &status.processes {
				lines.push(process_line_spans(proc, data.max_proc_name_width));
			}
		}
	}

	if data.show_extras {
		lines.push(RLine::from(""));
		if let Some(port) = data.http_port {
			lines.push(RLine::from(vec![
				Span::styled("●", green),
				Span::raw(" "),
				Span::styled("serve", bold),
				Span::raw(format!("  http://127.0.0.1:{}", port)),
			]));
		} else {
			lines.push(RLine::from(vec![
				Span::styled("○", dim),
				Span::raw(" "),
				Span::styled("serve", bold),
				Span::raw("  not running"),
			]));
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
				lines.push(RLine::from(vec![cron_sym, Span::raw(" "), Span::styled("cron", bold)]));

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
					let extra_str = if extra.is_empty() { String::new() } else { format!("  {}", extra) };
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

fn build_status_lines<'a>(args: &[String]) -> Vec<RLine<'a>> {
	status_data_to_lines(&gather_status_data(args))
}

fn watch_status(args: &[String], opts: &WatchOpts) {
	use crossterm::event::{self, Event, KeyCode, KeyModifiers};
	use crossterm::terminal;
	use ratatui::backend::CrosstermBackend;
	use ratatui::{Terminal, TerminalOptions, Viewport};
	use std::time::Duration;

	let start = Instant::now();

	let initial_lines = build_status_lines(args);
	let height = (initial_lines.len() as u16).max(1);

	terminal::enable_raw_mode().unwrap();
	let backend = CrosstermBackend::new(io::stdout());
	let mut term = Terminal::with_options(
		backend,
		TerminalOptions { viewport: Viewport::Inline(height) },
	).unwrap();

	loop {
		let lines = build_status_lines(args);
		let line_count = lines.len() as u16;
		if line_count != term.size().unwrap().height {
			term.resize(ratatui::layout::Rect::new(
				0, 0,
				term.size().unwrap().width,
				line_count.max(1),
			)).unwrap();
		}
		term.draw(|frame| {
			let text = ratatui::text::Text::from(lines);
			frame.render_widget(Paragraph::new(text), frame.area());
		}).unwrap();

		if let Some(duration) = opts.duration {
			if start.elapsed().as_secs() >= duration {
				break;
			}
		}

		if event::poll(Duration::from_secs(opts.interval)).unwrap() {
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

/// Resolve a list of CLI args into (service_names, process_names).
/// Handles: dot notation, known service names, bare process names via CWD, --all.
/// If args is empty, falls back to CWD project or errors.
fn resolve_service_targets(
	args: &[String],
	entries: &BTreeMap<String, ServiceEntry>,
) -> (Vec<String>, Vec<String>) {
	if args.is_empty() {
		if let Some(current) = get_current_project(entries) {
			return (vec![current], vec![]);
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
		return (entries.keys().cloned().collect(), vec![]);
	}

	let mut service_names: Vec<String> = Vec::new();
	let mut process_names: Vec<String> = Vec::new();

	for arg in args {
		if is_all_flag(arg) {
			continue;
		}
		let (svc, proc) = resolve_dot_target(arg, entries);
		if let Some(p) = proc {
			if !service_names.contains(&svc) {
				service_names.push(svc);
			}
			if !process_names.contains(&p) {
				process_names.push(p);
			}
		} else if entries.contains_key(&svc) {
			if !service_names.contains(&svc) {
				service_names.push(svc);
			}
		} else if let Some(current) = get_current_project(entries) {
			if !service_names.contains(&current) {
				service_names.push(current);
			}
			if !process_names.contains(&svc) {
				process_names.push(svc);
			}
		} else {
			eprintln!("unknown service: {}", svc);
			eprintln!("registered services: {}", entries.keys().cloned().collect::<Vec<_>>().join(", "));
			std::process::exit(1);
		}
	}

	(service_names, process_names)
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
	eprintln!("registered services: {}", entries.keys().cloned().collect::<Vec<_>>().join(", "));
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
		let max_w = services.iter()
			.flat_map(|s| s.processes.iter().map(|p| p.name.len()))
			.max()
			.unwrap_or(0);
		let mut map = std::collections::HashMap::new();
		for s in services {
			map.insert(s.name.clone(), s);
		}
		StatusData {
			sorted_filter: names,
			status_map: map,
			process_filter: None,
			max_proc_name_width: max_w,
			show_extras: false,
			http_port: None,
			cron_jobs: None,
		}
	}

	fn span_text(line: &ratatui::text::Line) -> String {
		line.spans.iter().map(|s| s.content.as_ref()).collect::<String>()
	}

	fn find_span<'a>(line: &'a ratatui::text::Line, content: &str) -> Option<&'a Span<'a>> {
		line.spans.iter().find(|s| s.content.as_ref() == content)
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
		assert_eq!(format_uptime(192), "3m12s");
		assert_eq!(format_uptime(300), "5m");
		assert_eq!(format_uptime(3599), "59m59s");
	}

	#[test]
	fn uptime_hours() {
		assert_eq!(format_uptime(3600), "1h");
		assert_eq!(format_uptime(9000), "2h30m");
		assert_eq!(format_uptime(86399), "23h59m");
	}

	#[test]
	fn uptime_days() {
		assert_eq!(format_uptime(86400), "1d");
		assert_eq!(format_uptime(104400), "1d5h");
		assert_eq!(format_uptime(172800), "2d");
	}

	// --- process_line_spans: running ---

	#[test]
	fn spans_running_basic() {
		let proc = test_proc("web", ProcessState::Running { pid: 1234, uptime_secs: 65 });
		let line = process_line_spans(&proc, 10);
		let text = span_text(&line);

		assert!(text.starts_with("  "));
		assert!(text.contains("●"));
		assert!(text.contains("on"));
		assert!(text.contains("1m5s"));
		assert!(text.contains("1234"));

		let dot = find_span(&line, "●").unwrap();
		assert_eq!(dot.style, Style::default().fg(Color::Green));
		let on = find_span(&line, "on").unwrap();
		assert_eq!(on.style, Style::default().fg(Color::Green));
	}

	#[test]
	fn spans_running_with_ports() {
		let mut proc = test_proc("web", ProcessState::Running { pid: 99, uptime_secs: 5 });
		proc.ports = vec![3000, 3001];
		let line = process_line_spans(&proc, 5);
		let text = span_text(&line);

		assert!(text.contains(":3000,:3001"));
	}

	#[test]
	fn spans_running_name_padding() {
		let proc = test_proc("web", ProcessState::Running { pid: 1, uptime_secs: 0 });
		let line = process_line_spans(&proc, 10);
		let text = span_text(&line);
		// "web" padded to 10 chars
		assert!(text.contains("web       "));
	}

	// --- process_line_spans: stopped ---

	#[test]
	fn spans_stopped_autostart() {
		let proc = test_proc("worker", ProcessState::Stopped);
		let line = process_line_spans(&proc, 8);
		let text = span_text(&line);

		assert!(text.contains("◻"));
		assert!(text.contains("off"));
		// No extra info for stopped
		assert!(!text.contains("exit"));

		let sq = find_span(&line, "◻").unwrap();
		assert_eq!(sq.style, Style::default().add_modifier(Modifier::DIM));
	}

	#[test]
	fn spans_stopped_optional() {
		let mut proc = test_proc("optional-svc", ProcessState::Stopped);
		proc.autostart = false;
		let line = process_line_spans(&proc, 12);
		let text = span_text(&line);

		assert!(text.contains("○"));
		assert!(text.contains("optional"));

		let circle = find_span(&line, "○").unwrap();
		assert_eq!(circle.style, Style::default().add_modifier(Modifier::DIM));
	}

	// --- process_line_spans: crashed ---

	#[test]
	fn spans_crashed() {
		let proc = test_proc("api", ProcessState::Crashed { exit_code: 137, retries: 2 });
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

	// --- status_data_to_lines: service headers ---

	#[test]
	fn lines_running_service_header() {
		let svc = test_service("myapp", vec![
			test_proc("web", ProcessState::Running { pid: 1, uptime_secs: 10 }),
		]);
		let data = test_status_data(vec![svc]);
		let lines = status_data_to_lines(&data);

		assert_eq!(lines.len(), 2); // header + 1 process
		let header = &lines[0];
		let header_text = span_text(header);
		assert!(header_text.contains("●"));
		assert!(header_text.contains("myapp"));

		let dot = find_span(header, "●").unwrap();
		assert_eq!(dot.style, Style::default().fg(Color::Green));
		let name = find_span(header, "myapp").unwrap();
		assert_eq!(name.style, Style::default().add_modifier(Modifier::BOLD));
	}

	#[test]
	fn lines_stopped_service_header() {
		let svc = test_service("myapp", vec![
			test_proc("web", ProcessState::Stopped),
		]);
		let data = test_status_data(vec![svc]);
		let lines = status_data_to_lines(&data);

		let header = &lines[0];
		let dot = find_span(header, "◻").unwrap();
		assert_eq!(dot.style, Style::default().add_modifier(Modifier::DIM));
	}

	// --- status_data_to_lines: multiple services ---

	#[test]
	fn lines_multiple_services() {
		let svc1 = test_service("alpha", vec![
			test_proc("web", ProcessState::Running { pid: 1, uptime_secs: 0 }),
			test_proc("worker", ProcessState::Stopped),
		]);
		let svc2 = test_service("beta", vec![
			test_proc("api", ProcessState::Failed { exit_code: 1 }),
		]);
		let data = test_status_data(vec![svc1, svc2]);
		let lines = status_data_to_lines(&data);

		// alpha header + web + worker + beta header + api = 5
		assert_eq!(lines.len(), 5);
		assert!(span_text(&lines[0]).contains("alpha"));
		assert!(span_text(&lines[3]).contains("beta"));
	}

	#[test]
	fn lines_name_width_consistent_across_services() {
		let svc1 = test_service("a", vec![
			test_proc("short", ProcessState::Stopped),
		]);
		let svc2 = test_service("b", vec![
			test_proc("very-long-name", ProcessState::Stopped),
		]);
		let data = test_status_data(vec![svc1, svc2]);
		let lines = status_data_to_lines(&data);

		// Both process lines should pad to max_proc_name_width = 14 ("very-long-name")
		let short_line = span_text(&lines[1]);
		// "short" should be padded to 14 chars
		assert!(short_line.contains("short         "));
	}

	// --- status_data_to_lines: show_extras ---

	#[test]
	fn lines_show_extras_with_http() {
		let mut data = test_status_data(vec![]);
		data.show_extras = true;
		data.http_port = Some(13369);
		let lines = status_data_to_lines(&data);

		// empty line + serve line = 2
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
		assert!(serve_text.contains("not running"));

		let circle = find_span(&lines[1], "○").unwrap();
		assert_eq!(circle.style, Style::default().add_modifier(Modifier::DIM));
	}

	// --- status_data_to_lines: process_filter ---

	#[test]
	fn lines_process_filter_found() {
		let svc = test_service("myapp", vec![
			test_proc("web", ProcessState::Running { pid: 1, uptime_secs: 0 }),
			test_proc("worker", ProcessState::Stopped),
		]);
		let mut data = test_status_data(vec![svc]);
		data.process_filter = Some("worker".to_string());
		let lines = status_data_to_lines(&data);

		assert_eq!(lines.len(), 1);
		assert!(span_text(&lines[0]).contains("off"));
	}

	#[test]
	fn lines_process_filter_not_found() {
		let svc = test_service("myapp", vec![
			test_proc("web", ProcessState::Running { pid: 1, uptime_secs: 0 }),
		]);
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
		// has_running = true, so green dot
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
		data.cron_jobs = Some(vec![
			koku::JobStatus {
				name: "cleanup".to_string(),
				state: koku::JobState::Paused,
				last_run: None,
				last_exit: Some(0),
				next_run: None,
			},
		]);
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
					if nc == 'm' { break; }
				}
			} else {
				result.push(c);
			}
		}
		result
	}

	fn capture_print_process_line(proc: &ProcessStatus, width: usize) -> String {
		// Reproduce print_process_line logic without println
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
		format!("  {} {:<w$} {}{}", symbol, proc.name, label, extra_str, w = width)
	}

	#[test]
	fn parity_running() {
		let proc = test_proc("web", ProcessState::Running { pid: 42, uptime_secs: 3661 });
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
		let proc = test_proc("api", ProcessState::Crashed { exit_code: 1, retries: 3 });
		let println_out = strip_ansi(&capture_print_process_line(&proc, 5));
		let ratatui_out = span_text(&process_line_spans(&proc, 5));
		assert_eq!(println_out, ratatui_out);
	}

	#[test]
	fn parity_failed() {
		let proc = test_proc("bg", ProcessState::Failed { exit_code: 127 });
		let println_out = strip_ansi(&capture_print_process_line(&proc, 4));
		let ratatui_out = span_text(&process_line_spans(&proc, 4));
		assert_eq!(println_out, ratatui_out);
	}

	#[test]
	fn parity_running_with_ports() {
		let mut proc = test_proc("web", ProcessState::Running { pid: 100, uptime_secs: 30 });
		proc.ports = vec![8080, 8443];
		let println_out = strip_ansi(&capture_print_process_line(&proc, 6));
		let ratatui_out = span_text(&process_line_spans(&proc, 6));
		assert_eq!(println_out, ratatui_out);
	}
}
