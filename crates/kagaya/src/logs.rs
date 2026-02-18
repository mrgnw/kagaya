use std::path::PathBuf;

/// Log directory for a specific service: `{log_dir}/{service}/`.
pub fn service_log_dir(log_dir: &std::path::Path, service: &str) -> PathBuf {
	log_dir.join(service)
}

/// Generate log filename for today: `{process} {YY-MMDD}.log`.
pub fn current_log_name(process: &str) -> String {
	let now = now_ymd();
	format!("{} {}.log", process, now)
}

/// Generate a rotated log filename with hour (and minute if needed for uniqueness).
pub fn rotated_log_name(log_dir: &std::path::Path, process: &str) -> String {
	let (date, hour, minute) = now_ymdhm();
	let candidate = format!("{} {} {}.log", process, date, hour);
	let candidate_path = log_dir.join(&candidate);
	if candidate_path.exists() {
		format!("{} {} {}.{}.log", process, date, hour, minute)
	} else {
		candidate
	}
}

/// Parse `(year, month, day)` from a log filename like `web 26-0214.log`.
pub fn parse_log_date(filename: &str) -> Option<(u32, u32, u32)> {
	let parts: Vec<&str> = filename.splitn(2, ' ').collect();
	if parts.len() < 2 {
		return None;
	}
	let rest = parts[1];
	let date_str = rest
		.split(' ')
		.next()
		.unwrap_or(rest)
		.trim_end_matches(".log");

	let parts: Vec<&str> = date_str.splitn(2, '-').collect();
	if parts.len() != 2 {
		return None;
	}
	let year: u32 = parts[0].parse().ok()?;
	let mmdd = parts[1];
	if mmdd.len() != 4 {
		return None;
	}
	let month: u32 = mmdd[..2].parse().ok()?;
	let day: u32 = mmdd[2..].parse().ok()?;
	Some((year, month, day))
}

/// Delete old logs: remove files older than `max_age_days` and trim to `max_files` per service.
pub fn expire_logs(log_dir: &std::path::Path, max_age_days: u32, max_files: u32) {
	if !log_dir.exists() {
		return;
	}

	let entries = match std::fs::read_dir(log_dir) {
		Ok(e) => e,
		Err(_) => return,
	};

	for entry in entries.flatten() {
		if !entry.path().is_dir() {
			continue;
		}
		expire_service_logs(&entry.path(), max_age_days, max_files);
	}
}

fn expire_service_logs(dir: &std::path::Path, max_age_days: u32, max_files: u32) {
	let mut log_files: Vec<(PathBuf, Option<(u32, u32, u32)>)> = Vec::new();

	let entries = match std::fs::read_dir(dir) {
		Ok(e) => e,
		Err(_) => return,
	};

	for entry in entries.flatten() {
		let path = entry.path();
		if path.extension().and_then(|e| e.to_str()) != Some("log") {
			continue;
		}
		let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
		let date = parse_log_date(&name);
		log_files.push((path, date));
	}

	if max_age_days > 0 {
		let now_secs = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();
		let cutoff_secs = now_secs.saturating_sub(max_age_days as u64 * 86400);

		for (path, date) in &log_files {
			if let Some((y, m, d)) = date {
				let file_epoch = date_to_epoch(*y, *m, *d);
				if file_epoch < cutoff_secs {
					let _ = std::fs::remove_file(path);
				}
			}
		}
	}

	if max_files > 0 && log_files.len() > max_files as usize {
		log_files.sort_by(|a, b| {
			let a_time = a.0.metadata().and_then(|m| m.modified()).ok();
			let b_time = b.0.metadata().and_then(|m| m.modified()).ok();
			a_time.cmp(&b_time)
		});
		let to_remove = log_files.len() - max_files as usize;
		for (path, _) in log_files.iter().take(to_remove) {
			let _ = std::fs::remove_file(path);
		}
	}
}

/// Convert Unix timestamp to `(year, month, day, hour, minute)`.
pub fn secs_to_datetime(secs: u64) -> (u32, u32, u32, u32, u32) {
	let days = (secs / 86400) as i64;
	let time_of_day = secs % 86400;
	let hour = (time_of_day / 3600) as u32;
	let minute = ((time_of_day % 3600) / 60) as u32;

	let z = days + 719468;
	let era = if z >= 0 { z } else { z - 146096 } / 146097;
	let doe = (z - era * 146097) as u32;
	let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
	let y = yoe as i64 + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = if mp < 10 { mp + 3 } else { mp - 9 };
	let y = if m <= 2 { y + 1 } else { y };

	(y as u32, m, d, hour, minute)
}

fn now_ymd() -> String {
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_secs();
	let (year, month, day, _, _) = secs_to_datetime(now);
	format!("{:02}-{:02}{:02}", year % 100, month, day)
}

fn now_ymdhm() -> (String, String, String) {
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_secs();
	let (year, month, day, hour, minute) = secs_to_datetime(now);
	(
		format!("{:02}-{:02}{:02}", year % 100, month, day),
		format!("{:02}", hour),
		format!("{:02}", minute),
	)
}

fn date_to_epoch(year: u32, month: u32, day: u32) -> u64 {
	let full_year = if year < 100 { 2000 + year } else { year };
	let y = full_year as i64;
	let m = month as i64;
	let d = day as i64;

	let y_adj = if m <= 2 { y - 1 } else { y };
	let m_adj = if m <= 2 { m + 9 } else { m - 3 };

	let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
	let yoe = y_adj - era * 400;
	let doy = (153 * m_adj + 2) / 5 + d - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	let days = era * 146097 + doe - 719468;
	(days * 86400) as u64
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_log_date() {
		assert_eq!(parse_log_date("web 26-0214.log"), Some((26, 2, 14)));
		assert_eq!(parse_log_date("web 26-0214 09.log"), Some((26, 2, 14)));
		assert_eq!(parse_log_date("web 26-0214 09.47.log"), Some((26, 2, 14)));
		assert_eq!(parse_log_date("invalid"), None);
	}

	#[test]
	fn test_secs_to_datetime() {
		let (y, m, d, h, min) = secs_to_datetime(1771027200);
		assert_eq!((y, m, d, h, min), (2026, 2, 14, 0, 0));
	}
}
