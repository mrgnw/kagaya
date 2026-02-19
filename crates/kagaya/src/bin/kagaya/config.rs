use crate::protocol::config_dir;
use kagaya::{ProcessDef, Service, ServiceType};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

// ── Global config (~/.config/kagaya/config.toml) ────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GlobalConfig {
	#[serde(default)]
	pub daemon: DaemonConfig,
	#[serde(default)]
	pub logs: LogsConfig,
	#[serde(default)]
	pub defaults: DefaultsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonConfig {
	#[serde(default = "default_idle_timeout")]
	#[allow(dead_code)]
	pub idle_timeout: u64,
	#[allow(dead_code)]
	pub log_dir: Option<String>,
	#[serde(default = "default_port")]
	pub port: u16,
}

impl Default for DaemonConfig {
	fn default() -> Self {
		Self { idle_timeout: default_idle_timeout(), log_dir: None, port: default_port() }
	}
}

fn default_idle_timeout() -> u64 { 300 }
fn default_port() -> u16 { 13369 }

#[derive(Debug, Clone, Deserialize)]
pub struct LogsConfig {
	#[serde(default = "default_max_size")]
	pub max_size_bytes: u64,
	#[serde(default = "default_max_age_days")]
	pub max_age_days: u32,
	#[serde(default = "default_max_files")]
	pub max_files: u32,
}

impl Default for LogsConfig {
	fn default() -> Self {
		Self {
			max_size_bytes: default_max_size(),
			max_age_days: default_max_age_days(),
			max_files: default_max_files(),
		}
	}
}

fn default_max_size() -> u64 { 10 * 1024 * 1024 }
fn default_max_age_days() -> u32 { 7 }
fn default_max_files() -> u32 { 5 }

#[derive(Debug, Clone, Deserialize)]
pub struct DefaultsConfig {
	#[serde(default = "default_true")]
	pub restart: bool,
	#[serde(default = "default_max_retries")]
	pub max_retries: u32,
	#[serde(default = "default_restart_delay")]
	pub restart_delay: u64,
	#[serde(default = "default_env")]
	pub env: HashMap<String, String>,
}

impl Default for DefaultsConfig {
	fn default() -> Self {
		Self {
			restart: true,
			max_retries: default_max_retries(),
			restart_delay: default_restart_delay(),
			env: default_env(),
		}
	}
}

fn default_true() -> bool { true }
fn default_max_retries() -> u32 { 3 }
fn default_restart_delay() -> u64 { 1 }
fn default_env() -> HashMap<String, String> {
	let mut env = HashMap::new();
	env.insert("FORCE_COLOR".into(), "1".into());
	env.insert("CLICOLOR_FORCE".into(), "1".into());
	env
}

pub fn load_global_config() -> GlobalConfig {
	let path = config_dir().join("config.toml");
	if path.exists() {
		match std::fs::read_to_string(&path) {
			Ok(content) => match toml::from_str(&content) {
				Ok(config) => return config,
				Err(e) => eprintln!("warning: failed to parse {}: {}", path.display(), e),
			},
			Err(e) => eprintln!("warning: failed to read {}: {}", path.display(), e),
		}
	}
	GlobalConfig::default()
}

// ── services.toml format ─────────────────────────────────────────────────────

/// A single service definition — either a bare command string or a full table.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ServiceDef {
	Simple(String),
	Full {
		run: String,
		#[serde(default, rename = "type")]
		service_type: ServiceType,
		restart: Option<bool>,
		max_retries: Option<u32>,
		restart_delay: Option<u64>,
		#[serde(default)]
		env: HashMap<String, String>,
		autostart: Option<bool>,
	},
}

impl ServiceDef {
	fn into_process_def(self, name: String, defaults: &DefaultsConfig) -> ProcessDef {
		match self {
			ServiceDef::Simple(cmd) => ProcessDef {
				name,
				command: cmd,
				service_type: ServiceType::Service,
				restart: defaults.restart,
				max_retries: defaults.max_retries,
				restart_delay_secs: defaults.restart_delay,
				env: defaults.env.clone(),
				autostart: true,
			},
			ServiceDef::Full { run, service_type, restart, max_retries, restart_delay, env, autostart } => {
				let is_task = service_type == ServiceType::Task;
				let mut merged_env = defaults.env.clone();
				merged_env.extend(env);
				ProcessDef {
					name,
					command: run,
					service_type,
					restart: restart.unwrap_or(if is_task { false } else { defaults.restart }),
					max_retries: max_retries.unwrap_or(defaults.max_retries),
					restart_delay_secs: restart_delay.unwrap_or(defaults.restart_delay),
					env: merged_env,
					autostart: autostart.unwrap_or(!is_task),
				}
			}
		}
	}
}

// ── projects.toml format ──────────────────────────────────────────────────────

/// An entry in projects.toml — either a directory path or a standalone command.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ProjectDef {
	Dir(String),
	DirTable {
		dir: String,
		#[serde(default)]
		autostart: bool,
	},
	Command {
		run: String,
		#[serde(default, rename = "type")]
		service_type: ServiceType,
		restart: Option<bool>,
		max_retries: Option<u32>,
		restart_delay: Option<u64>,
		#[serde(default)]
		env: HashMap<String, String>,
		#[serde(default)]
		autostart: bool,
	},
}

