pub mod api;
pub mod supervisor;

use std::sync::Arc;
use crate::config;
use crate::protocol::{Request, Response};

pub async fn run(args: &[String]) {
	tracing_subscriber::fmt().init();

	let _foreground = args.iter().any(|a| a == "--foreground" || a == "-f");
	let enable_http = args.iter().any(|a| a == "--http");
	let port_override = args.windows(2)
		.find(|w| w[0] == "--port" || w[0] == "-p")
		.and_then(|w| w[1].parse::<u16>().ok());

	let global_config = config::load_global_config();

	#[cfg(feature = "dev")]
	let port = port_override.unwrap_or(13370);
	#[cfg(not(feature = "dev"))]
	let port = port_override.unwrap_or(global_config.daemon.port);

	let http_port = if enable_http { Some(port) } else { None };
	let supervisor = supervisor::Supervisor::new(global_config.clone(), http_port);

	let paths = muzan::DaemonPaths::new("kagaya");

	let state_dir = paths.state_dir();
	let _ = std::fs::create_dir_all(&state_dir);

	let pid_path = paths.pid_path();
	let _ = std::fs::write(&pid_path, std::process::id().to_string());

	let socket_path = paths.socket_path();
	if socket_path.exists() {
		let _ = std::fs::remove_file(&socket_path);
	}

	let log_dir = crate::logs::log_dir();
	kagaya::logs::expire_logs(&log_dir, global_config.logs.max_age_days, global_config.logs.max_files);

	{
		let config = global_config.clone();
		let log_dir = log_dir.clone();
		tokio::spawn(async move {
			loop {
				tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
				kagaya::logs::expire_logs(&log_dir, config.logs.max_age_days, config.logs.max_files);
			}
		});
	}

	let shutdown = Arc::new(tokio::sync::Notify::new());

	let sup_socket = Arc::clone(&supervisor);
	let shutdown_socket = Arc::clone(&shutdown);
	let paths_socket = paths.clone();
	let socket_handle = tokio::spawn(async move {
		muzan::server::run_socket_server_with_error(
			&paths_socket,
			move |req: Request| {
				let sup = Arc::clone(&sup_socket);
				let shutdown = Arc::clone(&shutdown_socket);
				async move { handle_request(&sup, req, &shutdown).await }
			},
			Some(|msg: String| Response::Error { message: msg }),
		)
		.await;
	});

	let http_handle = if enable_http {
		let sup_http = Arc::clone(&supervisor);
		Some(tokio::spawn(async move {
			run_http_server(sup_http, port).await;
		}))
	} else {
		None
	};

	#[cfg(feature = "dev")]
	{
		let ui_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../ui");
		tracing::info!("spawning vite dev server in {}", ui_dir.display());
		tokio::spawn(async move {
			let mut child = tokio::process::Command::new("pnpm")
				.args(["dev"])
				.current_dir(&ui_dir)
				.spawn()
				.expect("failed to start vite dev server");
			child.wait().await.ok();
		});
	}

	tracing::info!("daemon started (pid {})", std::process::id());
	if enable_http {
		tracing::info!("HTTP server on port {}", port);
	}

	tokio::select! {
		_ = socket_handle => {},
		_ = async {
			if let Some(h) = http_handle { h.await.ok(); }
			else { std::future::pending::<()>().await; }
		} => {},
		_ = tokio::signal::ctrl_c() => {
			tracing::info!("shutting down");
		}
		_ = shutdown.notified() => {
			tracing::info!("shutdown requested");
		}
	}

	// Gracefully stop all supervised processes
	let services: Vec<String> = supervisor
		.inner
		.services
		.read()
		.await
		.keys()
		.cloned()
		.collect();
	for name in &services {
		let _ = supervisor.stop_service(name).await;
	}

	let _ = std::fs::remove_file(paths.socket_path());
	let _ = std::fs::remove_file(paths.pid_path());
}

