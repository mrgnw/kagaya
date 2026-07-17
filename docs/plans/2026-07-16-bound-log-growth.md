# Bound per-service log growth (issue #6)

launchd appends stdout/stderr to one file per process forever — no rotation,
no cap. Logs reached 4.3 GB. Bound each stream so no project can fill the disk.

## Mechanism

**Copytruncate**, kagaya-side. launchd has no native rotation, and newsyslog's
rename-based rotation loses data because launchd's held fd (`O_APPEND`) follows
the inode. Because the fd is `O_APPEND`, `set_len(0)` on the live file is safe:
the next write lands at the new EOF (the user proved this by hand-truncating
4.3 GB → 22 MB with services running).

When `len(file) > cap`: copy the last `cap/2` bytes to `<file>.1` (replacing any
previous `.1`), then truncate the live file to 0. Bounded copy (≤ cap/2), bounded
disk (≤ 1.5×cap per stream), recent history kept. Only files under
`~/.local/state/kagaya/logs` are ever touched (safety invariant).

Enforced at two points, both routing through `enforce_log_caps()`:
1. **Periodic** (the guarantee): 10-minute interval task in the `ky serve` daemon.
2. **On start**: top of `plist_sync::start_services` / `restart_services`.

The sweep enumerates `com.kagaya.*.plist` in `~/Library/LaunchAgents` (exactly the
set launchd can grow), reads each plist's stdout/stderr paths, and caps them. First
run migrates existing oversized logs.

## Config surface

- Global default: `[logs] max_mb = 100` in `~/.config/kagaya/config.toml` (`0` = off).
- Per-project override: `log_max_mb = 250` at the root of a project's `services.toml`.

`log_max_mb` is a reserved root key in services.toml — the service parsers skip it
(otherwise it would be parsed as a service named `log_max_mb` and warn / break port
resolution).

## Tasks (TDD)

- [x] 1. `logs.rs`: `bound_log_file` + `parse_log_max_mb` + tests (pure, `tempfile`).
- [x] 2. `config.rs`: reshape dead `LogsConfig` → `{ max_mb }` (default 100);
       skip reserved `log_max_mb` key in `load_service`; tests.
- [x] 3. `logs::enforce_log_caps` + wiring: call in `plist_sync::start_services`
       / `restart_services`; fix `parse_service_ports` to ignore `log_max_mb`
       when collapsing a single-service file; 10-min sweep in `server/mod.rs`;
       `server/api.rs::stream_log` resets read pos on shrink (web tail survives).
- [x] 4. Docs: README "Log rotation" note + CHANGELOG entry.

## Verify

`cargo test -p kagaya`, `cargo build --release`. Manual: `[logs] max_mb = 1`,
run a chatty inline service, watch `<name>.log` truncate at ~1 MB with the tail in
`<name>.log.1` while `ky echo` keeps streaming.

## Notes / decisions

- Old `[logs]` keys (`max_size_bytes`, `max_age_days`, `max_files`) become ignored
  — serde skips unknown keys, so existing configs keep parsing. Noted in CHANGELOG.
- Tail copy (`.1`) over plain truncate-to-zero: ~8 extra lines, but never erases the
  crash you're mid-debugging. Worst-case disk 1.5×cap instead of 1×.
- Default 100 MB (issue's suggestion): ~2 streams × ~dozen services ≈ 3 GB worst
  case, tunable down globally.
