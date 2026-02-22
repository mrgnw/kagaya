use owo_colors::OwoColorize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

const KAGAYA_PREFIX: &str = "com.kagaya.";

// --- Public entry point ---

pub fn cmd_launchd(args: &[String]) {
	let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
	let rest = if args.is_empty() { &[] as &[String] } else { &args[1..] };

	match subcmd {
		"help" | "--help" | "-h" => print_launchd_usage(),
		"list" | "ls" => cmd_list(rest),
		"status" | "st" => cmd_status(rest),
		"start" => cmd_start(rest),
		"stop" => cmd_stop(rest),
		"restart" => cmd_restart(rest),
		"logs" => cmd_logs(rest),
		"show" => cmd_show(rest),
		"create" => cmd_create(rest),
		"edit" => cmd_edit(rest),
		"remove" | "rm" => cmd_remove(rest),
		"alias" => cmd_alias(),
		label => {
			cmd_status(&[label.to_string()]);
		}
	}
}

fn print_launchd_usage() {
	eprintln!("kagaya launchd — manage macOS launchd agents");
	eprintln!();
	eprintln!("usage: ky launchd [command] [options]");
	eprintln!("alias: ky lctl, or shell alias: alias lctl='ky launchd'");
	eprintln!();
	eprintln!("commands:");
	eprintln!("  list [--all] [--global]       List agents (default: user plist agents)");
	eprintln!("  status [label]               Show agent status details");
	eprintln!("  start <label>                Start / load agent");
	eprintln!("  stop <label>                 Stop / unload agent");
	eprintln!("  restart <label>              Restart agent");
	eprintln!("  logs <label>                 Show agent log files");
	eprintln!("  show <label>                 Show plist contents");
	eprintln!("  create <label> -- <cmd>      Create a new agent plist");
	eprintln!("  edit <label>                 Open plist in $EDITOR");
	eprintln!("  remove <label> [--yes]       Unload and delete agent plist");
	eprintln!("  alias                        Print shell alias command");
	eprintln!();
	eprintln!("options:");
	eprintln!("  --all, -a                    Include all loaded agents (not just plist files)");
	eprintln!("  --global, -g                 Include /Library agents (read-only)");
	eprintln!();
	eprintln!("labels can be partial: 'lctl restart opencode' matches 'com.opencode.serve'");
}

// --- Data types ---

