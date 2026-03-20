use crate::autostart;
use crate::config;
use crate::daemon::supervisor::Supervisor;
use kagaya::{ProcessState, ServiceType};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower_http::cors::CorsLayer;

#[derive(RustEmbed)]
#[folder = "../../ui/build/"]
struct UiAssets;

#[derive(Clone)]
pub struct AppState {
	pub supervisor: Arc<Supervisor>,
}

pub fn router(supervisor: Arc<Supervisor>) -> Router {
	let state = AppState { supervisor };

	Router::new()
		.route("/api/services", get(list_services))
		.route("/api/services/{name}", get(service_detail))
		.route("/api/services/{name}/start", post(start_service))
		.route("/api/services/{name}/stop", post(stop_service))
		.route("/api/services/{name}/reload", post(reload_service))
		.route(
			"/api/services/{name}/processes/{process}/restart",
			post(restart_process),
		)
		.route(
			"/api/services/{name}/processes/{process}/kill",
			post(kill_process),
		)
		.route("/api/services/{name}/echo", get(echo_service))
		.route("/ws/echo/{name}", get(ws_echo))
		.route("/api/cron", get(cron_status))
		.route("/api/cron/{name}/run", post(cron_run))
		.route("/api/cron/{name}/pause", post(cron_pause))
		.route("/api/cron/{name}/resume", post(cron_resume))
		.route("/api/autostart", get(autostart_status))
		.route("/api/autostart/on", post(autostart_on))
		.route("/api/autostart/off", post(autostart_off))
		.route("/api/remote-control", get(rc_list))
		.route(
			"/api/remote-control/{name}",
			post(rc_enable).delete(rc_disable).patch(rc_update_mode),
		)
		.fallback(static_handler)
		.layer(CorsLayer::permissive())
		.with_state(state)
}

#[derive(Serialize)]
struct ServiceInfo {
	name: String,
	dir: String,
	running: bool,
	autostart: bool,
}

#[derive(Serialize)]
struct ServiceDetail {
	name: String,
	dir: String,
	running: bool,
	processes: Vec<ProcessInfo>,
}

#[derive(Serialize)]
struct ProcessInfo {
	name: String,
	pid: Option<u32>,
	status: String,
	autostart: bool,
	#[serde(rename = "type")]
	service_type: String,
	ports: Vec<u16>,
}

#[derive(Serialize)]
struct ActionResponse {
	message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
	error: String,
}

async fn list_services(State(state): State<AppState>) -> Json<Vec<ServiceInfo>> {
	let statuses = state.supervisor.status().await;
	let entries = config::load_service_entries();
	let services = statuses
		.iter()
		.map(|s| {
			let autostart = entries.get(&s.name).map(|e| e.autostart).unwrap_or(false);
			ServiceInfo {
				name: s.name.clone(),
				dir: s.dir.to_string_lossy().to_string(),
				running: s.is_running(),
				autostart,
			}
		})
		.collect();
	Json(services)
}

async fn service_detail(
	State(state): State<AppState>,
	Path(name): Path<String>,
) -> Result<Json<ServiceDetail>, (StatusCode, Json<ErrorResponse>)> {
	let statuses = state.supervisor.status().await;
	let status = statuses
		.into_iter()
		.find(|s| s.name == name)
		.ok_or_else(|| {
			(
				StatusCode::NOT_FOUND,
				Json(ErrorResponse {
					error: format!("service not found: {}", name),
				}),
			)
		})?;

	let running = status.is_running();
	let processes = status
		.processes
		.into_iter()
		.map(|p| {
			let status_str = match &p.state {
				ProcessState::Running { pid, uptime_secs } => {
					format!("running (pid {}, {}s)", pid, uptime_secs)
				}
				ProcessState::Stopped => "stopped".to_string(),
				ProcessState::Crashed { exit_code, retries } => {
					format!("crashed (exit {}, retry {})", exit_code, retries)
				}
				ProcessState::Failed { exit_code } => {
					format!("failed (exit {})", exit_code)
				}
			};
			ProcessInfo {
				name: p.name,
				pid: p.pid,
				status: status_str,
				autostart: p.autostart,
				service_type: match p.service_type {
					ServiceType::Task => "task".to_string(),
					ServiceType::Service => "service".to_string(),
				},
				ports: p.ports,
			}
		})
		.collect();

	Ok(Json(ServiceDetail {
		name: status.name,
		dir: status.dir.to_string_lossy().to_string(),
		running,
		processes,
	}))
}

async fn start_service(
	State(state): State<AppState>,
	Path(name): Path<String>,
	axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	let all = params.get("all").map(|v| v == "true" || v == "1").unwrap_or(false);
	state
		.supervisor
		.start_service_filtered(&name, all, &[], &[])
		.await
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| {
			(
				StatusCode::BAD_REQUEST,
				Json(ErrorResponse { error: e }),
			)
		})
}

