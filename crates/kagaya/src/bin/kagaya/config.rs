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
        Self {
            idle_timeout: default_idle_timeout(),
            log_dir: None,
            port: default_port(),
        }
    }
}

fn default_idle_timeout() -> u64 {
    300
}
fn default_port() -> u16 {
    13369
}

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

fn default_max_size() -> u64 {
    10 * 1024 * 1024
}
fn default_max_age_days() -> u32 {
    7
}
fn default_max_files() -> u32 {
    5
}

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

fn default_true() -> bool {
    true
}
fn default_max_retries() -> u32 {
    3
}
fn default_restart_delay() -> u64 {
    1
}
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
        pre_start: Option<String>,
        #[serde(default)]
        ports: Vec<u16>,
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
                pre_start: None,
                ports: Vec::new(),
            },
            ServiceDef::Full {
                run,
                service_type,
                restart,
                max_retries,
                restart_delay,
                env,
                autostart,
                pre_start,
                ports,
            } => {
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
                    pre_start,
                    ports,
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
                    eprintln!(
                        "warning: directory does not exist for {}: {}",
                        name,
                        dir.display()
                    );
                    continue;
                }
                services.insert(
                    name.clone(),
                    ServiceEntry {
                        name,
                        dir,
                        inline_command: None,
                        autostart: false,
                    },
                );
            }
            ProjectDef::DirTable {
                dir: dir_str,
                autostart,
            } => {
                let dir = expand_tilde(&dir_str);
                if !dir.exists() {
                    eprintln!(
                        "warning: directory does not exist for {}: {}",
                        name,
                        dir.display()
                    );
                    continue;
                }
                services.insert(
                    name.clone(),
                    ServiceEntry {
                        name,
                        dir,
                        inline_command: None,
                        autostart,
                    },
                );
            }
            ProjectDef::Command {
                run,
                service_type,
                restart,
                max_retries,
                restart_delay,
                env,
                autostart,
            } => {
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
            restart: cmd
                .restart
                .unwrap_or(if is_task { false } else { defaults.restart }),
            max_retries: cmd.max_retries.unwrap_or(defaults.max_retries),
            restart_delay_secs: cmd.restart_delay.unwrap_or(defaults.restart_delay),
            env,
            autostart: !is_task,
            pre_start: None,
            ports: Vec::new(),
        };
        return Service {
            name: entry.name.clone(),
            dir: entry.dir.clone(),
            processes: vec![proc],
        };
    }

    // Project with services.toml
    let services_path = entry.dir.join("services.toml");
    let content = match std::fs::read_to_string(&services_path) {
        Ok(c) => c,
        Err(_) => {
            return Service {
                name: entry.name.clone(),
                dir: entry.dir.clone(),
                processes: vec![],
            };
        }
    };

    let raw: BTreeMap<String, toml::Value> = match toml::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "warning: failed to parse {}: {}",
                services_path.display(),
                e
            );
            return Service {
                name: entry.name.clone(),
                dir: entry.dir.clone(),
                processes: vec![],
            };
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

    Service {
        name: entry.name.clone(),
        dir: entry.dir.clone(),
        processes,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_defaults() -> DefaultsConfig {
        DefaultsConfig {
            restart: true,
            max_retries: 3,
            restart_delay: 1,
            env: HashMap::new(),
        }
    }

    // ── ProjectDef deserialization ────────────────────────────────────────

    #[test]
    fn parse_project_simple_dir() {
        let val: toml::Value = toml::Value::String("/dev/myapp".into());
        let def: ProjectDef = val.try_into().unwrap();
        match def {
            ProjectDef::Dir(d) => assert_eq!(d, "/dev/myapp"),
            _ => panic!("expected Dir variant"),
        }
    }

    #[test]
    fn parse_project_dir_table() {
        let toml_str = r#"dir = "/dev/myapp"
autostart = true"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ProjectDef = val.try_into().unwrap();
        match def {
            ProjectDef::DirTable { dir, autostart } => {
                assert_eq!(dir, "/dev/myapp");
                assert!(autostart);
            }
            _ => panic!("expected DirTable variant"),
        }
    }

    #[test]
    fn parse_project_command() {
        let toml_str = r#"run = "ssh -N server""#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ProjectDef = val.try_into().unwrap();
        match def {
            ProjectDef::Command { run, .. } => assert_eq!(run, "ssh -N server"),
            _ => panic!("expected Command variant"),
        }
    }

    #[test]
    fn parse_project_command_with_extras() {
        let toml_str = r#"
run = "my-daemon"
type = "task"
restart = false
max_retries = 5
restart_delay = 10
autostart = true
env = { FOO = "bar" }
"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ProjectDef = val.try_into().unwrap();
        match def {
            ProjectDef::Command {
                run,
                service_type,
                restart,
                max_retries,
                restart_delay,
                env,
                autostart,
            } => {
                assert_eq!(run, "my-daemon");
                assert_eq!(service_type, ServiceType::Task);
                assert_eq!(restart, Some(false));
                assert_eq!(max_retries, Some(5));
                assert_eq!(restart_delay, Some(10));
                assert!(autostart);
                assert_eq!(env.get("FOO").unwrap(), "bar");
            }
            _ => panic!("expected Command variant"),
        }
    }

    // ── ServiceDef deserialization ────────────────────────────────────────

    #[test]
    fn parse_service_simple() {
        let val: toml::Value = toml::Value::String("npm run dev".into());
        let def: ServiceDef = val.try_into().unwrap();
        match def {
            ServiceDef::Simple(cmd) => assert_eq!(cmd, "npm run dev"),
            _ => panic!("expected Simple variant"),
        }
    }

    #[test]
    fn parse_service_full() {
        let toml_str = r#"
run = "python worker.py"
type = "task"
restart = false
ports = [8080, 9090]
pre_start = "python migrate.py"
env = { PYTHONPATH = "/app" }
"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        match def {
            ServiceDef::Full {
                run,
                service_type,
                restart,
                ports,
                pre_start,
                env,
                ..
            } => {
                assert_eq!(run, "python worker.py");
                assert_eq!(service_type, ServiceType::Task);
                assert_eq!(restart, Some(false));
                assert_eq!(ports, vec![8080, 9090]);
                assert_eq!(pre_start.unwrap(), "python migrate.py");
                assert_eq!(env.get("PYTHONPATH").unwrap(), "/app");
            }
            _ => panic!("expected Full variant"),
        }
    }

    // ── ServiceDef::into_process_def ─────────────────────────────────────

    #[test]
    fn simple_service_uses_defaults() {
        let def = ServiceDef::Simple("echo hi".into());
        let defaults = test_defaults();
        let proc = def.into_process_def("web".into(), &defaults);
        assert_eq!(proc.name, "web");
        assert_eq!(proc.command, "echo hi");
        assert_eq!(proc.service_type, ServiceType::Service);
        assert!(proc.restart);
        assert_eq!(proc.max_retries, 3);
        assert!(proc.autostart);
    }

    #[test]
    fn task_defaults_no_restart() {
        let toml_str = r#"run = "migrate"
type = "task""#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        let proc = def.into_process_def("migrate".into(), &test_defaults());
        assert_eq!(proc.service_type, ServiceType::Task);
        assert!(!proc.restart);
        assert!(!proc.autostart);
    }

    #[test]
    fn full_service_overrides_defaults() {
        let toml_str = r#"
run = "worker"
restart = false
max_retries = 10
restart_delay = 5
"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        let proc = def.into_process_def("worker".into(), &test_defaults());
        assert!(!proc.restart);
        assert_eq!(proc.max_retries, 10);
        assert_eq!(proc.restart_delay_secs, 5);
    }

    #[test]
    fn full_service_merges_env() {
        let mut defaults = test_defaults();
        defaults.env.insert("GLOBAL".into(), "1".into());

        let toml_str = r#"
run = "worker"
env = { LOCAL = "2" }
"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        let proc = def.into_process_def("worker".into(), &defaults);
        assert_eq!(proc.env.get("GLOBAL").unwrap(), "1");
        assert_eq!(proc.env.get("LOCAL").unwrap(), "2");
    }

    // ── load_service with inline command ─────────────────────────────────

    #[test]
    fn load_service_inline_command() {
        let entry = ServiceEntry {
            name: "tunnel".into(),
            dir: PathBuf::from("/tmp/kagaya-test-inline"),
            inline_command: Some(InlineCommand {
                run: "ssh -N server".into(),
                service_type: ServiceType::Service,
                restart: None,
                max_retries: None,
                restart_delay: None,
                env: HashMap::new(),
            }),
            autostart: false,
        };
        let svc = load_service(&entry, &test_defaults());
        assert_eq!(svc.processes.len(), 1);
        assert_eq!(svc.processes[0].name, "tunnel");
        assert_eq!(svc.processes[0].command, "ssh -N server");
        assert!(svc.processes[0].restart);
        assert!(svc.processes[0].autostart);
    }

    #[test]
    fn load_service_inline_task() {
        let entry = ServiceEntry {
            name: "migrate".into(),
            dir: PathBuf::from("/tmp/kagaya-test-task"),
            inline_command: Some(InlineCommand {
                run: "python migrate.py".into(),
                service_type: ServiceType::Task,
                restart: None,
                max_retries: None,
                restart_delay: None,
                env: HashMap::new(),
            }),
            autostart: false,
        };
        let svc = load_service(&entry, &test_defaults());
        assert_eq!(svc.processes.len(), 1);
        assert_eq!(svc.processes[0].service_type, ServiceType::Task);
        assert!(!svc.processes[0].restart);
        assert!(!svc.processes[0].autostart);
    }

    #[test]
    fn load_service_inline_with_overrides() {
        let entry = ServiceEntry {
            name: "daemon".into(),
            dir: PathBuf::from("/tmp/kagaya-test-overrides"),
            inline_command: Some(InlineCommand {
                run: "my-daemon".into(),
                service_type: ServiceType::Service,
                restart: Some(false),
                max_retries: Some(10),
                restart_delay: Some(5),
                env: [("MY_VAR".into(), "hello".into())].into(),
            }),
            autostart: false,
        };
        let svc = load_service(&entry, &test_defaults());
        let proc = &svc.processes[0];
        assert!(!proc.restart);
        assert_eq!(proc.max_retries, 10);
        assert_eq!(proc.restart_delay_secs, 5);
        assert_eq!(proc.env.get("MY_VAR").unwrap(), "hello");
    }

    // ── load_service from services.toml ──────────────────────────────────

    #[test]
    fn load_service_from_services_toml() {
        let dir = std::env::temp_dir().join("kagaya-test-svctoml");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("services.toml"),
            r#"
web = "npm run dev"
[worker]
run = "python worker.py"
type = "task"
"#,
        )
        .unwrap();

        let entry = ServiceEntry {
            name: "myapp".into(),
            dir: dir.clone(),
            inline_command: None,
            autostart: false,
        };
        let svc = load_service(&entry, &test_defaults());
        assert_eq!(svc.processes.len(), 2);
        let web = svc.processes.iter().find(|p| p.name == "web").unwrap();
        assert_eq!(web.command, "npm run dev");
        assert_eq!(web.service_type, ServiceType::Service);
        let worker = svc.processes.iter().find(|p| p.name == "worker").unwrap();
        assert_eq!(worker.command, "python worker.py");
        assert_eq!(worker.service_type, ServiceType::Task);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_service_missing_services_toml() {
        let entry = ServiceEntry {
            name: "nofile".into(),
            dir: PathBuf::from("/tmp/kagaya-test-nonexistent-dir"),
            inline_command: None,
            autostart: false,
        };
        let svc = load_service(&entry, &test_defaults());
        assert!(svc.processes.is_empty());
    }

    // ── expand_tilde ─────────────────────────────────────────────────────

    #[test]
    fn expand_tilde_home() {
        let result = expand_tilde("~/dev/myapp");
        let home = std::env::var("HOME").unwrap();
        assert_eq!(result, PathBuf::from(home).join("dev/myapp"));
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let result = expand_tilde("/dev/myapp");
        assert_eq!(result, PathBuf::from("/dev/myapp"));
    }

    // ── Full projects.toml parsing ───────────────────────────────────────

    #[test]
    fn parse_projects_toml_mixed() {
        let content = r#"
myapp = "/dev/myapp"

[frontend]
dir = "/dev/frontend"
autostart = true

[tunnel]
run = "ssh -N server"
"#;
        // Parse the same way load_projects does
        let raw: BTreeMap<String, toml::Value> = toml::from_str(content).unwrap();
        assert_eq!(raw.len(), 3);

        let myapp: ProjectDef = raw["myapp"].clone().try_into().unwrap();
        assert!(matches!(myapp, ProjectDef::Dir(ref d) if d == "/dev/myapp"));

        let frontend: ProjectDef = raw["frontend"].clone().try_into().unwrap();
        assert!(
            matches!(frontend, ProjectDef::DirTable { ref dir, autostart: true } if dir == "/dev/frontend")
        );

        let tunnel: ProjectDef = raw["tunnel"].clone().try_into().unwrap();
        assert!(matches!(tunnel, ProjectDef::Command { ref run, .. } if run == "ssh -N server"));
    }
}
