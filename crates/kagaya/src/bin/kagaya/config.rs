use crate::utils::config_dir;
use kagaya::{ProcessDef, Service, ServiceType};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

// ── Global config (~/.config/kagaya/config.toml) ────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GlobalConfig {
    #[serde(default)]
    #[cfg_attr(feature = "dev", allow(dead_code))]
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
    #[allow(dead_code)]
    pub public_base_url: Option<String>,
    #[allow(dead_code)]
    pub release_dir: Option<String>,
    #[serde(default = "default_port")]
    #[cfg_attr(feature = "dev", allow(dead_code))]
    pub port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_timeout: default_idle_timeout(),
            log_dir: None,
            public_base_url: None,
            release_dir: None,
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

/// Accepts either a single string or a list of strings in TOML.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StringOrVec {
    One(String),
    Many(Vec<String>),
}

impl StringOrVec {
    fn into_vec(self) -> Vec<String> {
        match self {
            StringOrVec::One(s) => vec![s],
            StringOrVec::Many(v) => v,
        }
    }
}

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
        depends_on: Option<StringOrVec>,
        ready: Option<String>,
        ready_timeout: Option<u64>,
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
                depends_on: Vec::new(),
                ready: None,
                ready_timeout: 10,
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
                depends_on,
                ready,
                ready_timeout,
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
                    depends_on: depends_on.map(|d| d.into_vec()).unwrap_or_default(),
                    ready,
                    ready_timeout: ready_timeout.unwrap_or(10),
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
        depends_on: Option<StringOrVec>,
        urls: Option<StringOrVec>,
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
        depends_on: Option<StringOrVec>,
        urls: Option<StringOrVec>,
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
    /// Other projects this project depends on (for autostart ordering)
    pub depends_on: Vec<String>,
    /// Manually configured URLs for this service (e.g. tunnel subdomains)
    pub urls: Vec<String>,
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
                        depends_on: vec![],
                        urls: vec![],
                    },
                );
            }
            ProjectDef::DirTable {
                dir: dir_str,
                autostart,
                depends_on,
                urls,
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
                        depends_on: depends_on.map(|d| d.into_vec()).unwrap_or_default(),
                        urls: urls.map(|u| u.into_vec()).unwrap_or_default(),
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
                depends_on,
                urls,
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
                        depends_on: depends_on.map(|d| d.into_vec()).unwrap_or_default(),
                        urls: urls.map(|u| u.into_vec()).unwrap_or_default(),
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

/// Returns autostart project names topologically sorted by depends_on,
/// and the dependency chains to pass to the start request.
pub fn autostart_sorted() -> (Vec<String>, Vec<Vec<String>>) {
    let projects = load_projects();
    let autostart: Vec<&ServiceEntry> = projects.values().filter(|e| e.autostart).collect();

    if autostart.is_empty() {
        return (vec![], vec![]);
    }

    let autostart_names: std::collections::HashSet<&str> =
        autostart.iter().map(|e| e.name.as_str()).collect();

    // Build chains from depends_on relationships
    let mut chains: Vec<Vec<String>> = Vec::new();
    for entry in &autostart {
        for dep in &entry.depends_on {
            if autostart_names.contains(dep.as_str()) {
                // Find or create a chain that ends with dep, extend it
                let mut found = false;
                for chain in &mut chains {
                    if chain.last().map(|s| s.as_str()) == Some(dep.as_str()) {
                        chain.push(entry.name.clone());
                        found = true;
                        break;
                    }
                }
                if !found {
                    chains.push(vec![dep.clone(), entry.name.clone()]);
                }
            }
        }
    }

    let names: Vec<String> = autostart.iter().map(|e| e.name.clone()).collect();
    (names, chains)
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
            depends_on: Vec::new(),
            ready: None,
            ready_timeout: 10,
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
            ProjectDef::DirTable { dir, autostart, .. } => {
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
                ..
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
            depends_on: vec![],
            urls: vec![],
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
            depends_on: vec![],
            urls: vec![],
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
            depends_on: vec![],
            urls: vec![],
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
            depends_on: vec![],
            urls: vec![],
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
            depends_on: vec![],
            urls: vec![],
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
            matches!(frontend, ProjectDef::DirTable { ref dir, autostart: true, .. } if dir == "/dev/frontend")
        );

        let tunnel: ProjectDef = raw["tunnel"].clone().try_into().unwrap();
        assert!(matches!(tunnel, ProjectDef::Command { ref run, .. } if run == "ssh -N server"));
    }

    #[test]
    fn parse_project_dir_table_with_depends_on() {
        let toml_str = r#"dir = "/dev/openchamber"
autostart = true
depends_on = "opencode""#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ProjectDef = val.try_into().unwrap();
        match def {
            ProjectDef::DirTable {
                dir,
                autostart,
                depends_on,
                ..
            } => {
                assert_eq!(dir, "/dev/openchamber");
                assert!(autostart);
                assert_eq!(depends_on.unwrap().into_vec(), vec!["opencode".to_string()]);
            }
            _ => panic!("expected DirTable variant"),
        }
    }

    #[test]
    fn parse_project_command_with_depends_on() {
        let toml_str = r#"run = "my-tool"
depends_on = ["svc-a", "svc-b"]"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ProjectDef = val.try_into().unwrap();
        match def {
            ProjectDef::Command { depends_on, .. } => {
                assert_eq!(
                    depends_on.unwrap().into_vec(),
                    vec!["svc-a".to_string(), "svc-b".to_string()]
                );
            }
            _ => panic!("expected Command variant"),
        }
    }

    // ── depends_on / ready / ready_timeout parsing ──────────────────────

    #[test]
    fn parse_depends_on_string() {
        let toml_str = r#"
run = "python api.py"
depends_on = "db"
"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        let proc = def.into_process_def("api".into(), &test_defaults());
        assert_eq!(proc.depends_on, vec!["db".to_string()]);
    }

    #[test]
    fn parse_depends_on_list() {
        let toml_str = r#"
run = "python worker.py"
depends_on = ["db", "cache"]
"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        let proc = def.into_process_def("worker".into(), &test_defaults());
        assert_eq!(proc.depends_on, vec!["db".to_string(), "cache".to_string()]);
    }

    #[test]
    fn parse_ready_command() {
        let toml_str = r#"
run = "docker compose up postgres"
ready = "pg_isready -h localhost"
ready_timeout = 30
"#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        let proc = def.into_process_def("db".into(), &test_defaults());
        assert_eq!(proc.ready.unwrap(), "pg_isready -h localhost");
        assert_eq!(proc.ready_timeout, 30);
    }

    #[test]
    fn parse_no_depends_on_defaults_empty() {
        let toml_str = r#"run = "echo hi""#;
        let val: toml::Value = toml::from_str(toml_str).unwrap();
        let def: ServiceDef = val.try_into().unwrap();
        let proc = def.into_process_def("web".into(), &test_defaults());
        assert!(proc.depends_on.is_empty());
        assert!(proc.ready.is_none());
        assert_eq!(proc.ready_timeout, 10);
    }

    #[test]
    fn simple_service_no_depends_on() {
        let def = ServiceDef::Simple("npm run dev".into());
        let proc = def.into_process_def("web".into(), &test_defaults());
        assert!(proc.depends_on.is_empty());
        assert!(proc.ready.is_none());
        assert_eq!(proc.ready_timeout, 10);
    }
}