async fn stop_service(
	State(state): State<AppState>,
	Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	state
		.supervisor
		.stop_service(&name)
		.await
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| {
			(
				StatusCode::BAD_REQUEST,
				Json(ErrorResponse { error: e }),
			)
		})
}

async fn reload_service(
	State(state): State<AppState>,
	Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	state
		.supervisor
		.reload_service_filtered(&name, false, &[])
		.await
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| {
			(
				StatusCode::BAD_REQUEST,
				Json(ErrorResponse { error: e }),
			)
		})
}

async fn restart_process(
	State(state): State<AppState>,
	Path((name, process)): Path<(String, String)>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	state
		.supervisor
		.restart_process(&name, &process)
		.await
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| {
			(
				StatusCode::BAD_REQUEST,
				Json(ErrorResponse { error: e }),
			)
		})
}

async fn kill_process(
	State(state): State<AppState>,
	Path((name, process)): Path<(String, String)>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	state
		.supervisor
		.kill_process(&name, &process)
		.await
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| {
			(
				StatusCode::BAD_REQUEST,
				Json(ErrorResponse { error: e }),
			)
		})
}

async fn cron_status() -> Result<Json<Vec<koku::JobStatus>>, (StatusCode, Json<ErrorResponse>)> {
	crate::koku_client::fetch_status()
		.map(Json)
		.ok_or_else(|| {
			(
				StatusCode::SERVICE_UNAVAILABLE,
				Json(ErrorResponse { error: "koku daemon not running".to_string() }),
			)
		})
}

async fn cron_run(
	Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	crate::koku_client::run_job(&name)
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}

async fn cron_pause(
	Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	crate::koku_client::pause_job(&name)
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}

async fn cron_resume(
	Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	crate::koku_client::resume_job(&name)
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}

async fn echo_service(
	State(state): State<AppState>,
	Path(name): Path<String>,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
	let outputs = state.supervisor.get_all_outputs(&name).await.map_err(|e| {
		(
			StatusCode::NOT_FOUND,
			Json(ErrorResponse { error: e }),
		)
	})?;

	let mut result = String::new();
	for (proc_name, capture) in outputs {
		if !result.is_empty() {
			result.push_str(&format!("\n--- {} ---\n", proc_name));
		}
		let snapshot = capture.snapshot().await;
		result.push_str(&String::from_utf8_lossy(&snapshot));
	}
	Ok(result)
}

async fn ws_echo(
	State(state): State<AppState>,
	Path(name): Path<String>,
	ws: WebSocketUpgrade,
) -> impl IntoResponse {
	ws.on_upgrade(move |socket| handle_ws_echo(socket, state, name))
}

async fn handle_ws_echo(mut socket: WebSocket, state: AppState, name: String) {
	let outputs = match state.supervisor.get_all_outputs(&name).await {
		Ok(o) => o,
		Err(_) => {
			return;
		}
	};

	for (proc_name, capture) in &outputs {
		let snapshot = capture.snapshot().await;
		if !snapshot.is_empty() {
			let header = format!("\x1b[1m--- {} ---\x1b[0m\r\n", proc_name);
			let mut data = header.into_bytes();
			data.extend_from_slice(&snapshot);
			let _ = socket.send(Message::Binary(data.into())).await;
		}
	}

	let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(256);

	for (_, capture) in &outputs {
		let mut broadcast_rx = capture.subscribe();
		let tx = tx.clone();
		tokio::spawn(async move {
			loop {
				match broadcast_rx.recv().await {
					Ok(data) => {
						if tx.send(data).await.is_err() {
							break;
						}
					}
					Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
					Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
				}
			}
		});
	}
	drop(tx);

	loop {
		tokio::select! {
			biased;
			msg = socket.recv() => {
				match msg {
					Some(Ok(Message::Close(_))) | None => break,
					_ => {}
				}
			}
			data = rx.recv() => {
				match data {
					Some(bytes) => {
						if socket.send(Message::Binary(bytes.into())).await.is_err() {
							break;
						}
					}
					None => break,
				}
			}
		}
	}
}

// ── Autostart ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct AutostartStatusResponse {
	installed: bool,
	active: bool,
	agent_path: Option<String>,
	projects: Vec<String>,
}

async fn autostart_status() -> Json<AutostartStatusResponse> {
	let info = autostart::status_info();
	Json(AutostartStatusResponse {
		installed: info.installed,
		active: info.active,
		agent_path: info.agent_path,
		projects: info.projects,
	})
}

async fn autostart_on() -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	autostart::enable()
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}

