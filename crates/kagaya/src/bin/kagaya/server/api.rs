use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct VersionResponse {
    pub version: String,
}

pub async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

use crate::config::{self, ServiceEntry};
use crate::plist_sync;
use axum::extract::Path;
use axum::http::StatusCode;
use kagaya::types::{ProcessState, ProcessStatus, ServiceState, ServiceStatus};

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize)]
pub struct ServiceInfo {
    pub name: String,
    pub dir: String,
    pub running: bool,
    pub state: ServiceState,
    pub autostart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_tail: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: Option<u32>,
    pub status: String,
    pub autostart: bool,
    pub ports: Vec<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

#[derive(Serialize)]
pub struct ServiceDetail {
    pub name: String,
    pub dir: String,
    pub running: bool,
    pub state: ServiceState,
    pub processes: Vec<ProcessInfo>,
}

fn process_status_str(state: &ProcessState) -> String {
    match state {
        ProcessState::Running { .. } => "running",
        ProcessState::Stopped => "stopped",
        ProcessState::Crashed { .. } => "crashed",
        ProcessState::Failed { .. } => "failed",
    }
    .to_string()
}

fn to_process_info(p: &ProcessStatus) -> ProcessInfo {
    ProcessInfo {
        name: p.name.clone(),
        pid: p.pid,
        status: process_status_str(&p.state),
        autostart: p.autostart,
        ports: p.ports.clone(),
        cpu_percent: p.cpu_percent,
        memory_bytes: p.memory_bytes,
    }
}

fn error_tail_for(name: &str) -> Option<Vec<String>> {
    let (_stdout, stderr) = plist_sync::log_paths(name)?;
    let content = std::fs::read_to_string(&stderr).ok()?;
    let lines: Vec<String> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(5);
    Some(lines[start..].to_vec())
}

fn to_service_info(st: &ServiceStatus, entry: &ServiceEntry) -> ServiceInfo {
    let ports: Vec<u16> = st.processes.iter().flat_map(|p| p.ports.clone()).collect();
    ServiceInfo {
        name: st.name.clone(),
        dir: st.dir.to_string_lossy().to_string(),
        running: st.is_running(),
        state: st.aggregate_state(),
        autostart: entry.autostart,
        urls: (!entry.urls.is_empty()).then(|| entry.urls.clone()),
        ports: (!ports.is_empty()).then_some(ports),
        error_tail: error_tail_for(&st.name),
    }
}

pub fn not_found(name: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("service '{}' not found", name),
        }),
    )
}

pub async fn list_services() -> Json<Vec<ServiceInfo>> {
    let entries = config::load_service_entries();
    let infos = entries
        .values()
        .map(|e| to_service_info(&plist_sync::status_for(e), e))
        .collect();
    Json(infos)
}

pub async fn service_detail(
    Path(name): Path<String>,
) -> Result<Json<ServiceDetail>, (StatusCode, Json<ErrorResponse>)> {
    let entries = config::load_service_entries();
    let entry = entries.get(&name).ok_or_else(|| not_found(&name))?;
    let st = plist_sync::status_for(entry);
    Ok(Json(ServiceDetail {
        name: st.name.clone(),
        dir: st.dir.to_string_lossy().to_string(),
        running: st.is_running(),
        state: st.aggregate_state(),
        processes: st.processes.iter().map(to_process_info).collect(),
    }))
}
