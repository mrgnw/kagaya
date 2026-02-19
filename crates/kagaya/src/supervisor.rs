use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;

use crate::output::OutputCapture;
use crate::types::*;

/// Configuration for the supervisor.
pub struct SupervisorConfig {
	pub log_dir: PathBuf,
	pub max_log_size: u64,
}

/// Core process supervisor. Manages named services, each with one or more processes.
pub struct Supervisor {
	pub services: Arc<RwLock<HashMap<String, ManagedService>>>,
	pub config: SupervisorConfig,
}

/// A service being managed by the supervisor.
pub struct ManagedService {
	pub dir: PathBuf,
	pub processes: HashMap<String, ManagedProcess>,
}

/// A process being managed within a service.
pub struct ManagedProcess {
	pub def: ProcessDef,
	pub state: ProcessState,
	pub output: OutputCapture,
	pub retry_count: u32,
	cancel: Option<tokio::sync::watch::Sender<bool>>,
}

impl Supervisor {
	pub fn new(config: SupervisorConfig) -> Arc<Self> {
		Arc::new(Self {
			services: Arc::new(RwLock::new(HashMap::new())),
			config,
		})
	}

	/// Get status of all managed services.
	pub async fn status(&self) -> Vec<ServiceStatus> {
		let services = self.services.read().await;
		let mut result = Vec::new();

		for (name, managed) in services.iter() {
			let processes = managed
				.processes
				.iter()
				.map(|(pname, mp)| {
					let pid = match &mp.state {
						ProcessState::Running { pid, .. } => Some(*pid),
						_ => None,
					};
					ProcessStatus {
						name: pname.clone(),
						state: mp.state.clone(),
						pid,
						autostart: mp.def.autostart,
						service_type: mp.def.service_type.clone(),
						ports: vec![],
					}
				})
				.collect();
			result.push(ServiceStatus {
				name: name.clone(),
				dir: managed.dir.clone(),
				processes,
			});
		}
		result
	}

	/// Start a service with the given process definitions.
	///
	/// `filter` limits which processes start; empty means use `all` flag or `autostart`.
	pub async fn start_service(
		self: &Arc<Self>,
		name: &str,
		dir: &Path,
		process_defs: &[ProcessDef],
		all: bool,
		filter: &[String],
	) -> Result<String, String> {
		// If the service is already managed and specific processes are requested,
		// start only those stopped/failed processes within the existing service.
		if !filter.is_empty() {
			let mut services = self.services.write().await;
			if let Some(managed) = services.get_mut(name) {
				let mut started = Vec::new();
				for proc_name in filter {
					let mp = match managed.processes.get_mut(proc_name.as_str()) {
						Some(mp) => mp,
						None => return Err(format!("{}/{}: not found", name, proc_name)),
					};
					if mp.state.is_running() {
						started.push(format!("{}/{}: already running", name, proc_name));
						continue;
					}
					// Reset and start this process
					if let Some(cancel) = mp.cancel.take() {
						let _ = cancel.send(true);
					}
					mp.state = ProcessState::Stopped;
					mp.retry_count = 0;

					let output = OutputCapture::new(
						&self.config.log_dir, name, proc_name, self.config.max_log_size,
					);
					let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
					mp.output = output.clone();
					mp.cancel = Some(cancel_tx);

					let sup = Arc::clone(self);
					let svc = name.to_string();
					let pname = proc_name.clone();
					let def = mp.def.clone();
					let d = dir.to_path_buf();
					tokio::spawn(async move {
						run_process_loop(sup, svc, pname, def, d, output, cancel_rx).await;
					});
					started.push(format!("{}/{}: starting", name, proc_name));
				}
				return Ok(started.join("\n"));
			}
		}

		{
			let services = self.services.read().await;
			if let Some(managed) = services.get(name) {
				if managed.processes.values().any(|p| p.state.is_running()) {
					return Ok(format!("{}: already running", name));
				}
			}
		}

		if process_defs.is_empty() {
			return Err(format!("{}: no processes defined", name));
		}

		let mut managed_processes = HashMap::new();

		for proc_def in process_defs {
			let should_start = if !filter.is_empty() {
				filter.iter().any(|p| p == &proc_def.name)
			} else if all {
				true
			} else {
				proc_def.autostart
			};

			let output = OutputCapture::new(
				&self.config.log_dir,
				name,
				&proc_def.name,
				self.config.max_log_size,
			);
			let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

			let mp = ManagedProcess {
				def: proc_def.clone(),
				state: ProcessState::Stopped,
				output: output.clone(),
				retry_count: 0,
				cancel: Some(cancel_tx),
			};
			managed_processes.insert(proc_def.name.clone(), mp);

			if should_start {
				let sup = Arc::clone(self);
				let service_name = name.to_string();
				let process_name = proc_def.name.clone();
				let proc_def_clone = proc_def.clone();
				let dir = dir.to_path_buf();

				tokio::spawn(async move {
					run_process_loop(
						sup,
						service_name,
						process_name,
						proc_def_clone,
						dir,
						output,
						cancel_rx,
					)
					.await;
				});
			}
		}

		{
			let mut services = self.services.write().await;
			services.insert(
				name.to_string(),
			ManagedService {
				dir: dir.to_path_buf(),
					processes: managed_processes,
				},
			);
		}

		Ok(format!("{}: starting", name))
	}

