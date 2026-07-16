use crate::utils::state_dir;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub fn log_dir() -> PathBuf {
    state_dir().join("logs")
}

pub const MB: u64 = 1024 * 1024;

/// Bound a launchd log file to `max_bytes` via copytruncate.
///
/// launchd holds the log fd open with `O_APPEND`, so `set_len(0)` on the live file
/// is safe — the next write lands at the new EOF. When the file exceeds the cap we
/// copy its last `max_bytes / 2` bytes to `<path>.1` (recent history for debugging)
/// and truncate the live file to zero. Bounded work (≤ max/2 copied) and bounded
/// disk (≤ 1.5×max per stream). Returns `Ok(true)` if it rotated, `Ok(false)` if the
/// file was under the cap, missing, or the cap is disabled (`0`).
pub fn bound_log_file(path: &Path, max_bytes: u64) -> std::io::Result<bool> {
    let len = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if max_bytes == 0 || len <= max_bytes {
        return Ok(false);
    }
    let keep = max_bytes / 2;

    let mut archive = path.as_os_str().to_owned();
    archive.push(".1");
    let archive = PathBuf::from(archive);

    // Copy the tail to <path>.1 (streamed, 8 KiB buffer — no large allocation).
    let mut src = File::open(path)?;
    src.seek(SeekFrom::Start(len - keep))?;
    let mut dst = File::create(&archive)?;
    std::io::copy(&mut src.take(keep), &mut dst)?;

    // ponytail: copytruncate races the writer — bytes written between the copy and
    // the truncate are dropped. Standard logrotate copytruncate has the same window;
    // acceptable for logs.
    OpenOptions::new().write(true).open(path)?.set_len(0)?;
    Ok(true)
}

/// Root-level `log_max_mb = N` override from a project's `services.toml`.
pub fn parse_log_max_mb(services_toml_body: &str) -> Option<u64> {
    toml::from_str::<toml::Value>(services_toml_body)
        .ok()?
        .get("log_max_mb")
        .and_then(|v| v.as_integer())
        .and_then(|n| u64::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn under_cap_is_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_file(tmp.path(), "svc.log", b"small");
        assert_eq!(bound_log_file(&path, 1000).unwrap(), false);
        assert_eq!(fs::read(&path).unwrap(), b"small");
        assert!(!path.with_file_name("svc.log.1").exists());
    }

    #[test]
    fn over_cap_copies_tail_and_truncates() {
        let tmp = tempfile::tempdir().unwrap();
        // 100 bytes, cap 40 → keep last 20 bytes.
        let data: Vec<u8> = (0..100u8).collect();
        let path = write_file(tmp.path(), "svc.log", &data);

        assert_eq!(bound_log_file(&path, 40).unwrap(), true);
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);

        let archive = tmp.path().join("svc.log.1");
        assert_eq!(fs::read(&archive).unwrap(), &data[80..]);
    }

    #[test]
    fn second_rotation_replaces_dot1() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_file(tmp.path(), "svc.log", &vec![b'a'; 100]);
        bound_log_file(&path, 40).unwrap();

        // Fresh content, rotate again — .1 holds the NEW tail, not the old.
        fs::write(&path, vec![b'b'; 100]).unwrap();
        bound_log_file(&path, 40).unwrap();

        let archive = tmp.path().join("svc.log.1");
        assert_eq!(fs::read(&archive).unwrap(), vec![b'b'; 20]);
    }

    #[test]
    fn missing_file_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("does-not-exist.log");
        assert_eq!(bound_log_file(&path, 40).unwrap(), false);
    }

    #[test]
    fn zero_cap_disables() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_file(tmp.path(), "svc.log", &vec![b'x'; 100]);
        assert_eq!(bound_log_file(&path, 0).unwrap(), false);
        assert_eq!(fs::metadata(&path).unwrap().len(), 100);
    }

    /// The invariant the whole design rests on: launchd holds an O_APPEND fd, so a
    /// truncate from another handle is safe — the next append lands at the new EOF.
    #[test]
    fn append_fd_continues_after_truncate() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("svc.log");

        // Simulate launchd's held fd: opened once, append-only, kept across the rotate.
        let mut held = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        held.write_all(&vec![b'o'; 100]).unwrap();

        assert_eq!(bound_log_file(&path, 40).unwrap(), true);

        // The pre-existing append fd keeps writing — lands at the new (zero) EOF.
        held.write_all(b"NEW").unwrap();
        held.flush().unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"NEW");
    }

    #[test]
    fn parse_log_max_mb_present() {
        assert_eq!(
            parse_log_max_mb("log_max_mb = 250\nweb = \"npm run dev\"\n"),
            Some(250)
        );
    }

    #[test]
    fn parse_log_max_mb_absent() {
        assert_eq!(parse_log_max_mb("web = \"npm run dev\"\n"), None);
    }

    #[test]
    fn parse_log_max_mb_non_integer_ignored() {
        assert_eq!(parse_log_max_mb("log_max_mb = \"lots\"\n"), None);
    }

    #[test]
    fn parse_log_max_mb_before_sections() {
        let body = "log_max_mb = 10\n\n[worker]\nrun = \"x\"\n";
        assert_eq!(parse_log_max_mb(body), Some(10));
    }
}
