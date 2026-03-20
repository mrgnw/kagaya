use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::remote_control;
use crate::services;
use crate::tmux;

pub const DEFAULT_PORT: u16 = 13369;

pub async fn start(port: u16, static_dir: Option<PathBuf>) {
    let api = Router::new()
        .route("/api/services", get(api_services))
        .route("/api/services/{name}", get(api_service_detail))
        .route("/api/services/{name}/start", post(api_start))
        .route("/api/services/{name}/stop", post(api_stop))
        .route("/api/services/{name}/reload", post(api_reload))
        .route(
            "/api/services/{name}/processes/{process}/restart",
            post(api_restart_process),
        )
        .route(
            "/api/services/{name}/processes/{process}/kill",
            post(api_kill_process),
        )
        .route("/api/services/{name}/echo", get(api_echo))
        .route("/api/services/{name}/panes", get(api_panes))
        .route(
            "/api/services/{name}/panes/{window}/{pane}",
            get(api_capture_pane),
        )
        .route("/ws/echo/{name}", get(ws_echo))
        .route("/api/serve/status", get(api_serve_status))
        .route("/api/serve/logs", get(api_serve_logs))
        .route("/api/remote-control", get(api_rc_list))
        .route(
            "/api/remote-control/{name}",
            post(api_rc_enable).delete(api_rc_disable).patch(api_rc_update_mode),
        )
        .layer(CorsLayer::permissive());

    let app = if let Some(dir) = static_dir {
        api.fallback_service(ServeDir::new(dir).append_index_html_on_directories(true))
    } else {
        api
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("HTTP server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn api_services() -> Json<Vec<services::ServiceInfo>> {
    Json(services::list_services())
}

async fn api_service_detail(Path(name): Path<String>) -> impl IntoResponse {
    match services::get_service_detail(&name) {
        Ok(detail) => (StatusCode::OK, Json(serde_json::to_value(detail).unwrap())),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e })),
        ),
    }
}