#[derive(Debug, Clone)]
struct AgentInfo {
	label: String,
	plist_path: Option<PathBuf>,
	pid: Option<u32>,
	exit_code: Option<i32>,
	loaded: bool,
	domain: AgentDomain,
	program: Option<String>,
	keep_alive: bool,
	run_at_load: bool,
	stdout_path: Option<String>,
	stderr_path: Option<String>,
	working_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum AgentDomain {
	UserAgent,
	GlobalAgent,
	GlobalDaemon,
}

impl AgentDomain {
	fn display(&self) -> &str {
		match self {
			AgentDomain::UserAgent => "user",
			AgentDomain::GlobalAgent => "global",
			AgentDomain::GlobalDaemon => "system",
		}
	}
}

// --- Display helpers ---

fn short_label(label: &str) -> String {
	let parts: Vec<&str> = label.split('.').collect();
	if parts.len() <= 1 {
		return label.to_string();
	}

	let start = if matches!(parts[0], "com" | "org" | "io" | "net" | "homebrew") {
		1
	} else {
		0
	};
	let rest = &parts[start..];

	match rest.len() {
		0 => label.to_string(),
		1 => rest[0].to_string(),
		2 => {
			let vendor = rest[0].to_lowercase();
			let next = rest[1].to_lowercase();
			let noise = matches!(vendor.as_str(), "user" | "mxcl");
			if noise || next.starts_with(&vendor) || vendor == next {
				rest[1].to_string()
			} else {
				rest.join(".")
			}
		}
		_ => rest[1..].join("."),
	}
}

use crate::utils::format_uptime;

fn get_uptimes(pids: &[u32]) -> HashMap<u32, u64> {
	if pids.is_empty() {
		return HashMap::new();
	}
	let pid_args: Vec<String> = pids.iter().map(|p| p.to_string()).collect();
	let output = match Command::new("ps")
		.arg("-o")
		.arg("etime=,pid=")
		.arg("-p")
		.arg(pid_args.join(","))
		.output()
	{
		Ok(o) => o,
		Err(_) => return HashMap::new(),
	};
	let stdout = String::from_utf8_lossy(&output.stdout);
	let mut map = HashMap::new();
	for line in stdout.lines() {
		let line = line.trim();
		if line.is_empty() {
			continue;
		}
		let parts: Vec<&str> = line.split_whitespace().collect();
		if parts.len() < 2 {
			continue;
		}
		let etime = parts[0];
		let pid: u32 = match parts[1].parse() {
			Ok(p) => p,
			Err(_) => continue,
		};
		if let Some(secs) = parse_etime(etime) {
			map.insert(pid, secs);
		}
	}
	map
}

fn parse_etime(etime: &str) -> Option<u64> {
	// Formats: MM:SS, HH:MM:SS, D-HH:MM:SS
	let (days, rest) = if let Some(pos) = etime.find('-') {
		let d: u64 = etime[..pos].parse().ok()?;
		(d, &etime[pos + 1..])
	} else {
		(0, etime)
	};
	let parts: Vec<&str> = rest.split(':').collect();
	match parts.len() {
		2 => {
			let m: u64 = parts[0].parse().ok()?;
			let s: u64 = parts[1].parse().ok()?;
			Some(days * 86400 + m * 60 + s)
		}
		3 => {
			let h: u64 = parts[0].parse().ok()?;
			let m: u64 = parts[1].parse().ok()?;
			let s: u64 = parts[2].parse().ok()?;
			Some(days * 86400 + h * 3600 + m * 60 + s)
		}
		_ => None,
	}
}

use crate::utils::listening_ports_for_pids;

// --- Discovery ---

fn get_uid() -> u32 {
	Command::new("id")
		.arg("-u")
		.output()
		.ok()
		.and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
		.unwrap_or(501)
}

fn user_agents_dir() -> PathBuf {
	let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
	PathBuf::from(home).join("Library").join("LaunchAgents")
}

fn launchd_log_dir() -> PathBuf {
	let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
	PathBuf::from(home)
		.join(".local")
		.join("state")
		.join("kagaya")
		.join("launchd")
}

fn plist_dirs(include_global: bool) -> Vec<(PathBuf, AgentDomain)> {
	let mut dirs = vec![(user_agents_dir(), AgentDomain::UserAgent)];
	if include_global {
		dirs.push((
			PathBuf::from("/Library/LaunchAgents"),
			AgentDomain::GlobalAgent,
		));
		dirs.push((
			PathBuf::from("/Library/LaunchDaemons"),
			AgentDomain::GlobalDaemon,
		));
	}
	dirs
}

fn parse_launchctl_list() -> BTreeMap<String, (Option<u32>, Option<i32>)> {
	let mut map = BTreeMap::new();
	let output = match Command::new("launchctl").arg("list").output() {
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

fn scan_plists(include_global: bool, include_all_loaded: bool) -> BTreeMap<String, AgentInfo> {
	let mut agents: BTreeMap<String, AgentInfo> = BTreeMap::new();
	let loaded = parse_launchctl_list();

	for (dir, domain) in plist_dirs(include_global) {
		if !dir.exists() {
			continue;
		}
		let entries = match std::fs::read_dir(&dir) {
			Ok(e) => e,
			Err(_) => continue,
		};
		for entry in entries.flatten() {
			let path = entry.path();
			if path.extension().and_then(|e| e.to_str()) != Some("plist") {
				continue;
			}
			if let Some(info) = parse_plist_file(&path, &domain, &loaded) {
				agents.insert(info.label.clone(), info);
			}
		}
	}

	if include_all_loaded {
		// Add loaded agents that don't have a plist in the scanned dirs
		for (label, (pid, exit_code)) in &loaded {
			if !agents.contains_key(label) {
				agents.insert(
					label.clone(),
					AgentInfo {
						label: label.clone(),
						plist_path: None,
						pid: *pid,
						exit_code: *exit_code,
						loaded: true,
						domain: AgentDomain::UserAgent,
						program: None,
						keep_alive: false,
						run_at_load: false,
						stdout_path: None,
						stderr_path: None,
						working_dir: None,
					},
				);
			}
		}
	}

	agents
}

fn parse_plist_file(
	path: &Path,
	domain: &AgentDomain,
	loaded: &BTreeMap<String, (Option<u32>, Option<i32>)>,
) -> Option<AgentInfo> {
	let value = plist::Value::from_file(path).ok()?;
	let dict = value.as_dictionary()?;

	let label = dict
		.get("Label")
		.and_then(|v| v.as_string())
		.map(|s| s.to_string())?;

	let (pid, exit_code, is_loaded) = match loaded.get(&label) {
		Some((pid, exit)) => (*pid, *exit, true),
		None => (None, None, false),
	};

	let program = dict
		.get("Program")
		.and_then(|v| v.as_string())
		.map(|s| s.to_string())
		.or_else(|| {
			dict.get("ProgramArguments")
				.and_then(|v| v.as_array())
				.map(|arr| {
					arr.iter()
						.filter_map(|v| v.as_string())
						.collect::<Vec<_>>()
						.join(" ")
				})
		});

	let keep_alive = dict
		.get("KeepAlive")
		.and_then(|v| v.as_boolean())
		.unwrap_or(false);

	let run_at_load = dict
		.get("RunAtLoad")
		.and_then(|v| v.as_boolean())
		.unwrap_or(false);

	let stdout_path = dict
		.get("StandardOutPath")
		.and_then(|v| v.as_string())
		.map(|s| s.to_string());

	let stderr_path = dict
		.get("StandardErrorPath")
		.and_then(|v| v.as_string())
		.map(|s| s.to_string());

	let working_dir = dict
		.get("WorkingDirectory")
		.and_then(|v| v.as_string())
		.map(|s| s.to_string());

	Some(AgentInfo {
		label,
		plist_path: Some(path.to_path_buf()),
		pid,
		exit_code,
		loaded: is_loaded,
		domain: domain.clone(),
		program,
		keep_alive,
		run_at_load,
		stdout_path,
		stderr_path,
		working_dir,
	})
}

fn resolve_label(partial: &str, agents: &BTreeMap<String, AgentInfo>) -> Option<String> {
	// Exact match first
	if agents.contains_key(partial) {
		return Some(partial.to_string());
	}
	// Try with kagaya prefix
	let prefixed = format!("{}{}", KAGAYA_PREFIX, partial);
	if agents.contains_key(&prefixed) {
		return Some(prefixed);
	}
	// Substring match (if unique)
	let matches: Vec<&String> = agents
		.keys()
		.filter(|k| k.contains(partial))
		.collect();
	if matches.len() == 1 {
		return Some(matches[0].clone());
	}
	// Segment match: check if any dot-segment of a label matches the partial
	let partial_lower = partial.to_lowercase();
	let seg_matches: Vec<&String> = agents
		.keys()
		.filter(|k| {
			k.split('.').any(|seg| seg.to_lowercase() == partial_lower)
		})
		.collect();
	if seg_matches.len() == 1 {
		return Some(seg_matches[0].clone());
	}
	// Multiple segment matches — prefer agents with plist files (not dynamic/Apple agents)
	if seg_matches.len() > 1 {
		let with_plist: Vec<&&String> = seg_matches
			.iter()
			.filter(|k| agents[k.as_str()].plist_path.is_some())
			.collect();
		if with_plist.len() == 1 {
			return Some(with_plist[0].to_string());
		}
	}
	None
}

fn print_not_found(partial: &str, agents: &BTreeMap<String, AgentInfo>) {
	eprintln!("agent not found: {}", partial);
	let partial_lower = partial.to_lowercase();

	// Find suggestions: substring or segment match
	let suggestions: Vec<&String> = agents
		.keys()
		.filter(|k| {
			k.to_lowercase().contains(&partial_lower)
				|| k.split('.').any(|seg| seg.to_lowercase().contains(&partial_lower))
		})
		.collect();

	if suggestions.len() == 1 {
		eprintln!("did you mean: {}?", suggestions[0]);
	} else if suggestions.len() > 1 && suggestions.len() <= 5 {
		eprintln!(
			"did you mean: {}?",
			suggestions.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
		);
	}

	let kagaya_agents: Vec<&String> = agents
		.keys()
		.filter(|k| k.starts_with(KAGAYA_PREFIX))
		.collect();
	if !kagaya_agents.is_empty() && suggestions.is_empty() {
		eprintln!(
			"kagaya agents: {}",
			kagaya_agents
				.iter()
				.map(|s| s.strip_prefix(KAGAYA_PREFIX).unwrap_or(s))
				.collect::<Vec<_>>()
				.join(", ")
		);
	}
}

fn find_plist_path(label: &str) -> Option<PathBuf> {
	for (dir, _) in plist_dirs(true) {
		if !dir.exists() {
			continue;
		}
		let entries = match std::fs::read_dir(&dir) {
			Ok(e) => e,
			Err(_) => continue,
		};
		for entry in entries.flatten() {
			let path = entry.path();
			if path.extension().and_then(|e| e.to_str()) != Some("plist") {
				continue;
			}
			if let Ok(value) = plist::Value::from_file(&path) {
				if let Some(dict) = value.as_dictionary() {
					if let Some(l) = dict.get("Label").and_then(|v| v.as_string()) {
						if l == label {
							return Some(path);
						}
					}
				}
			}
		}
	}
	None
}

// --- Commands ---

fn cmd_list(args: &[String]) {
	let include_global = args.iter().any(|a| a == "--global" || a == "-g");
	let include_all = args.iter().any(|a| a == "--all" || a == "-a");
	let agents = scan_plists(include_global, include_all);

	if agents.is_empty() {
		eprintln!("no agents found");
		return;
	}

	let running_pids: Vec<u32> = agents.values().filter_map(|a| a.pid).collect();
	let uptimes = get_uptimes(&running_pids);
	let ports = listening_ports_for_pids(&running_pids);

	let max_name_width = agents
		.values()
		.map(|a| short_label(&a.label).len())
		.max()
		.unwrap_or(0);

	for agent in agents.values() {
		let short = short_label(&agent.label);

		let (symbol, status_text, extra) = if let Some(pid) = agent.pid {
			let port_str = ports
				.get(&pid)
				.map(|ps| ps.iter().map(|p| format!(":{}", p)).collect::<Vec<_>>().join(","))
				.unwrap_or_default();
			let uptime_str = uptimes
				.get(&pid)
				.map(|s| format_uptime(*s))
				.unwrap_or_default();
			let extra = format!(
				"{:<8} {:<8} {}",
				uptime_str, pid, port_str,
			);
			("●".green().to_string(), "on".green().to_string(), extra)
		} else if agent.loaded {
			match agent.exit_code {
				Some(code) if code != 0 => {
					let extra = format!("exit {}", code);
					("⚠".yellow().to_string(), "crashed".yellow().to_string(), extra)
				}
				Some(code) => {
					("◻".dimmed().to_string(), format!("exit {}", code).dimmed().to_string(), String::new())
				}
				None => {
					("◻".dimmed().to_string(), "loaded".dimmed().to_string(), String::new())
				}
			}
		} else {
			("◻".dimmed().to_string(), "not loaded".dimmed().to_string(), String::new())
		};

		let domain_tag = if agent.domain != AgentDomain::UserAgent {
			format!(" {}", format!("[{}]", agent.domain.display()).dimmed())
		} else {
			String::new()
		};

		let name_display = if agent.pid.is_some() {
			if let Some(dot) = short.rfind('.') {
				let prefix = &short[..dot + 1];
				let suffix = &short[dot + 1..];
				format!(
					"{}{}",
					prefix.to_string().dimmed(),
					suffix.to_string().green(),
				)
			} else {
				short.green().to_string()
			}
		} else {
			if let Some(dot) = short.rfind('.') {
				let prefix = &short[..dot + 1];
				let suffix = &short[dot + 1..];
				format!(
					"{}{}",
					prefix.to_string().dimmed(),
					suffix,
				)
			} else {
				short.to_string()
			}
		};

		let extra_str = if extra.is_empty() {
			String::new()
		} else {
			format!("  {}", extra.trim_end())
		};

		// Pad the name using the plain short label length
		let padding = max_name_width.saturating_sub(short.len());
		let pad = " ".repeat(padding);

		println!(
			" {} {}{} {}{}{}",
			symbol, name_display, pad, status_text, extra_str, domain_tag,
		);
	}
}

fn cmd_status(args: &[String]) {
	if args.is_empty() {
		cmd_list(&[]);
		return;
	}

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];
	let circle = if agent.pid.is_some() {
		"●".green().to_string()
	} else if agent.loaded {
		match agent.exit_code {
			Some(c) if c != 0 => "⚠".yellow().to_string(),
			_ => "◻".dimmed().to_string(),
		}
	} else {
		"◻".dimmed().to_string()
	};

	println!(" {} {}", circle, agent.label.bold());
	println!();

	if let Some(ref path) = agent.plist_path {
		println!("   {:>12} {}", "plist:".dimmed(), path.display());
	}
	if let Some(ref prog) = agent.program {
		println!("   {:>12} {}", "command:".dimmed(), prog);
	}
	if let Some(pid) = agent.pid {
		println!("   {:>12} {}", "pid:".dimmed(), pid);
		let uptimes = get_uptimes(&[pid]);
		if let Some(secs) = uptimes.get(&pid) {
			println!("   {:>12} {}", "uptime:".dimmed(), format_uptime(*secs));
		}
		let ports = listening_ports_for_pids(&[pid]);
		if let Some(ps) = ports.get(&pid) {
			let port_str = ps.iter().map(|p| format!(":{}", p)).collect::<Vec<_>>().join(" ");
			println!("   {:>12} {}", "ports:".dimmed(), port_str);
		}
	}
	if let Some(exit) = agent.exit_code {
		let exit_display = if exit != 0 {
			format!("{}", exit).yellow().to_string()
		} else {
			format!("{}", exit)
		};
		println!("   {:>12} {}", "exit code:".dimmed(), exit_display);
	}
	println!(
		"   {:>12} {}",
		"loaded:".dimmed(),
		if agent.loaded { "yes" } else { "no" }
	);
	println!(
		"   {:>12} {}",
		"keep alive:".dimmed(),
		if agent.keep_alive { "yes" } else { "no" }
	);
	println!(
		"   {:>12} {}",
		"run at load:".dimmed(),
		if agent.run_at_load { "yes" } else { "no" }
	);
	if let Some(ref dir) = agent.working_dir {
		println!("   {:>12} {}", "workdir:".dimmed(), dir);
	}
	if let Some(ref p) = agent.stdout_path {
		println!("   {:>12} {}", "stdout:".dimmed(), p);
	}
	if let Some(ref p) = agent.stderr_path {
		println!("   {:>12} {}", "stderr:".dimmed(), p);
	}
	println!("   {:>12} {}", "domain:".dimmed(), agent.domain.display());
}

fn cmd_start(args: &[String]) {
	if args.is_empty() {
		eprintln!("usage: ky launchd start <label>");
		std::process::exit(1);
	}

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];

	if agent.domain != AgentDomain::UserAgent {
		eprintln!("warning: managing {} agents may require sudo", agent.domain.display());
	}

	let uid = get_uid();

	if agent.loaded {
		// Already loaded — kickstart it
		let target = format!("gui/{}/{}", uid, label);
		let result = Command::new("launchctl")
			.args(["kickstart", "-kp", &target])
			.output();
		match result {
			Ok(output) if output.status.success() => {
				eprintln!("{}: started (kickstart)", label);
			}
			Ok(output) => {
				let err = String::from_utf8_lossy(&output.stderr);
				eprintln!("{}: kickstart failed: {}", label, err.trim());
				std::process::exit(1);
			}
			Err(e) => {
				eprintln!("error: {}", e);
				std::process::exit(1);
			}
		}
	} else {
		// Not loaded — bootstrap it
		let plist_path = agent
			.plist_path
			.as_ref()
			.or_else(|| find_plist_path(&label).as_ref().map(|_| unreachable!()))
			.cloned()
			.unwrap_or_else(|| {
				eprintln!("{}: no plist file found", label);
				std::process::exit(1);
			});

		let target = format!("gui/{}", uid);
		let result = Command::new("launchctl")
			.args(["bootstrap", &target, &plist_path.to_string_lossy()])
			.output();
		match result {
			Ok(output) if output.status.success() => {
				eprintln!("{}: loaded and started", label);
			}
			Ok(output) => {
				let err = String::from_utf8_lossy(&output.stderr);
				// Fall back to legacy load
				let legacy = Command::new("launchctl")
					.args(["load", &plist_path.to_string_lossy()])
					.output();
				match legacy {
					Ok(o) if o.status.success() => {
						eprintln!("{}: loaded (legacy)", label);
					}
					_ => {
						eprintln!("{}: bootstrap failed: {}", label, err.trim());
						std::process::exit(1);
					}
				}
			}
			Err(e) => {
				eprintln!("error: {}", e);
				std::process::exit(1);
			}
		}
	}
}

fn cmd_stop(args: &[String]) {
	if args.is_empty() {
		eprintln!("usage: ky launchd stop <label>");
		std::process::exit(1);
	}

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];

