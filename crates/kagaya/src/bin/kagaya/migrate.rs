use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::protocol;

struct ProcEntry {
	name: String,
	command: String,
	is_task: bool,
}

fn parse_procfile(content: &str) -> Vec<ProcEntry> {
	let mut entries = Vec::new();
	for line in content.lines() {
		let line = line.trim();
		if line.is_empty() {
			continue;
		}
		// Task: #~ name: command
		if let Some(rest) = line.strip_prefix("#~") {
			let rest = rest.trim();
			if let Some((name, cmd)) = rest.split_once(':') {
				entries.push(ProcEntry {
					name: name.trim().to_string(),
					command: cmd.trim().to_string(),
					is_task: true,
				});
			}
			continue;
		}
		// Skip comments
		if line.starts_with('#') {
			continue;
		}
		// Regular: name: command
		if let Some((name, cmd)) = line.split_once(':') {
			entries.push(ProcEntry {
				name: name.trim().to_string(),
				command: cmd.trim().to_string(),
				is_task: false,
			});
		}
	}
	entries
}

fn parse_old_projects(content: &str) -> Vec<(String, String)> {
	let mut projects = Vec::new();
	for line in content.lines() {
		let line = line.trim();
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		if let Some((name, dir)) = line.split_once(':') {
			projects.push((name.trim().to_string(), dir.trim().to_string()));
		}
	}
	projects
}

fn parse_old_commands(content: &str) -> Vec<(String, String)> {
	parse_old_projects(content)
}

fn generate_projects_toml(
	projects: &[(String, String)],
	commands: &[(String, String)],
) -> String {
	let mut out = String::new();
	for (name, dir) in projects {
		out.push_str(&format!("{} = {:?}\n", name, dir));
	}
	if !commands.is_empty() {
		if !out.is_empty() {
			out.push('\n');
		}
		for (name, cmd) in commands {
			out.push_str(&format!("[{}]\nrun = {:?}\n\n", name, cmd));
		}
	}
	out
}

fn generate_services_toml(entries: &[ProcEntry]) -> String {
	let mut out = String::new();
	for entry in entries {
		if entry.is_task {
			out.push_str(&format!("[{}]\nrun = {:?}\ntype = \"task\"\n\n", entry.name, entry.command));
		} else {
			out.push_str(&format!("{} = {:?}\n", entry.name, entry.command));
		}
	}
	out
}

fn ubermind_config_dir() -> PathBuf {
	if let Ok(home) = std::env::var("HOME") {
		PathBuf::from(home).join(".config").join("ubermind")
	} else {
		PathBuf::from("/tmp/.config/ubermind")
	}
}

fn expand_tilde(path: &str) -> PathBuf {
	if let Some(rest) = path.strip_prefix("~/") {
		if let Ok(home) = std::env::var("HOME") {
			return PathBuf::from(home).join(rest);
		}
	}
	PathBuf::from(path)
}

