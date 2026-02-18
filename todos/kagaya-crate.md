# kagaya — process supervisor toolkit + CLI

branch: muzan

## Status: implemented

kagaya replaces ubermind as the user-facing binary (`ky`).

## Architecture

- **muzan** — daemon lifecycle library (socket IPC, PID, auto-start)
- **kagaya** — process supervisor library + CLI binary
  - Library: types, logs, output capture, supervisor core
  - Binary (`ky`): CLI, config loading, HTTP/WS API, launchd integration

## What's done

### Library (`crates/kagaya/src/`)
- [x] `types.rs` — ProcessDef, ProcessState, ServiceType, ServiceStatus, ProcessStatus, Service
- [x] `logs.rs` — Date-based log naming, rotation, expiry
- [x] `output.rs` — OutputCapture (ring buffer + broadcast + log file rotation)
- [x] `supervisor.rs` — Supervisor with start/stop/reload/restart/kill, process lifecycle

### CLI binary (`crates/kagaya/src/bin/kagaya/`)
- [x] `main.rs` — CLI commands (status, start, stop, reload, restart, logs, tail, echo, etc.)
- [x] `config.rs` — projects.toml + services.toml + config.toml parsing
- [x] `protocol.rs` — Request/Response types for daemon IPC
- [x] `daemon/` — daemon entry point, request dispatch, HTTP/WS API
- [x] `launchd.rs` — macOS launchd agent management
- [x] `self_update.rs` — self-update from GitHub releases
- [x] All `ubermind` references replaced with `kagaya`/`ky`

### Tests
- [x] 19 kagaya tests (2 unit + 16 integration + 1 doc-test)
- [x] 15 muzan tests (14 integration + 1 doc-test)

## Future work
- Publish muzan + kagaya to crates.io
- Remove/archive ubermind-cli crate
- [x] Rename GitHub repo
