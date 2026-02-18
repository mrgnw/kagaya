use std::fs;
use std::path::PathBuf;
use std::process::Command;

const REPO: &str = "mrgnw/kagaya";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn cmd_self_update() {
	let latest = match fetch_latest_version() {
		Ok(v) => v,
		Err(e) => {
			eprintln!("error: failed to check for updates: {}", e);
			std::process::exit(1);
		}
	};

	let latest_clean = latest.strip_prefix('v').unwrap_or(&latest);

	if latest_clean == CURRENT_VERSION {
		eprintln!("already up to date ({})", CURRENT_VERSION);
		return;
	}

	eprintln!("updating kagaya {} -> {}", CURRENT_VERSION, latest_clean);

	let target = detect_target();
	let tag = if latest.starts_with('v') { latest.clone() } else { format!("v{}", latest) };
	let archive_name = format!("kagaya-{}-{}.tar.gz", tag, target);
	let url = format!(
		"https://github.com/{}/releases/download/{}/{}",
		REPO, tag, archive_name
	);

	let install_dir = match std::env::current_exe() {
		Ok(exe) => exe.parent().unwrap_or(&PathBuf::from("/usr/local/bin")).to_path_buf(),
		Err(_) => PathBuf::from("/usr/local/bin"),
	};

	let tmpdir = std::env::temp_dir().join(format!("kagaya-update-{}", std::process::id()));
	let _ = fs::create_dir_all(&tmpdir);

	let archive_path = tmpdir.join(&archive_name);
	if let Err(e) = download(&url, &archive_path) {
		let _ = fs::remove_dir_all(&tmpdir);
		eprintln!("error: failed to download {}: {}", url, e);
		std::process::exit(1);
	}

	let status = Command::new("tar")
		.args(["-xzf", &archive_path.to_string_lossy(), "-C", &tmpdir.to_string_lossy()])
		.status();

	if status.is_err() || !status.unwrap().success() {
		let _ = fs::remove_dir_all(&tmpdir);
		eprintln!("error: failed to extract archive");
		std::process::exit(1);
	}

	for bin_name in &["kagaya"] {
		let src = tmpdir.join(bin_name);
		let dest = install_dir.join(bin_name);
		if src.exists() {
			if let Err(e) = replace_binary(&src, &dest) {
				eprintln!("error: failed to install {}: {}", bin_name, e);
				let _ = fs::remove_dir_all(&tmpdir);
				std::process::exit(1);
			}
		}
	}

	let _ = fs::remove_dir_all(&tmpdir);

	eprintln!("updated to {}", latest_clean);
}

fn fetch_latest_version() -> Result<String, String> {
	let output = Command::new("curl")
		.args([
			"-fsSL",
			&format!("https://api.github.com/repos/{}/releases/latest", REPO),
		])
		.output()
		.map_err(|e| format!("curl failed: {}", e))?;

	if !output.status.success() {
		return Err("failed to fetch release info".to_string());
	}

	let body = String::from_utf8_lossy(&output.stdout);

	for line in body.lines() {
		let line = line.trim();
		if line.contains("\"tag_name\"") {
			if let Some(start) = line.find(": \"") {
				let rest = &line[start + 3..];
				if let Some(end) = rest.find('"') {
					return Ok(rest[..end].to_string());
				}
			}
		}
	}

	Err("could not find tag_name in release response".to_string())
}

fn detect_target() -> String {
	let os = std::env::consts::OS;
	let arch = std::env::consts::ARCH;

	let os_part = match os {
		"macos" => "apple-darwin",
		"linux" => "unknown-linux-musl",
		_ => {
			eprintln!("unsupported OS: {}", os);
			std::process::exit(1);
		}
	};

	let arch_part = match arch {
		"x86_64" => "x86_64",
		"aarch64" => "aarch64",
		_ => {
			eprintln!("unsupported architecture: {}", arch);
			std::process::exit(1);
		}
	};

	format!("{}-{}", arch_part, os_part)
}

fn download(url: &str, dest: &PathBuf) -> Result<(), String> {
	let status = Command::new("curl")
		.args(["-fsSL", "-o", &dest.to_string_lossy(), url])
		.status()
		.map_err(|e| format!("curl failed: {}", e))?;

	if status.success() {
		Ok(())
	} else {
		Err(format!("download failed (HTTP error)"))
	}
}

fn replace_binary(src: &PathBuf, dest: &PathBuf) -> Result<(), String> {
	let backup = dest.with_extension("old");
	let _ = fs::remove_file(&backup);

	if dest.exists() {
		fs::rename(dest, &backup).map_err(|e| format!("backup failed: {}", e))?;
	}

	match fs::copy(src, dest) {
		Ok(_) => {
			#[cfg(unix)]
			{
				use std::os::unix::fs::PermissionsExt;
				let _ = fs::set_permissions(dest, fs::Permissions::from_mode(0o755));
			}
			let _ = fs::remove_file(&backup);
			Ok(())
		}
		Err(e) => {
			if backup.exists() {
				let _ = fs::rename(&backup, dest);
			}
			Err(format!("copy failed: {}", e))
		}
	}
}