async fn api_start(Path(name): Path<String>) -> impl IntoResponse {
    eprintln!("[API] start service: {}", name);
    match services::start_service(&name) {
        Ok(msg) => {
            eprintln!("[API] start service {} -> success: {}", name, msg);
            (StatusCode::OK, Json(serde_json::json!({ "message": msg })))
        }
        Err(e) => {
            eprintln!("[API] start service {} -> error: {}", name, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        }
    }
}

async fn api_stop(Path(name): Path<String>) -> impl IntoResponse {
    eprintln!("[API] stop service: {}", name);
    match services::stop_service(&name) {
        Ok(msg) => {
            eprintln!("[API] stop service {} -> success: {}", name, msg);
            (StatusCode::OK, Json(serde_json::json!({ "message": msg })))
        }
        Err(e) => {
            eprintln!("[API] stop service {} -> error: {}", name, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        }
    }
}

async fn api_reload(Path(name): Path<String>) -> impl IntoResponse {
    eprintln!("[API] reload service: {}", name);
    match services::reload_service(&name) {
        Ok(msg) => {
            eprintln!("[API] reload service {} -> success: {}", name, msg);
            (StatusCode::OK, Json(serde_json::json!({ "message": msg })))
        }
        Err(e) => {
            eprintln!("[API] reload service {} -> error: {}", name, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        }
    }
}

async fn api_restart_process(Path((name, process)): Path<(String, String)>) -> impl IntoResponse {
    eprintln!("[API] restart process: {} / {}", name, process);
    match services::restart_process(&name, &process) {
        Ok(msg) => {
            eprintln!("[API] restart process {} / {} -> success: {}", name, process, msg);
            (StatusCode::OK, Json(serde_json::json!({ "message": msg })))
        }
        Err(e) => {
            eprintln!("[API] restart process {} / {} -> error: {}", name, process, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        }
    }
}

async fn api_kill_process(Path((name, process)): Path<(String, String)>) -> impl IntoResponse {
    eprintln!("[API] kill process: {} / {}", name, process);
    match services::kill_process(&name, &process) {
        Ok(msg) => {
            eprintln!("[API] kill process {} / {} -> success: {}", name, process, msg);
            (StatusCode::OK, Json(serde_json::json!({ "message": msg })))
        }
        Err(e) => {
            eprintln!("[API] kill process {} / {} -> error: {}", name, process, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        }
    }
}

async fn api_echo(Path(name): Path<String>) -> impl IntoResponse {
    match tmux::capture_all_panes(&name) {
        Ok(content) => (StatusCode::OK, content),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn api_panes(Path(name): Path<String>) -> Json<Vec<tmux::TmuxPane>> {
    Json(tmux::list_panes(&name))
}

async fn api_capture_pane(
    Path((name, window, pane)): Path<(String, u32, u32)>,
) -> impl IntoResponse {
    match tmux::capture_pane(&name, window, pane) {
        Ok(content) => (StatusCode::OK, content),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn ws_echo(Path(name): Path<String>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_echo_ws(socket, name))
}

async fn handle_echo_ws(mut socket: WebSocket, name: String) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(200));
    let mut last_content = String::new();

    loop {
        tokio::select! {
            _ = interval.tick() => {
                match tmux::capture_all_panes(&name) {
                    Ok(content) => {
                        if content != last_content {
                            last_content = content.clone();
                            if socket.send(Message::Text(content.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

fn home_dir() -> PathBuf {
    PathBuf::from(env::var("HOME").expect("HOME not set"))
}

fn serve_state_file() -> PathBuf {
    home_dir().join(".local/share/kagaya/serve-state")
}

fn serve_log_file() -> Option<PathBuf> {
    fs::read_to_string(serve_state_file())
        .ok()
        .and_then(|s| s.lines().next().map(|l| PathBuf::from(l)))
}

async fn api_serve_status() -> Json<serde_json::Value> {
    let output = Command::new("lsof")
        .args(["-ti", ":13369", "-sTCP:LISTEN"])
        .output();
    
    let running = output
        .ok()
        .and_then(|out| {
            if out.stdout.is_empty() {
                None
            } else {
                let pid_str = String::from_utf8_lossy(&out.stdout);
                pid_str.trim().parse::<u32>().ok()
            }
        })
        .is_some();
    
    let log_file = serve_log_file()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    
    Json(serde_json::json!({
        "running": running,
        "log_file": log_file
    }))
}

async fn api_serve_logs() -> impl IntoResponse {
    match serve_log_file() {
        Some(log_file) if log_file.exists() => {
            match fs::read_to_string(&log_file) {
                Ok(content) => (StatusCode::OK, content),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to read log file: {e}")
                ),
            }
        }
        Some(log_file) => (
            StatusCode::NOT_FOUND,
            format!("log file not found: {}", log_file.display())
        ),
        None => (
            StatusCode::NOT_FOUND,
            "no log file recorded".to_string()
        ),
    }
}

async fn api_rc_list() -> impl IntoResponse {
	match remote_control::list().await {
		Ok(projects) => (StatusCode::OK, Json(serde_json::to_value(projects).unwrap())),
		Err(e) => (
			StatusCode::INTERNAL_SERVER_ERROR,
			Json(serde_json::json!({ "error": e })),
		),
	}
}

#[derive(serde::Deserialize)]
struct RcEnableBody {
	dir: String,
	mode: String,
}

async fn api_rc_enable(Path(name): Path<String>, Json(body): Json<RcEnableBody>) -> impl IntoResponse {
	eprintln!("[API] rc enable: {}", name);
	match remote_control::enable(name.clone(), body.dir, body.mode).await {
		Ok(msg) => {
			eprintln!("[API] rc enable {} -> success: {}", name, msg);
			(StatusCode::OK, Json(serde_json::json!({ "message": msg })))
		}
		Err(e) => {
			eprintln!("[API] rc enable {} -> error: {}", name, e);
			(
				StatusCode::INTERNAL_SERVER_ERROR,
				Json(serde_json::json!({ "error": e })),
			)
		}
	}
}

async fn api_rc_disable(Path(name): Path<String>) -> impl IntoResponse {
	eprintln!("[API] rc disable: {}", name);
	match remote_control::disable(name.clone()).await {
		Ok(msg) => {
			eprintln!("[API] rc disable {} -> success: {}", name, msg);
			(StatusCode::OK, Json(serde_json::json!({ "message": msg })))
		}
		Err(e) => {
			eprintln!("[API] rc disable {} -> error: {}", name, e);
			(
				StatusCode::INTERNAL_SERVER_ERROR,
				Json(serde_json::json!({ "error": e })),
			)
		}
	}
}

#[derive(serde::Deserialize)]
struct RcUpdateModeBody {
	mode: String,
}

async fn api_rc_update_mode(
	Path(name): Path<String>,
	Json(body): Json<RcUpdateModeBody>,
) -> impl IntoResponse {
	eprintln!("[API] rc update_mode: {} -> {}", name, body.mode);
	match remote_control::update_mode(name.clone(), body.mode).await {
		Ok(msg) => {
			eprintln!("[API] rc update_mode {} -> success: {}", name, msg);
			(StatusCode::OK, Json(serde_json::json!({ "message": msg })))
		}
		Err(e) => {
			eprintln!("[API] rc update_mode {} -> error: {}", name, e);
			(
				StatusCode::INTERNAL_SERVER_ERROR,
				Json(serde_json::json!({ "error": e })),
			)
		}
	}
}