	if !agent.loaded {
		eprintln!("{}: not loaded", label);
		return;
	}

	if agent.domain != AgentDomain::UserAgent {
		eprintln!("warning: managing {} agents may require sudo", agent.domain.display());
	}

	let uid = get_uid();

	// Try bootout first (fully unloads)
	let plist_path = agent
		.plist_path
		.as_ref()
		.map(|p| p.to_string_lossy().to_string());

	let target = format!("gui/{}/{}", uid, label);
	let result = Command::new("launchctl")
		.args(["bootout", &target])
		.output();

	match result {
		Ok(output) if output.status.success() => {
			eprintln!("{}: stopped and unloaded", label);
		}
		_ => {
			// Fall back: try kill, then legacy unload
			let _ = Command::new("launchctl")
				.args(["kill", "SIGTERM", &target])
				.output();

			if let Some(ref path) = plist_path {
				let _ = Command::new("launchctl")
					.args(["unload", path])
					.output();
			}
			eprintln!("{}: stopped", label);
		}
	}
}

fn cmd_restart(args: &[String]) {
	if args.is_empty() {
		eprintln!("usage: ky launchd restart <label>");
		std::process::exit(1);
	}

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];

	if agent.domain != AgentDomain::UserAgent {
		eprintln!("warning: managing {} agents may require sudo", agent.domain.display());
	}

	let uid = get_uid();

	if agent.loaded {
		// kickstart with -k (kill existing) and -p (print pid)
		let target = format!("gui/{}/{}", uid, label);
		let result = Command::new("launchctl")
			.args(["kickstart", "-kp", &target])
			.output();
		match result {
			Ok(output) if output.status.success() => {
				let out = String::from_utf8_lossy(&output.stdout);
				eprintln!("{}: restarted {}", label, out.trim());
			}
			Ok(output) => {
				let err = String::from_utf8_lossy(&output.stderr);
				eprintln!("{}: restart failed: {}", label, err.trim());
				std::process::exit(1);
			}
			Err(e) => {
				eprintln!("error: {}", e);
				std::process::exit(1);
			}
		}
	} else {
		// Not loaded — just start it
		cmd_start(args);
	}
}

