# Robust Process Restart with Port Safety

Status: implemented

## Problem

LocalAI's process (PID 32232) crashed but was still alive holding port 29350. Kagaya's retry loop spawned new instances without confirming the port was free, causing all 3 retries to fail with `bind: address already in use`.

## Changes

### Replaced `netstat2` with `listeners` crate
- Removed unmaintained `netstat2` (macOS-only)
- Added `listeners` 0.4 (cross-platform: macOS, Linux, FreeBSD, Windows)
- Port detection now works on Linux too (was no-op stubs before)
- `get_listening_ports()`, `kill_port_holders()`, `ports_in_use()` rewritten
- Added `port_holder()` helper for targeted "who holds this port?" queries

### Runtime port discovery
- `ManagedProcess` now has `runtime_ports: Arc<Mutex<Vec<u16>>>`
- After a process starts, a background task scans its listening ports after 3s
- Discovered ports are stored and used for cleanup during restarts
- If undeclared ports are found, logs a suggestion to add them to services.toml

### Port-free gate before spawn
- Before each spawn (including retries), checks if configured + runtime ports are free
- If ports are busy: identifies holder (PID + name), kills it, waits for release
- If ports are still stuck after two cleanup attempts: counts as a failed retry with a clear message instead of spawning a doomed process
- All stop/restart/kill operations now include runtime-discovered ports in cleanup

### Files modified
- `crates/kagaya/Cargo.toml` — deps
- `crates/kagaya/src/supervisor.rs` — core changes
- `crates/kagaya/src/bin/kagaya/utils.rs` — status display port detection
