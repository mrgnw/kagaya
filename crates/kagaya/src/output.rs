use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::Mutex;

use crate::logs;

const RING_BUFFER_SIZE: usize = 64 * 1024;

/// Ring buffer state with monotonic position tracking.
struct RingState {
	buf: VecDeque<u8>,
	/// Total bytes ever written — monotonically increasing, never resets.
	total_written: u64,
}

/// Captures process output to a ring buffer, log file, and broadcast channel.
///
/// - **Ring buffer**: 64KB in-memory for instant snapshots
/// - **Log file**: Appends to disk with automatic rotation by size
/// - **Broadcast**: Live streaming to subscribers (e.g. WebSocket)
#[derive(Clone)]
pub struct OutputCapture {
	ring: Arc<Mutex<RingState>>,
	log_writer: Arc<Mutex<LogWriter>>,
	sender: broadcast::Sender<Vec<u8>>,
}

struct LogWriter {
	file: Option<File>,
	path: PathBuf,
	bytes_written: u64,
	max_size: u64,
	log_dir: PathBuf,
	process: String,
}

impl OutputCapture {
	/// Create a new output capture, opening a log file in `{log_dir}/{service}/`.
	pub fn new(log_dir: &Path, service: &str, process: &str, max_log_size: u64) -> Self {
		let svc_log_dir = logs::service_log_dir(log_dir, service);
		let _ = fs::create_dir_all(&svc_log_dir);

		let log_name = logs::current_log_name(process);
		let log_path = svc_log_dir.join(&log_name);

		let file = OpenOptions::new()
			.create(true)
			.append(true)
			.open(&log_path)
			.ok();

		let bytes_written = file
			.as_ref()
			.and_then(|f| f.metadata().ok())
			.map(|m| m.len())
			.unwrap_or(0);

		let (sender, _) = broadcast::channel(256);

		Self {
			ring: Arc::new(Mutex::new(RingState {
				buf: VecDeque::with_capacity(RING_BUFFER_SIZE),
				total_written: 0,
			})),
			log_writer: Arc::new(Mutex::new(LogWriter {
				file,
				path: log_path,
				bytes_written,
				max_size: max_log_size,
				log_dir: svc_log_dir,
				process: process.to_string(),
			})),
			sender,
		}
	}

	/// Write data to ring buffer, log file, and broadcast channel.
	pub async fn write(&self, data: &[u8]) {
		{
			let mut state = self.ring.lock().await;
			for &byte in data {
				if state.buf.len() >= RING_BUFFER_SIZE {
					state.buf.pop_front();
				}
				state.buf.push_back(byte);
			}
			state.total_written += data.len() as u64;
		}

		{
			let mut writer = self.log_writer.lock().await;
			writer.write(data);
		}

		let _ = self.sender.send(data.to_vec());
	}

	/// Get a snapshot of the ring buffer contents.
	pub async fn snapshot(&self) -> Vec<u8> {
		let state = self.ring.lock().await;
		state.buf.iter().copied().collect()
	}

	/// Get bytes written after `offset`, returning (data, new_offset).
	/// If offset is stale or zero, returns only the last ~4KB to avoid
	/// dumping the entire ring buffer (e.g. from crash-loop history).
	pub async fn snapshot_from(&self, offset: u64) -> (Vec<u8>, u64) {
		const INITIAL_CAP: usize = 4 * 1024;

		let state = self.ring.lock().await;
		let new_offset = state.total_written;
		if offset >= new_offset {
			return (Vec::new(), new_offset);
		}
		let available_from = new_offset.saturating_sub(state.buf.len() as u64);
		let skip = if offset > available_from {
			(offset - available_from) as usize
		} else {
			state.buf.len().saturating_sub(INITIAL_CAP)
		};
		let data: Vec<u8> = state.buf.iter().skip(skip).copied().collect();
		(data, new_offset)
	}

	/// Subscribe to live output updates.
	pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
		self.sender.subscribe()
	}
}

impl LogWriter {
	fn write(&mut self, data: &[u8]) {
		if let Some(ref mut file) = self.file {
			let _ = file.write_all(data);

			self.bytes_written += data.len() as u64;

			if self.bytes_written >= self.max_size {
				self.rotate();
			}
		}
	}

	fn rotate(&mut self) {
		if let Some(file) = self.file.take() {
			drop(file);
		}

		let rotated_name = logs::rotated_log_name(&self.log_dir, &self.process);
		let rotated_path = self.log_dir.join(&rotated_name);
		let _ = fs::rename(&self.path, &rotated_path);

		let new_name = logs::current_log_name(&self.process);
		self.path = self.log_dir.join(&new_name);
		self.file = OpenOptions::new()
			.create(true)
			.append(true)
			.open(&self.path)
			.ok();
		self.bytes_written = 0;
	}
}