fn cmd_logs(args: &[String]) {
	if args.is_empty() {
		eprintln!("usage: ky launchd logs <label>");
		std::process::exit(1);
	}

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];
	let mut log_files: Vec<PathBuf> = Vec::new();

	if let Some(ref p) = agent.stdout_path {
		let path = PathBuf::from(p);
		if path.exists() {
			log_files.push(path);
		}
	}
	if let Some(ref p) = agent.stderr_path {
		let path = PathBuf::from(p);
		if path.exists() && !log_files.iter().any(|f| f == &path) {
			log_files.push(path);
		}
	}

	if log_files.is_empty() {
		// Fall back to unified log
		eprintln!("no log files configured, querying system log...");
		eprintln!();
		let result = Command::new("log")
			.args([
				"show",
				"--predicate",
				&format!("subsystem == \"{}\" OR senderImagePath CONTAINS \"{}\"", label, label),
				"--last",
				"5m",
				"--style",
				"compact",
			])
			.output();
		match result {
			Ok(output) => {
				let text = String::from_utf8_lossy(&output.stdout);
				if text.trim().is_empty() || text.lines().count() <= 1 {
					eprintln!("no recent log entries found for {}", label);
				} else {
					print!("{}", text);
				}
			}
			Err(e) => {
				eprintln!("error querying log: {}", e);
				std::process::exit(1);
			}
		}
		return;
	}

	for log_file in &log_files {
		if log_files.len() > 1 {
			println!("{}", log_file.display().dimmed());
			println!();
		}
		let content = match std::fs::read_to_string(log_file) {
			Ok(c) => c,
			Err(e) => {
				eprintln!("error reading {}: {}", log_file.display(), e);
				continue;
			}
		};
		let lines: Vec<&str> = content.lines().collect();
		let start = if lines.len() > 100 { lines.len() - 100 } else { 0 };
		for line in &lines[start..] {
			println!("{}", line);
		}
	}
}