	/// Stop all processes in a service. Sends SIGTERM then SIGKILL after 3s.
	pub async fn stop_service(self: &Arc<Self>, name: &str) -> Result<String, String> {
		let mut services = self.services.write().await;
		let managed = services
			.get_mut(name)
			.ok_or_else(|| format!("{}: not running", name))?;

		let mut any_running = false;
		for (_, mp) in managed.processes.iter_mut() {
			if mp.state.is_running() {
				any_running = true;
				if let Some(cancel) = mp.cancel.take() {
					let _ = cancel.send(true);
				}
				if let ProcessState::Running { pid, .. } = &mp.state {
					kill_process_tree(*pid);
				}
				mp.state = ProcessState::Stopped;
			}
		}

		if !any_running {
			return Ok(format!("{}: already stopped", name));
		}

		services.remove(name);
		Ok(format!("{}: stopped", name))
	}

	/// Stop specific processes within a service.
	pub async fn stop_processes(
		self: &Arc<Self>,
		name: &str,
		processes: &[String],
	) -> Result<String, String> {
		let mut services = self.services.write().await;
		let managed = services
			.get_mut(name)
			.ok_or_else(|| format!("{}: not running", name))?;

		let mut messages = Vec::new();
		for proc_name in processes {
			if let Some(mp) = managed.processes.get_mut(proc_name.as_str()) {
				if mp.state.is_running() {
					if let Some(cancel) = mp.cancel.take() {
						let _ = cancel.send(true);
					}
					if let ProcessState::Running { pid, .. } = &mp.state {
						kill_process_tree(*pid);
					}
					mp.state = ProcessState::Stopped;
					messages.push(format!("{}/{}: stopped", name, proc_name));
				} else {
					messages.push(format!("{}/{}: not running", name, proc_name));
				}
			} else {
				messages.push(format!("{}/{}: not found", name, proc_name));
			}
		}
		Ok(messages.join("\n"))
	}

	/// Stop then start a service (with 200ms gap).
	pub async fn reload_service(
		self: &Arc<Self>,
		name: &str,
		dir: &Path,
		process_defs: &[ProcessDef],
		all: bool,
		filter: &[String],
	) -> Result<String, String> {
		let _ = self.stop_service(name).await;
		tokio::time::sleep(std::time::Duration::from_millis(200)).await;
		self.start_service(name, dir, process_defs, all, filter).await
	}

	/// Restart a single process within a service.
	pub async fn restart_process(
		self: &Arc<Self>,
		service: &str,
		process: &str,
		dir: &Path,
	) -> Result<String, String> {
		let mut services = self.services.write().await;
		let managed = services
			.get_mut(service)
			.ok_or_else(|| format!("{}: not running", service))?;
		let mp = managed
			.processes
			.get_mut(process)
			.ok_or_else(|| format!("{}/{}: not found", service, process))?;

		if let Some(cancel) = mp.cancel.take() {
			let _ = cancel.send(true);
		}
		if let ProcessState::Running { pid, .. } = &mp.state {
			kill_process_tree(*pid);
		}
		mp.state = ProcessState::Stopped;
		mp.retry_count = 0;

		let output = OutputCapture::new(
			&self.config.log_dir,
			service,
			process,
			self.config.max_log_size,
		);
		let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
		mp.output = output.clone();
		mp.cancel = Some(cancel_tx);

		let sup = Arc::clone(self);
		let service_name = service.to_string();
		let process_name = process.to_string();
		let proc_def = mp.def.clone();
		let dir = dir.to_path_buf();

		tokio::spawn(async move {
			run_process_loop(sup, service_name, process_name, proc_def, dir, output, cancel_rx)
				.await;
		});

		Ok(format!("{}/{}: restarting", service, process))
	}

