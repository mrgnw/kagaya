use crate::autostart;
use crate::plist_sync::{OpResult, ProcessFilters};
use axum::Json;
use serde::Serialize;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response as AxumResponse;
use std::io::SeekFrom;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub async fn ws_echo(ws: WebSocketUpgrade, Path(name): Path<String>) -> AxumResponse {
    ws.on_upgrade(move |socket| stream_log(socket, name))
}

async fn stream_log(mut socket: WebSocket, name: String) {
    let Some((stdout_path, _stderr)) = plist_sync::log_paths(&name) else {
        let _ = socket
            .send(Message::Text(format!("no log for '{}'", name).into()))
            .await;
        return;
    };
    let mut file = match tokio::fs::File::open(&stdout_path).await {
        Ok(f) => f,
        Err(_) => {
            let _ = socket
                .send(Message::Text(
                    format!("log not found: {}", stdout_path.display()).into(),
                ))
                .await;
            return;
        }
    };
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(64 * 1024);
    let mut pos = start;
    let _ = file.seek(SeekFrom::Start(start)).await;
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match file.read(&mut buf).await {
            Ok(0) => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let _ = file.seek(SeekFrom::Start(pos)).await;
                continue;
            }
            Ok(n) => n,
            Err(_) => return,
        };
        pos += n as u64;
        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
        if socket.send(Message::Text(chunk.into())).await.is_err() {
            return;
        }
    }
}

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
    use std::io::{Read, Seek, SeekFrom};
    let (_stdout, stderr) = plist_sync::log_paths(name)?;
    let mut file = std::fs::File::open(&stderr).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let read_size = len.min(8 * 1024);
    file.seek(SeekFrom::End(-(read_size as i64))).ok()?;
    let mut buf = vec![0u8; read_size as usize];
    file.read_exact(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<String> = text
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
        error_tail: (!st.is_running())
            .then(|| error_tail_for(&st.name))
            .flatten(),
    }
}

#[derive(Serialize)]
pub struct ActionResponse {
    pub message: String,
}

fn op_response(
    results: Vec<OpResult>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let any_err = results.iter().any(|r| !r.ok);
    let message = results
        .iter()
        .map(|r| r.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    if any_err {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: message }),
        ))
    } else {
        Ok(Json(ActionResponse { message }))
    }
}

fn require_service(name: &str) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if config::load_service_entries().contains_key(name) {
        Ok(())
    } else {
        Err(not_found(name))
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

type ActionResult = Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)>;

pub async fn start(Path(name): Path<String>) -> ActionResult {
    require_service(&name)?;
    op_response(plist_sync::start_services(&[name], &ProcessFilters::new()))
}

pub async fn stop(Path(name): Path<String>) -> ActionResult {
    require_service(&name)?;
    op_response(plist_sync::stop_services(&[name], &ProcessFilters::new()))
}

pub async fn reload(Path(name): Path<String>) -> ActionResult {
    let entries = config::load_service_entries();
    let entry = entries.get(&name).ok_or_else(|| not_found(&name))?;
    match plist_sync::sync_service(entry) {
        Ok(n) => Ok(Json(ActionResponse {
            message: format!("{}: synced {} process(es)", name, n),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
    }
}

fn one_filter(name: &str, process: &str) -> ProcessFilters {
    let mut f = ProcessFilters::new();
    f.insert(name.to_string(), vec![process.to_string()]);
    f
}

pub async fn restart_process(Path((name, process)): Path<(String, String)>) -> ActionResult {
    require_service(&name)?;
    op_response(plist_sync::restart_services(
        &[name.clone()],
        &one_filter(&name, &process),
    ))
}

pub async fn kill_process(Path((name, process)): Path<(String, String)>) -> ActionResult {
    require_service(&name)?;
    op_response(plist_sync::stop_services(
        &[name.clone()],
        &one_filter(&name, &process),
    ))
}

#[derive(Serialize)]
pub struct HostInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tailscale_hostname: Option<String>,
}

fn detect_tailscale_hostname() -> Option<String> {
    let output = std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let dns_name = json.get("Self")?.get("DNSName")?.as_str()?;
    Some(dns_name.trim_end_matches('.').to_string())
}

pub async fn host_info() -> Json<HostInfo> {
    Json(HostInfo {
        tailscale_hostname: detect_tailscale_hostname(),
    })
}

#[derive(Serialize)]
pub struct AutostartStatus {
    pub installed: bool,
    pub active: bool,
    pub agent_path: Option<String>,
    pub projects: Vec<String>,
}

pub async fn autostart_status() -> Json<AutostartStatus> {
    let info = autostart::status_info();
    Json(AutostartStatus {
        installed: info.installed,
        active: info.active,
        agent_path: info.agent_path,
        projects: info.projects,
    })
}

pub async fn autostart_on() -> ActionResult {
    autostart::enable()
        .map(|message| Json(ActionResponse { message }))
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}

pub async fn autostart_off() -> ActionResult {
    autostart::disable()
        .map(|message| Json(ActionResponse { message }))
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}