fn cmd_show(args: &[String]) {
	if args.is_empty() {
		eprintln!("usage: ky launchd show <label>");
		std::process::exit(1);
	}

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];

	let plist_path = match &agent.plist_path {
		Some(p) => p.clone(),
		None => {
			eprintln!("{}: no plist file on disk", label);
			std::process::exit(1);
		}
	};

	println!("{}", plist_path.display().dimmed());
	println!();

	let content = match std::fs::read_to_string(&plist_path) {
		Ok(c) => c,
		Err(e) => {
			eprintln!("error reading plist: {}", e);
			std::process::exit(1);
		}
	};

	// Syntax-highlight the XML plist
	for line in content.lines() {
		let trimmed = line.trim();
		if trimmed.starts_with("<?xml") || trimmed.starts_with("<!DOCTYPE") {
			println!("{}", line.dimmed());
		} else if trimmed.starts_with("<key>") {
			// Highlight key names
			if let (Some(start), Some(end)) = (trimmed.find("<key>"), trimmed.find("</key>")) {
				let key = &trimmed[start + 5..end];
				let indent = &line[..line.len() - trimmed.len()];
				print!("{}{}{}", indent, "<key>".dimmed(), key.cyan());
				let rest = &trimmed[end..];
				println!("{}", rest.dimmed());
			} else {
				println!("{}", line);
			}
		} else if trimmed == "<true/>" {
			let indent = &line[..line.len() - trimmed.len()];
			println!("{}{}", indent, "<true/>".green());
		} else if trimmed == "<false/>" {
			let indent = &line[..line.len() - trimmed.len()];
			println!("{}{}", indent, "<false/>".red());
		} else if trimmed.starts_with("<string>") {
			if let (Some(start), Some(end)) = (trimmed.find("<string>"), trimmed.find("</string>"))
			{
				let val = &trimmed[start + 8..end];
				let indent = &line[..line.len() - trimmed.len()];
				print!("{}{}{}", indent, "<string>".dimmed(), val.yellow());
				println!("{}", "</string>".dimmed());
			} else {
				println!("{}", line);
			}
		} else if trimmed.starts_with("<integer>") {
			if let (Some(start), Some(end)) =
				(trimmed.find("<integer>"), trimmed.find("</integer>"))
			{
				let val = &trimmed[start + 9..end];
				let indent = &line[..line.len() - trimmed.len()];
				print!("{}{}{}", indent, "<integer>".dimmed(), val.yellow());
				println!("{}", "</integer>".dimmed());
			} else {
				println!("{}", line);
			}
		} else {
			println!("{}", line.dimmed());
		}
	}
}