	/// Kill a single process without restarting.
	pub async fn kill_process(
		self: &Arc<Self>,
		service: &str,
		process: &str,
	) -> Result<String, String> {
		let mut services = self.services.write().await;
		let managed = services
			.get_mut(service)
			.ok_or_else(|| format!("{}: not running", service))?;
		let mp = managed
			.processes
			.get_mut(process)
			.ok_or_else(|| format!("{}/{}: not found", service, process))?;

		if let Some(cancel) = mp.cancel.take() {
			let _ = cancel.send(true);
		}
		if let ProcessState::Running { pid, .. } = &mp.state {
			kill_process_tree(*pid);
		}
		mp.state = ProcessState::Stopped;

		Ok(format!("{}/{}: killed", service, process))
	}

	/// Get the output capture for a process (or the first process if `process` is None).
	pub async fn get_output(
		&self,
		service: &str,
		process: Option<&str>,
	) -> Result<OutputCapture, String> {
		let services = self.services.read().await;
		let managed = services
			.get(service)
			.ok_or_else(|| format!("{}: not found", service))?;

		if let Some(proc_name) = process {
			let mp = managed
				.processes
				.get(proc_name)
				.ok_or_else(|| format!("{}/{}: not found", service, proc_name))?;
			Ok(mp.output.clone())
		} else {
			managed
				.processes
				.values()
				.next()
				.map(|mp| mp.output.clone())
				.ok_or_else(|| format!("{}: no processes", service))
		}
	}

	/// Get output captures for all processes in a service.
	pub async fn get_all_outputs(
		&self,
		service: &str,
	) -> Result<Vec<(String, OutputCapture)>, String> {
		let services = self.services.read().await;
		let managed = services
			.get(service)
			.ok_or_else(|| format!("{}: not found", service))?;
		Ok(managed
			.processes
			.iter()
			.map(|(name, mp)| (name.clone(), mp.output.clone()))
			.collect())
	}
}