// ── ServiceEntry: resolved project ready for the daemon ──────────────────────

pub struct ServiceEntry {
	pub name: String,
	pub dir: PathBuf,
	/// Set for standalone commands (no services.toml in dir)
	pub inline_command: Option<InlineCommand>,
	/// Whether this project should be started on boot (via `ky autostart`)
	pub autostart: bool,
}

pub struct InlineCommand {
	pub run: String,
	pub service_type: ServiceType,
	pub restart: Option<bool>,
	pub max_retries: Option<u32>,
	pub restart_delay: Option<u64>,
	pub env: HashMap<String, String>,
}

// ── Loading projects ──────────────────────────────────────────────────────────

pub fn load_projects() -> BTreeMap<String, ServiceEntry> {
	let path = config_dir().join("projects.toml");
	let mut services = BTreeMap::new();

	let content = match std::fs::read_to_string(&path) {
		Ok(c) => c,
		Err(_) => return services,
	};

	let raw: BTreeMap<String, toml::Value> = match toml::from_str(&content) {
		Ok(v) => v,
		Err(e) => {
			eprintln!("warning: failed to parse {}: {}", path.display(), e);
			return services;
		}
	};

	for (name, value) in raw {
		let def: ProjectDef = match value.try_into() {
			Ok(d) => d,
			Err(e) => {
				eprintln!("warning: skipping '{}' in projects.toml: {}", name, e);
				continue;
			}
		};

		match def {
			ProjectDef::Dir(dir_str) => {
				let dir = expand_tilde(&dir_str);
				if !dir.exists() {
					eprintln!("warning: directory does not exist for {}: {}", name, dir.display());
					continue;
				}
				services.insert(name.clone(), ServiceEntry { name, dir, inline_command: None, autostart: false });
			}
			ProjectDef::DirTable { dir: dir_str, autostart } => {
				let dir = expand_tilde(&dir_str);
				if !dir.exists() {
					eprintln!("warning: directory does not exist for {}: {}", name, dir.display());
					continue;
				}
				services.insert(name.clone(), ServiceEntry { name, dir, inline_command: None, autostart });
			}
			ProjectDef::Command { run, service_type, restart, max_retries, restart_delay, env, autostart } => {
				// Standalone commands get a synthetic dir under ~/.config/kagaya/_commands/
				let dir = config_dir().join("_commands").join(&name);
				let _ = std::fs::create_dir_all(&dir);
				services.insert(
					name.clone(),
					ServiceEntry {
						name,
						dir,
						inline_command: Some(InlineCommand {
							run,
							service_type,
							restart,
							max_retries,
							restart_delay,
							env,
						}),
						autostart,
					},
				);
			}
		}
	}

	services
}

pub fn load_service_entries() -> BTreeMap<String, ServiceEntry> {
	load_projects()
}

pub fn autostart_project_names() -> Vec<String> {
	load_projects()
		.into_iter()
		.filter(|(_, entry)| entry.autostart)
		.map(|(name, _)| name)
		.collect()
}

// ── Loading a service (processes) from a ServiceEntry ────────────────────────

pub fn load_service(entry: &ServiceEntry, defaults: &DefaultsConfig) -> Service {
	// Inline command (standalone task from projects.toml)
	if let Some(ref cmd) = entry.inline_command {
		let is_task = cmd.service_type == ServiceType::Task;
		let mut env = defaults.env.clone();
		env.extend(cmd.env.clone());
		let proc = ProcessDef {
			name: entry.name.clone(),
			command: cmd.run.clone(),
			service_type: cmd.service_type.clone(),
			restart: cmd.restart.unwrap_or(if is_task { false } else { defaults.restart }),
			max_retries: cmd.max_retries.unwrap_or(defaults.max_retries),
			restart_delay_secs: cmd.restart_delay.unwrap_or(defaults.restart_delay),
			env,
			autostart: !is_task,
		};
		return Service { name: entry.name.clone(), dir: entry.dir.clone(), processes: vec![proc] };
	}

	// Project with services.toml
	let services_path = entry.dir.join("services.toml");
	let content = match std::fs::read_to_string(&services_path) {
		Ok(c) => c,
		Err(_) => {
			return Service { name: entry.name.clone(), dir: entry.dir.clone(), processes: vec![] };
		}
	};

	let raw: BTreeMap<String, toml::Value> = match toml::from_str(&content) {
		Ok(v) => v,
		Err(e) => {
			eprintln!("warning: failed to parse {}: {}", services_path.display(), e);
			return Service { name: entry.name.clone(), dir: entry.dir.clone(), processes: vec![] };
		}
	};

	let processes = raw
		.into_iter()
		.filter_map(|(name, value)| {
			let def: ServiceDef = match value.try_into() {
				Ok(d) => d,
				Err(e) => {
					eprintln!("warning: skipping '{}' in services.toml: {}", name, e);
					return None;
				}
			};
			Some(def.into_process_def(name, defaults))
		})
		.collect();

	Service { name: entry.name.clone(), dir: entry.dir.clone(), processes }
}

fn expand_tilde(path: &str) -> PathBuf {
	if let Some(rest) = path.strip_prefix("~/") {
		if let Ok(home) = std::env::var("HOME") {
			return PathBuf::from(home).join(rest);
		}
	}
	PathBuf::from(path)
}