async fn handle_request(supervisor: &Arc<supervisor::Supervisor>, request: Request, shutdown: &Arc<tokio::sync::Notify>) -> Response {
	match request {
		Request::Ping => Response::Pong,
		Request::Status => {
			let services = supervisor.status().await;
			Response::Status { services, http_port: supervisor.http_port }
		}
		Request::Start { names, all, processes, chains, wait } => {
			// Build cross-project dependency map from chains
			let mut project_deps: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
			for chain in &chains {
				for i in 1..chain.len() {
					// chain[i] depends on chain[i-1] (if both are project names)
					if names.contains(&chain[i]) && names.contains(&chain[i - 1]) {
						project_deps.entry(chain[i].clone()).or_default().push(chain[i - 1].clone());
					}
				}
			}

			if project_deps.is_empty() {
				// No cross-project deps — start all in parallel as before
				let mut messages = Vec::new();
				for name in &names {
					// Filter chains to only intra-project ones
					let intra_chains: Vec<Vec<String>> = chains.iter()
						.filter(|c| !c.iter().any(|n| names.contains(n) && n != name))
						.cloned()
						.collect();
					match supervisor.start_service_filtered(name, all, &processes, &intra_chains).await {
						Ok(msg) => messages.push(msg),
						Err(e) => return Response::Error { message: e },
					}
				}
				if wait {
					for name in &names {
						supervisor.wait_for_ready(name).await;
					}
				}
				Response::Ok {
					message: Some(messages.join("\n")),
				}
			} else {
				// Start projects respecting cross-project dependency order
				let mut started: std::collections::HashSet<String> = std::collections::HashSet::new();
				let mut messages = Vec::new();
				let mut remaining: Vec<String> = names.clone();

				while !remaining.is_empty() {
					// Find projects whose deps are all started and ready
					let ready_to_start: Vec<String> = remaining.iter()
						.filter(|name| {
							project_deps.get(*name)
								.map(|deps| deps.iter().all(|d| started.contains(d)))
								.unwrap_or(true)
						})
						.cloned()
						.collect();

					if ready_to_start.is_empty() {
						// Shouldn't happen if deps are valid, but prevent infinite loop
						for name in &remaining {
							messages.push(format!("{}: skipped (unresolvable dependency)", name));
						}
						break;
					}

					for name in &ready_to_start {
						match supervisor.start_service_filtered(name, all, &processes, &[]).await {
							Ok(msg) => messages.push(msg),
							Err(e) => messages.push(format!("{}: error: {}", name, e)),
						}
					}

					// Wait for this wave to be ready before starting dependents
					let has_dependents = remaining.iter().any(|n| !ready_to_start.contains(n));
					if has_dependents {
						for name in &ready_to_start {
							supervisor.wait_for_ready(name).await;
						}
					}

					for name in &ready_to_start {
						started.insert(name.clone());
					}
					remaining.retain(|n| !ready_to_start.contains(n));
				}

				if wait {
					for name in &names {
						supervisor.wait_for_ready(name).await;
					}
				}
				Response::Ok {
					message: Some(messages.join("\n")),
				}
			}
		}
		Request::Stop { names, processes } => {
			let mut messages = Vec::new();
			for name in &names {
				match supervisor.stop_service_filtered(name, &processes).await {
					Ok(msg) => messages.push(msg),
					Err(e) => return Response::Error { message: e },
				}
			}
			Response::Ok {
				message: Some(messages.join("\n")),
			}
		}
		Request::Reload { names, all, processes } => {
			let mut messages = Vec::new();
			for name in &names {
				match supervisor.reload_service_filtered(name, all, &processes).await {
					Ok(msg) => messages.push(msg),
					Err(e) => return Response::Error { message: e },
				}
			}
			Response::Ok {
				message: Some(messages.join("\n")),
			}
		}
		Request::Restart { service, process } => {
			match supervisor.restart_process(&service, &process).await {
				Ok(msg) => Response::Ok { message: Some(msg) },
				Err(e) => Response::Error { message: e },
			}
		}
		Request::Kill { service, process } => {
			match supervisor.kill_process(&service, &process).await {
				Ok(msg) => Response::Ok { message: Some(msg) },
				Err(e) => Response::Error { message: e },
			}
		}
		Request::ReloadConfig => {
			// Config is already loaded fresh on each operation, so we just verify it's valid
			match std::panic::catch_unwind(|| config::load_service_entries()) {
				Ok(_) => Response::Ok {
					message: Some("projects.toml reloaded successfully".to_string()),
				},
				Err(_) => Response::Error {
					message: "failed to parse projects.toml".to_string(),
				},
			}
		}
		Request::Logs { service, process, follow: _, offset } => {
			match supervisor.get_output(&service, process.as_deref()).await {
				Ok(capture) => {
					let (data, new_offset) = capture.snapshot_from(offset).await;
					Response::Log {
						line: String::from_utf8_lossy(&data).to_string(),
						offset: new_offset,
					}
				}
				Err(e) => Response::Error { message: e },
			}
		}
		Request::Shutdown => {
			shutdown.notify_one();
			Response::Ok {
				message: Some("shutting down".to_string()),
			}
		}
	}
}

async fn run_http_server(supervisor: Arc<supervisor::Supervisor>, port: u16) {
	let app = api::router(supervisor);
	let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
	let listener = match tokio::net::TcpListener::bind(addr).await {
		Ok(l) => l,
		Err(e) => {
			tracing::error!("failed to bind HTTP on {}: {}", addr, e);
			return;
		}
	};
	tracing::info!("HTTP listening on {}", addr);
	if let Err(e) = axum::serve(listener, app).await {
		tracing::error!("HTTP server error: {}", e);
	}
}
