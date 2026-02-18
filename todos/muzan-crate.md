# muzan — daemon lifecycle toolkit for Rust CLIs

branch: muzan

## Status: implemented

All core features implemented and integrated into kagaya.

## What

Extract kagaya's daemon plumbing into a reusable crate called `muzan`.
Named after Kibutsuji Muzan (Demon Slayer) — the progenitor demon who creates and commands all other demons.

Any Rust CLI can use `muzan` to add a background daemon mode with IPC, auto-start, and lifecycle management.

## Why

No Rust crate exists for this. The ecosystem has:
- `daemonize` / `fork` — just the fork/detach syscalls, no IPC
- `interprocess` / `tipsy` — just transport, no protocol or daemon pattern
- `service-manager` — delegates to OS (systemd/launchd), not self-managed
- `psup` — dead (2021), attempted this exact thing
- `pmdaemon` — monolithic PM2 clone, not composable

`muzan` fills the gap: composable daemon lifecycle for any CLI.

## What's done

### Core crate (`crates/muzan/`)
- [x] `paths.rs` — XDG-compliant state/config dirs, parameterized by app name
- [x] `server.rs` — tokio Unix socket server, newline-delimited JSON, generic over `Req`/`Resp`
  - `run_socket_server` — simple version, drops parse errors
  - `run_socket_server_with_error` — with `on_parse_error` callback
- [x] `client.rs` — sync `DaemonClient<Req, Resp>` with `connect()` and `send()`
  - `is_running()` and `read_pid()` helpers
- [x] `daemon.rs` — `Daemon` struct with `run()`, `start_background()`, `stop()`, `cleanup()`
  - `ensure_daemon()` — auto-start with default args
  - `ensure_daemon_with_args()` — auto-start with custom args
- [x] `clap.rs` — (feature = "clap") `DaemonCommand` enum: Run, Start, Stop, Status
- [x] Doc comments on all public API

### Integration into kagaya
- [x] `protocol.rs` — delegates path logic to `muzan::DaemonPaths`
- [x] `daemon/mod.rs` — uses `muzan::server::run_socket_server_with_error`
- [x] `main.rs` — uses `muzan::DaemonClient`, `muzan::ensure_daemon_with_args`, `muzan::client::is_running`

### Verified
- [x] `cargo check --workspace` — zero warnings
- [x] `cargo check -p muzan --features clap` — clean
- [x] `cargo test -p kagaya` — 2/2 tests pass

## Future work
- Publish muzan + kagaya to crates.io
- ~~`kagaya` — process supervisor crate~~ done (see `todos/kagaya-crate.md`)