pub fn cmd_migrate(force: bool) {
	let old_dir = ubermind_config_dir();
	let new_dir = protocol::config_dir();

	// Read old projects file
	let old_projects_path = old_dir.join("projects");
	let projects = if old_projects_path.exists() {
		match std::fs::read_to_string(&old_projects_path) {
			Ok(content) => parse_old_projects(&content),
			Err(e) => {
				eprintln!("error reading {}: {}", old_projects_path.display(), e);
				return;
			}
		}
	} else {
		eprintln!("no ubermind config found at {}", old_projects_path.display());
		return;
	};

	// Read old commands file
	let old_commands_path = old_dir.join("commands");
	let commands = if old_commands_path.exists() {
		match std::fs::read_to_string(&old_commands_path) {
			Ok(content) => parse_old_commands(&content),
			Err(e) => {
				eprintln!("warning: failed to read {}: {}", old_commands_path.display(), e);
				Vec::new()
			}
		}
	} else {
		Vec::new()
	};

	if projects.is_empty() && commands.is_empty() {
		eprintln!("nothing to migrate");
		return;
	}

	// Create kagaya config dir
	if let Err(e) = std::fs::create_dir_all(&new_dir) {
		eprintln!("error creating {}: {}", new_dir.display(), e);
		return;
	}

	// Write projects.toml
	let projects_toml_path = new_dir.join("projects.toml");
	if projects_toml_path.exists() && !force {
		eprintln!("skip: {} already exists (use --force to overwrite)", projects_toml_path.display());
	} else {
		let content = generate_projects_toml(&projects, &commands);
		match std::fs::write(&projects_toml_path, &content) {
			Ok(_) => eprintln!("wrote {}", projects_toml_path.display()),
			Err(e) => eprintln!("error writing {}: {}", projects_toml_path.display(), e),
		}
	}

	// Migrate Procfiles -> services.toml for each project
	let mut migrated = 0usize;
	let mut skipped = 0usize;
	let mut missing = 0usize;

	// Collect project dirs with resolved paths
	let project_dirs: BTreeMap<String, PathBuf> = projects
		.iter()
		.map(|(name, dir)| (name.clone(), expand_tilde(dir)))
		.collect();

	for (name, dir) in &project_dirs {
		let procfile_path = dir.join("Procfile");
		let services_toml_path = dir.join("services.toml");

		if !dir.exists() {
			eprintln!("skip: {} — dir not found: {}", name, dir.display());
			missing += 1;
			continue;
		}

		if !procfile_path.exists() {
			continue;
		}

		if services_toml_path.exists() && !force {
			eprintln!("skip: {} — services.toml already exists (use --force)", name);
			skipped += 1;
			continue;
		}

		let content = match std::fs::read_to_string(&procfile_path) {
			Ok(c) => c,
			Err(e) => {
				eprintln!("error reading {}: {}", procfile_path.display(), e);
				continue;
			}
		};

		let entries = parse_procfile(&content);
		if entries.is_empty() {
			continue;
		}

		let toml_content = generate_services_toml(&entries);
		match std::fs::write(&services_toml_path, &toml_content) {
			Ok(_) => {
				let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
				let task_count = entries.iter().filter(|e| e.is_task).count();
				let suffix = if task_count > 0 {
					format!(" ({} task{})", task_count, if task_count == 1 { "" } else { "s" })
				} else {
					String::new()
				};
				eprintln!("wrote {} — {}{}", services_toml_path.display(), names.join(", "), suffix);
				migrated += 1;
			}
			Err(e) => eprintln!("error writing {}: {}", services_toml_path.display(), e),
		}
	}

	eprintln!();
	eprintln!(
		"migrated {} project{}, {} skipped, {} missing",
		migrated,
		if migrated == 1 { "" } else { "s" },
		skipped,
		missing,
	);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_procfile_regular() {
		let content = "web: npm run dev\napi: python main.py\n";
		let entries = parse_procfile(content);
		assert_eq!(entries.len(), 2);
		assert_eq!(entries[0].name, "web");
		assert_eq!(entries[0].command, "npm run dev");
		assert!(!entries[0].is_task);
		assert_eq!(entries[1].name, "api");
		assert_eq!(entries[1].command, "python main.py");
	}

	#[test]
	fn test_parse_procfile_with_tasks() {
		let content = "dev: bun run dev\n#~ tauri: cargo tauri dev\n";
		let entries = parse_procfile(content);
		assert_eq!(entries.len(), 2);
		assert_eq!(entries[0].name, "dev");
		assert!(!entries[0].is_task);
		assert_eq!(entries[1].name, "tauri");
		assert_eq!(entries[1].command, "cargo tauri dev");
		assert!(entries[1].is_task);
	}

	#[test]
	fn test_parse_procfile_comments_and_blanks() {
		let content = "# comment\n\nweb: npm start\n\n# another comment\n";
		let entries = parse_procfile(content);
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].name, "web");
	}

	#[test]
	fn test_parse_procfile_command_with_colons() {
		let content = "server: ./bin/app -c config/app.toml\n";
		let entries = parse_procfile(content);
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].command, "./bin/app -c config/app.toml");
	}

	#[test]
	fn test_parse_old_projects() {
		let content = "# name: dir\nappligator: ~/dev/appligator\nmatrix: /Users/m/dev/matrix\n";
		let projects = parse_old_projects(content);
		assert_eq!(projects.len(), 2);
		assert_eq!(projects[0], ("appligator".into(), "~/dev/appligator".into()));
		assert_eq!(projects[1], ("matrix".into(), "/Users/m/dev/matrix".into()));
	}

	#[test]
	fn test_generate_projects_toml() {
		let projects = vec![
			("appligator".into(), "~/dev/appligator".into()),
			("mors".into(), "~/dev/mors".into()),
		];
		let commands = vec![
			("tunnel".into(), "ssh -N -L 5432:localhost:5432 myserver".into()),
		];
		let toml = generate_projects_toml(&projects, &commands);
		assert!(toml.contains("appligator = \"~/dev/appligator\""));
		assert!(toml.contains("mors = \"~/dev/mors\""));
		assert!(toml.contains("[tunnel]"));
		assert!(toml.contains("run = \"ssh -N -L 5432:localhost:5432 myserver\""));
	}

	#[test]
	fn test_generate_services_toml_simple() {
		let entries = vec![
			ProcEntry { name: "web".into(), command: "npm run dev".into(), is_task: false },
			ProcEntry { name: "api".into(), command: "python main.py".into(), is_task: false },
		];
		let toml = generate_services_toml(&entries);
		assert!(toml.contains("web = \"npm run dev\""));
		assert!(toml.contains("api = \"python main.py\""));
	}

	#[test]
	fn test_generate_services_toml_with_task() {
		let entries = vec![
			ProcEntry { name: "dev".into(), command: "bun run dev".into(), is_task: false },
			ProcEntry { name: "tauri".into(), command: "cargo tauri dev".into(), is_task: true },
		];
		let toml = generate_services_toml(&entries);
		assert!(toml.contains("dev = \"bun run dev\""));
		assert!(toml.contains("[tauri]"));
		assert!(toml.contains("run = \"cargo tauri dev\""));
		assert!(toml.contains("type = \"task\""));
	}

	#[test]
	fn test_parse_procfile_env_in_command() {
		let content = "api: cd api && UVICORN_PORT=8040 uv run python main.py\n";
		let entries = parse_procfile(content);
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].command, "cd api && UVICORN_PORT=8040 uv run python main.py");
	}

	#[test]
	fn test_generate_projects_toml_no_commands() {
		let projects = vec![("app".into(), "~/dev/app".into())];
		let toml = generate_projects_toml(&projects, &[]);
		assert_eq!(toml, "app = \"~/dev/app\"\n");
		assert!(!toml.contains('['));
	}

	#[test]
	fn test_parse_empty() {
		assert!(parse_procfile("").is_empty());
		assert!(parse_procfile("# just comments\n\n").is_empty());
		assert!(parse_old_projects("").is_empty());
		assert!(parse_old_commands("  \n").is_empty());
	}
}