async fn autostart_off() -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	autostart::disable()
		.map(|msg| Json(ActionResponse { message: msg }))
		.map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}

// ── Remote Control (claude-rc proxy) ─────────────────────────────────────────

mod rc {
	use super::*;
	use std::path::PathBuf;

	#[derive(Serialize)]
	pub enum Request {
		List,
		Enable { name: String, dir: String, mode: String },
		Disable { name: String },
		UpdateMode { name: String, mode: String },
	}

	#[derive(Deserialize)]
	pub enum Response {
		Ok { message: String },
		ProjectList(Vec<ProjectStatus>),
		Error { message: String },
	}

	#[derive(Deserialize, Serialize, Clone)]
	pub struct ProjectStatus {
		pub name: String,
		pub dir: String,
		pub mode: String,
		pub running: bool,
		pub pid: Option<u32>,
	}

	fn socket_path() -> PathBuf {
		let home = std::env::var("HOME").expect("HOME not set");
		PathBuf::from(home).join(".local/state/claude-rc/daemon.sock")
	}

	pub async fn send(req: &Request) -> Result<Response, String> {
		let stream = tokio::net::UnixStream::connect(socket_path())
			.await
			.map_err(|e| format!("claude-rc daemon not reachable: {e}"))?;
		let (reader, mut writer) = stream.into_split();
		let mut json = serde_json::to_string(req).map_err(|e| format!("serialize: {e}"))?;
		json.push('\n');
		writer.write_all(json.as_bytes()).await.map_err(|e| format!("write: {e}"))?;
		let mut buf = BufReader::new(reader);
		let mut line = String::new();
		buf.read_line(&mut line).await.map_err(|e| format!("read: {e}"))?;
		if line.is_empty() {
			return Err("daemon closed connection".to_string());
		}
		serde_json::from_str(&line).map_err(|e| format!("parse: {e}"))
	}
}

async fn rc_list() -> Result<Json<Vec<rc::ProjectStatus>>, (StatusCode, Json<ErrorResponse>)> {
	match rc::send(&rc::Request::List).await {
		Ok(rc::Response::ProjectList(projects)) => Ok(Json(projects)),
		Ok(rc::Response::Error { message }) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: message }))),
		Ok(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "unexpected response".into() }))),
		Err(e) => Err((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse { error: e }))),
	}
}

#[derive(Deserialize)]
struct RcEnableBody {
	dir: String,
	mode: String,
}

async fn rc_enable(
	Path(name): Path<String>,
	Json(body): Json<RcEnableBody>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	match rc::send(&rc::Request::Enable { name, dir: body.dir, mode: body.mode }).await {
		Ok(rc::Response::Ok { message }) => Ok(Json(ActionResponse { message })),
		Ok(rc::Response::Error { message }) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: message }))),
		Ok(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "unexpected response".into() }))),
		Err(e) => Err((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse { error: e }))),
	}
}

async fn rc_disable(
	Path(name): Path<String>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	match rc::send(&rc::Request::Disable { name }).await {
		Ok(rc::Response::Ok { message }) => Ok(Json(ActionResponse { message })),
		Ok(rc::Response::Error { message }) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: message }))),
		Ok(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "unexpected response".into() }))),
		Err(e) => Err((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse { error: e }))),
	}
}

#[derive(Deserialize)]
struct RcUpdateModeBody {
	mode: String,
}

async fn rc_update_mode(
	Path(name): Path<String>,
	Json(body): Json<RcUpdateModeBody>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorResponse>)> {
	match rc::send(&rc::Request::UpdateMode { name, mode: body.mode }).await {
		Ok(rc::Response::Ok { message }) => Ok(Json(ActionResponse { message })),
		Ok(rc::Response::Error { message }) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: message }))),
		Ok(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "unexpected response".into() }))),
		Err(e) => Err((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse { error: e }))),
	}
}

// ── Static files ─────────────────────────────────────────────────────────────

async fn static_handler(uri: Uri) -> impl IntoResponse {
	let path = uri.path().trim_start_matches('/');

	if let Some(content) = UiAssets::get(path) {
		return serve_asset(path, content);
	}

	if !path.starts_with("_app/") && !path.contains('.') {
		if let Some(content) = UiAssets::get("index.html") {
			return serve_asset("index.html", content);
		}
	}

	Response::builder()
		.status(StatusCode::NOT_FOUND)
		.body("Not Found".into())
		.unwrap()
}

fn serve_asset(path: &str, content: rust_embed::EmbeddedFile) -> Response {
	let mime = mime_guess::from_path(path).first_or_octet_stream();

	Response::builder()
		.status(StatusCode::OK)
		.header(header::CONTENT_TYPE, mime.as_ref())
		.body(content.data.into())
		.unwrap()
}