fn cmd_create(args: &[String]) {
	// Parse: create <label> [options] -- <command...>
	if args.is_empty() {
		eprintln!("usage: ky launchd create <label> [options] -- <command...>");
		eprintln!();
		eprintln!("options:");
		eprintln!("  --dir <path>           Working directory (default: current dir)");
		eprintln!("  --no-keep-alive        Don't restart on crash");
		eprintln!("  --no-run-at-load       Don't start on load/login");
		eprintln!("  --env KEY=VAL          Set environment variable (repeatable)");
		std::process::exit(1);
	}

	let label_short = &args[0];
	let label = if label_short.contains('.') {
		label_short.clone()
	} else {
		format!("{}{}", KAGAYA_PREFIX, label_short)
	};

	// Find the -- separator
	let separator_pos = args.iter().position(|a| a == "--");
	let (option_args, command_args) = match separator_pos {
		Some(pos) => (&args[1..pos], &args[pos + 1..]),
		None => {
			eprintln!("error: missing -- separator before command");
			eprintln!("usage: ky launchd create {} -- <command...>", label_short);
			std::process::exit(1);
		}
	};

	if command_args.is_empty() {
		eprintln!("error: no command specified after --");
		std::process::exit(1);
	}

	// Parse options
	let mut working_dir = std::env::current_dir()
		.unwrap_or_else(|_| PathBuf::from("/tmp"))
		.to_string_lossy()
		.to_string();
	let mut keep_alive = true;
	let mut run_at_load = true;
	let mut env_vars: Vec<(String, String)> = Vec::new();

	let mut i = 0;
	while i < option_args.len() {
		match option_args[i].as_str() {
			"--dir" => {
				i += 1;
				if i < option_args.len() {
					working_dir = option_args[i].clone();
				}
			}
			"--no-keep-alive" => keep_alive = false,
			"--no-run-at-load" => run_at_load = false,
			"--env" => {
				i += 1;
				if i < option_args.len() {
					if let Some((k, v)) = option_args[i].split_once('=') {
						env_vars.push((k.to_string(), v.to_string()));
					}
				}
			}
			other => {
				eprintln!("unknown option: {}", other);
				std::process::exit(1);
			}
		}
		i += 1;
	}

	// Check if plist already exists
	let agents_dir = user_agents_dir();
	let _ = std::fs::create_dir_all(&agents_dir);
	let plist_path = agents_dir.join(format!("{}.plist", label));

	if plist_path.exists() {
		eprintln!("error: plist already exists: {}", plist_path.display());
		eprintln!("use 'ky launchd edit {}' to modify, or 'ky launchd remove {}' first", label_short, label_short);
		std::process::exit(1);
	}

	// Create log directory
	let log_dir = launchd_log_dir();
	let _ = std::fs::create_dir_all(&log_dir);
	let stdout_log = log_dir.join(format!("{}.out.log", label_short));
	let stderr_log = log_dir.join(format!("{}.err.log", label_short));

	// Build plist dictionary
	let mut dict = plist::Dictionary::new();
	dict.insert("Label".to_string(), plist::Value::String(label.clone()));

	let program_args: Vec<plist::Value> = command_args
		.iter()
		.map(|s| plist::Value::String(s.clone()))
		.collect();
	dict.insert(
		"ProgramArguments".to_string(),
		plist::Value::Array(program_args),
	);

	dict.insert(
		"WorkingDirectory".to_string(),
		plist::Value::String(working_dir),
	);
	dict.insert(
		"KeepAlive".to_string(),
		plist::Value::Boolean(keep_alive),
	);
	dict.insert(
		"RunAtLoad".to_string(),
		plist::Value::Boolean(run_at_load),
	);
	dict.insert(
		"StandardOutPath".to_string(),
		plist::Value::String(stdout_log.to_string_lossy().to_string()),
	);
	dict.insert(
		"StandardErrorPath".to_string(),
		plist::Value::String(stderr_log.to_string_lossy().to_string()),
	);

	if !env_vars.is_empty() {
		let mut env_dict = plist::Dictionary::new();
		for (k, v) in &env_vars {
			env_dict.insert(k.clone(), plist::Value::String(v.clone()));
		}
		dict.insert(
			"EnvironmentVariables".to_string(),
			plist::Value::Dictionary(env_dict),
		);
	}

	// Write plist
	let value = plist::Value::Dictionary(dict);
	if let Err(e) = value.to_file_xml(&plist_path) {
		eprintln!("error writing plist: {}", e);
		std::process::exit(1);
	}
	eprintln!("created {}", plist_path.display());

	// Bootstrap it
	let uid = get_uid();
	let target = format!("gui/{}", uid);
	let result = Command::new("launchctl")
		.args(["bootstrap", &target, &plist_path.to_string_lossy()])
		.output();
	match result {
		Ok(output) if output.status.success() => {
			eprintln!("{}: loaded and started", label);
		}
		Ok(output) => {
			let err = String::from_utf8_lossy(&output.stderr);
			// Try legacy load
			let legacy = Command::new("launchctl")
				.args(["load", &plist_path.to_string_lossy()])
				.output();
			match legacy {
				Ok(o) if o.status.success() => {
					eprintln!("{}: loaded (legacy)", label);
				}
				_ => {
					eprintln!("created plist but failed to load: {}", err.trim());
					eprintln!("try: launchctl load {}", plist_path.display());
				}
			}
		}
		Err(e) => {
			eprintln!("created plist but failed to load: {}", e);
			eprintln!("try: launchctl load {}", plist_path.display());
		}
	}
}

