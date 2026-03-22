use crate::config;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "mrgnw/kagaya";
const DEFAULT_BASE_URL: &str = "https://ky.xcc.es";

pub fn cmd_self_update() {
    let target = detect_target();
    let archive_name = format!("ky-{}.tar.gz", target);
    let base_url = install_base_url();
    let hosted_url = format!("{}/releases/latest/{}", base_url, archive_name);
    let github_url = format!(
        "https://github.com/{}/releases/latest/download/{}",
        REPO, archive_name
    );

    eprintln!("updating ky ({})", target);

    let install_dir = match std::env::current_exe() {
        Ok(exe) => exe
            .parent()
            .unwrap_or(Path::new("/usr/local/bin"))
            .to_path_buf(),
        Err(_) => PathBuf::from("/usr/local/bin"),
    };

    let tmpdir = std::env::temp_dir().join(format!("ky-update-{}", std::process::id()));
    let _ = fs::create_dir_all(&tmpdir);

    let archive_path = tmpdir.join(&archive_name);
    if let Err(host_error) = download(&hosted_url, &archive_path) {
        eprintln!("hosted binary unavailable, falling back to GitHub release");
        if let Err(github_error) = download(&github_url, &archive_path) {
            let _ = fs::remove_dir_all(&tmpdir);
            eprintln!(
                "error: failed to download {} ({}) and {} ({})",
                hosted_url, host_error, github_url, github_error
            );
            std::process::exit(1);
        }
    }

    let status = Command::new("tar")
        .args([
            "-xzf",
            &archive_path.to_string_lossy(),
            "-C",
            &tmpdir.to_string_lossy(),
        ])
        .status();

    if status.is_err() || !status.unwrap().success() {
        let _ = fs::remove_dir_all(&tmpdir);
        eprintln!("error: failed to extract archive");
        std::process::exit(1);
    }

    let src = tmpdir.join("ky");
    let dest = install_dir.join("ky");
    if !src.exists() {
        let _ = fs::remove_dir_all(&tmpdir);
        eprintln!("error: archive did not contain ky");
        std::process::exit(1);
    }

    if let Err(e) = replace_binary(&src, &dest) {
        eprintln!("error: failed to install ky: {}", e);
        let _ = fs::remove_dir_all(&tmpdir);
        std::process::exit(1);
    }

    let _ = fs::remove_dir_all(&tmpdir);

    eprintln!("updated ky");
}

fn install_base_url() -> String {
    if let Ok(value) = std::env::var("INSTALL_BASE_URL") {
        return trim_trailing_slash(value);
    }

    let global_config = config::load_global_config();
    if let Some(value) = global_config.daemon.public_base_url {
        return trim_trailing_slash(value);
    }

    DEFAULT_BASE_URL.to_string()
}

fn trim_trailing_slash(value: String) -> String {
    value.trim_end_matches('/').to_string()
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
    let status = if command_exists("curl") {
        Command::new("curl")
            .args(["-fsSL", "-o", &dest.to_string_lossy(), url])
            .status()
            .map_err(|e| format!("curl failed: {}", e))?
    } else if command_exists("wget") {
        Command::new("wget")
            .args(["-qO", &dest.to_string_lossy(), url])
            .status()
            .map_err(|e| format!("wget failed: {}", e))?
    } else {
        return Err("curl or wget required".to_string());
    };

    if status.success() {
        Ok(())
    } else {
        Err("download failed (HTTP error)".to_string())
    }
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {} >/dev/null 2>&1", name)])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
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
