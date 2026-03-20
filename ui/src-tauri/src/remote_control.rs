use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Serialize)]
enum Request {
	List,
	Enable { name: String, dir: String, mode: String },
	Disable { name: String },
	UpdateMode { name: String, mode: String },
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ProjectStatus {
	pub name: String,
	pub dir: String,
	pub mode: String,
	pub running: bool,
	pub pid: Option<u32>,
}

#[derive(Deserialize)]
enum Response {
	Ok { message: String },
	ProjectList(Vec<ProjectStatus>),
	Error { message: String },
}

fn socket_path() -> PathBuf {
	PathBuf::from(env::var("HOME").expect("HOME not set"))
		.join(".local/state/claude-rc/daemon.sock")
}

async fn send_request(req: &Request) -> Result<Response, String> {
	let path = socket_path();
	let stream = UnixStream::connect(&path)
		.await
		.map_err(|e| format!("failed to connect to claude-rc daemon: {e}"))?;

	let (reader, mut writer) = stream.into_split();

	let mut json = serde_json::to_string(req).map_err(|e| format!("serialize error: {e}"))?;
	json.push('\n');
	writer
		.write_all(json.as_bytes())
		.await
		.map_err(|e| format!("write error: {e}"))?;

	let mut buf_reader = BufReader::new(reader);
	let mut line = String::new();
	buf_reader
		.read_line(&mut line)
		.await
		.map_err(|e| format!("read error: {e}"))?;

	if line.is_empty() {
		return Err("daemon closed connection without response".to_string());
	}

	serde_json::from_str(&line).map_err(|e| format!("parse error: {e}"))
}

pub async fn list() -> Result<Vec<ProjectStatus>, String> {
	match send_request(&Request::List).await? {
		Response::ProjectList(projects) => Ok(projects),
		Response::Ok { message } => Err(format!("unexpected Ok response: {message}")),
		Response::Error { message } => Err(message),
	}
}

pub async fn enable(name: String, dir: String, mode: String) -> Result<String, String> {
	match send_request(&Request::Enable { name, dir, mode }).await? {
		Response::Ok { message } => Ok(message),
		Response::Error { message } => Err(message),
		Response::ProjectList(_) => Err("unexpected ProjectList response".to_string()),
	}
}

pub async fn disable(name: String) -> Result<String, String> {
	match send_request(&Request::Disable { name }).await? {
		Response::Ok { message } => Ok(message),
		Response::Error { message } => Err(message),
		Response::ProjectList(_) => Err("unexpected ProjectList response".to_string()),
	}
}

pub async fn update_mode(name: String, mode: String) -> Result<String, String> {
	match send_request(&Request::UpdateMode { name, mode }).await? {
		Response::Ok { message } => Ok(message),
		Response::Error { message } => Err(message),
		Response::ProjectList(_) => Err("unexpected ProjectList response".to_string()),
	}
}