fn cmd_edit(args: &[String]) {
	if args.is_empty() {
		eprintln!("usage: ky launchd edit <label>");
		std::process::exit(1);
	}

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];
	let plist_path = match &agent.plist_path {
		Some(p) => p.clone(),
		None => {
			eprintln!("{}: no plist file on disk", label);
			std::process::exit(1);
		}
	};

	if agent.domain != AgentDomain::UserAgent {
		eprintln!("warning: editing {} agent — may need sudo to save", agent.domain.display());
	}

	let editor = std::env::var("EDITOR").unwrap_or_else(|_| "open -e".to_string());

	let mtime_before = std::fs::metadata(&plist_path)
		.and_then(|m| m.modified())
		.ok();

	let parts: Vec<&str> = editor.split_whitespace().collect();
	let status = Command::new(parts[0])
		.args(&parts[1..])
		.arg(&plist_path)
		.status();

	match status {
		Ok(s) if s.success() => {
			let mtime_after = std::fs::metadata(&plist_path)
				.and_then(|m| m.modified())
				.ok();
			if mtime_before != mtime_after {
				eprintln!("plist modified. reload agent? [Y/n] ");
				let mut input = String::new();
				if std::io::stdin().read_line(&mut input).is_ok() {
					let input = input.trim().to_lowercase();
					if input.is_empty() || input == "y" || input == "yes" {
						let uid = get_uid();
						let target = format!("gui/{}/{}", uid, label);
						let _ = Command::new("launchctl")
							.args(["bootout", &target])
							.output();
						std::thread::sleep(std::time::Duration::from_millis(500));
						let target = format!("gui/{}", uid);
						let _ = Command::new("launchctl")
							.args(["bootstrap", &target, &plist_path.to_string_lossy()])
							.output();
						eprintln!("{}: reloaded", label);
					}
				}
			}
		}
		Ok(_) => {
			eprintln!("editor exited with error");
		}
		Err(e) => {
			eprintln!("failed to open editor: {}", e);
			eprintln!("set $EDITOR or try: open {}", plist_path.display());
			std::process::exit(1);
		}
	}
}