async fn run_process_loop(
	supervisor: Arc<Supervisor>,
	service: String,
	process: String,
	def: ProcessDef,
	dir: PathBuf,
	output: OutputCapture,
	mut cancel: tokio::sync::watch::Receiver<bool>,
) {
	let mut retry_count: u32 = 0;

	loop {
		if *cancel.borrow() {
			return;
		}

		let child = spawn_process(&def, &dir).await;
		let mut child = match child {
			Ok(c) => c,
			Err(e) => {
				let msg = format!("[kagaya] failed to spawn {}/{}: {}\n", service, process, e);
				output.write(msg.as_bytes()).await;
				update_state(
					&supervisor,
					&service,
					&process,
					ProcessState::Failed { exit_code: -1 },
				)
				.await;
				return;
			}
		};

		let pid = match child.id() {
			Some(id) => id,
			None => {
				let msg = format!(
					"[kagaya] {}/{} exited before PID could be read\n",
					service, process
				);
				output.write(msg.as_bytes()).await;
				let exit_result = child.wait().await;
				let code = exit_result.ok().and_then(|s| s.code()).unwrap_or(-1);
				update_state(
					&supervisor,
					&service,
					&process,
					ProcessState::Failed { exit_code: code },
				)
				.await;
				return;
			}
		};
		let started_at = Instant::now();
		update_state(
			&supervisor,
			&service,
			&process,
			ProcessState::Running {
				pid,
				uptime_secs: 0,
			},
		)
		.await;

		if let Some(stdout) = child.stdout.take() {
			let out = output.clone();
			tokio::spawn(async move {
				pipe_output(stdout, out).await;
			});
		}
		if let Some(stderr) = child.stderr.take() {
			let out = output.clone();
			tokio::spawn(async move {
				pipe_output(stderr, out).await;
			});
		}

		let sup_clone = Arc::clone(&supervisor);
		let svc = service.clone();
		let proc_name = process.clone();
		let cancel_clone = cancel.clone();
		let uptime_handle = tokio::spawn(async move {
			loop {
				tokio::time::sleep(std::time::Duration::from_secs(1)).await;
				if *cancel_clone.borrow() {
					return;
				}
				let uptime = started_at.elapsed().as_secs();
				update_state(
					&sup_clone,
					&svc,
					&proc_name,
					ProcessState::Running {
						pid,
						uptime_secs: uptime,
					},
				)
				.await;
			}
		});

		let exit_result = tokio::select! {
			status = child.wait() => status,
			_ = cancel.changed() => {
				let _ = child.kill().await;
				uptime_handle.abort();
				return;
			}
		};

		uptime_handle.abort();

		match exit_result {
			Ok(exit) if exit.success() => {
				let msg = format!("[kagaya] {}/{} exited cleanly\n", service, process);
				output.write(msg.as_bytes()).await;
				update_state(&supervisor, &service, &process, ProcessState::Stopped).await;
				return;
			}
			Ok(exit) => {
				let code = exit.code().unwrap_or(-1);

				if def.service_type == ServiceType::Task {
					let msg =
						format!("[kagaya] {}/{} failed (exit {})\n", service, process, code);
					output.write(msg.as_bytes()).await;
					update_state(
						&supervisor,
						&service,
						&process,
						ProcessState::Failed { exit_code: code },
					)
					.await;
					return;
				}

				retry_count += 1;

				if def.restart && retry_count <= def.max_retries {
					let msg = format!(
						"[kagaya] {}/{} crashed (exit {}), restarting ({}/{})\n",
						service, process, code, retry_count, def.max_retries
					);
					output.write(msg.as_bytes()).await;
					update_state(
						&supervisor,
						&service,
						&process,
						ProcessState::Crashed {
							exit_code: code,
							retries: retry_count,
						},
					)
					.await;
					tokio::time::sleep(std::time::Duration::from_secs(def.restart_delay_secs))
						.await;
					continue;
				} else {
					let msg = format!(
						"[kagaya] {}/{} failed (exit {}), max retries exceeded\n",
						service, process, code
					);
					output.write(msg.as_bytes()).await;
					update_state(
						&supervisor,
						&service,
						&process,
						ProcessState::Failed { exit_code: code },
					)
					.await;
					return;
				}
			}
			Err(e) => {
				let msg = format!("[kagaya] {}/{} error: {}\n", service, process, e);
				output.write(msg.as_bytes()).await;
				update_state(
					&supervisor,
					&service,
					&process,
					ProcessState::Failed { exit_code: -1 },
				)
				.await;
				return;
			}
		}
	}
}

async fn spawn_process(def: &ProcessDef, dir: &Path) -> Result<Child, String> {
	let mut cmd = Command::new("sh");
	cmd.args(["-c", &def.command])
		.current_dir(dir)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.process_group(0);

	for (key, val) in &def.env {
		cmd.env(key, val);
	}

	cmd.spawn().map_err(|e| format!("spawn failed: {}", e))
}

async fn pipe_output<R: tokio::io::AsyncRead + Unpin>(mut reader: R, output: OutputCapture) {
	let mut buf = [0u8; 4096];
	loop {
		match reader.read(&mut buf).await {
			Ok(0) => break,
			Ok(n) => output.write(&buf[..n]).await,
			Err(_) => break,
		}
	}
}

async fn update_state(
	supervisor: &Arc<Supervisor>,
	service: &str,
	process: &str,
	state: ProcessState,
) {
	let mut services = supervisor.services.write().await;
	if let Some(managed) = services.get_mut(service) {
		if let Some(mp) = managed.processes.get_mut(process) {
			mp.state = state;
		}
	}
}

/// Send SIGTERM to a process group, then SIGKILL after 3 seconds.
pub fn kill_process_tree(pid: u32) {
	if pid == 0 {
		return;
	}
	use nix::sys::signal::{killpg, Signal};
	use nix::unistd::Pid;
	let pgid = Pid::from_raw(pid as i32);
	let _ = killpg(pgid, Signal::SIGTERM);
	std::thread::spawn(move || {
		std::thread::sleep(std::time::Duration::from_secs(3));
		let _ = killpg(pgid, Signal::SIGKILL);
	});
}
