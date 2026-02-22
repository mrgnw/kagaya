use kagaya::{ProcessState, ServiceStatus};
use serde::Serialize;

#[derive(Serialize)]
pub struct StatusOutput {
    pub services: Vec<ServiceStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_port: Option<u16>,
}

#[derive(Serialize)]
pub struct ActionOutput {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct LogLine {
    pub line: String,
    #[serde(default)]
    pub offset: u64,
}

pub fn json_status(services: &[ServiceStatus], http_port: Option<u16>) {
    let out = StatusOutput {
        services: services.to_vec(),
        http_port,
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

pub fn tsv_status(services: &[ServiceStatus]) {
    println!("service\tprocess\tstate\tpid\tuptime\tports\texit_code\tretries\tautostart\ttype\tstate_since");
    for svc in services {
        for proc in &svc.processes {
            let (state, pid, uptime, exit_code, retries) = match &proc.state {
                ProcessState::Running { pid, uptime_secs } => (
                    "running".to_string(),
                    pid.to_string(),
                    uptime_secs.to_string(),
                    String::new(),
                    String::new(),
                ),
                ProcessState::Stopped => (
                    "stopped".to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ),
                ProcessState::Crashed { exit_code, retries } => (
                    "crashed".to_string(),
                    String::new(),
                    String::new(),
                    exit_code.to_string(),
                    retries.to_string(),
                ),
                ProcessState::Failed { exit_code } => (
                    "failed".to_string(),
                    String::new(),
                    String::new(),
                    exit_code.to_string(),
                    String::new(),
                ),
            };
            let ports = proc
                .ports
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let stype = match proc.service_type {
                kagaya::ServiceType::Service => "service",
                kagaya::ServiceType::Task => "task",
            };
            let since = proc.state_since.map(|t| t.to_string()).unwrap_or_default();
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                svc.name,
                proc.name,
                state,
                pid,
                uptime,
                ports,
                exit_code,
                retries,
                proc.autostart,
                stype,
                since,
            );
        }
    }
}

pub fn json_ok(message: Option<String>) {
    let out = ActionOutput {
        ok: true,
        message,
        error: None,
    };
    println!("{}", serde_json::to_string(&out).unwrap());
}

pub fn json_error(message: &str) {
    let out = ActionOutput {
        ok: false,
        message: None,
        error: Some(message.to_string()),
    };
    println!("{}", serde_json::to_string(&out).unwrap());
}

pub fn json_log_line(line: &str, offset: u64) {
    let out = LogLine {
        line: line.to_string(),
        offset,
    };
    println!("{}", serde_json::to_string(&out).unwrap());
}

pub fn json_value(val: &impl Serialize) {
    println!("{}", serde_json::to_string_pretty(val).unwrap());
}