fn cmd_remove(args: &[String]) {
	if args.is_empty() {
		eprintln!("usage: ky launchd remove <label> [--yes]");
		std::process::exit(1);
	}

	let force = args.iter().any(|a| a == "--yes" || a == "-y");

	let agents = scan_plists(true, true);
	let label = match resolve_label(&args[0], &agents) {
		Some(l) => l,
		None => {
			print_not_found(&args[0], &agents);
			std::process::exit(1);
		}
	};

	let agent = &agents[&label];

	if !label.starts_with(KAGAYA_PREFIX) && !force {
		eprintln!(
			"refusing to remove non-kagaya agent: {}",
			label
		);
		eprintln!("use --yes to force removal");
		std::process::exit(1);
	}

	let plist_path = match &agent.plist_path {
		Some(p) => p.clone(),
		None => {
			eprintln!("{}: no plist file on disk (only loaded in memory)", label);
			if agent.loaded {
				eprintln!("to unload: launchctl bootout gui/{}/{}", get_uid(), label);
			}
			std::process::exit(1);
		}
	};

	if !force {
		eprintln!("remove {} ?", label);
		eprintln!("  plist: {}", plist_path.display());
		eprint!("  confirm [y/N]: ");
		let mut input = String::new();
		if std::io::stdin().read_line(&mut input).is_ok() {
			let input = input.trim().to_lowercase();
			if input != "y" && input != "yes" {
				eprintln!("cancelled");
				return;
			}
		} else {
			eprintln!("cancelled");
			return;
		}
	}

	// Unload if loaded
	if agent.loaded {
		let uid = get_uid();
		let target = format!("gui/{}/{}", uid, label);
		let _ = Command::new("launchctl")
			.args(["bootout", &target])
			.output();
		eprintln!("{}: unloaded", label);
	}

	// Delete plist
	match std::fs::remove_file(&plist_path) {
		Ok(_) => eprintln!("{}: plist removed", label),
		Err(e) => {
			eprintln!("error removing {}: {}", plist_path.display(), e);
			std::process::exit(1);
		}
	}
}

fn cmd_alias() {
	let shell = detect_shell();
	let rc_file = shell_rc_file(&shell);
	let alias_line = "alias lctl='ky launchd'";

	println!("echo \"{}\" >> {}", alias_line, rc_file);
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
