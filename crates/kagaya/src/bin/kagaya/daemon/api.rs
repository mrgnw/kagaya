use crate::daemon::supervisor::Supervisor;
use kagaya::{ProcessState, ServiceType};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
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
		.fallback(static_handler)
		.layer(CorsLayer::permissive())
		.with_state(state)
}

#[derive(Serialize)]
struct ServiceInfo {
	name: String,
	dir: String,
	running: bool,
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
	let services = statuses
		.iter()
		.map(|s| ServiceInfo {
			name: s.name.clone(),
			dir: s.dir.to_string_lossy().to_string(),
			running: s.is_running(),
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
		.start_service_filtered(&name, all, &[])
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
