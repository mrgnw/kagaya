# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **`ky autostart <svc> on|off` is durable.** It edited only the launchd plist and left `projects.toml` alone. Since the plist's `RunAtLoad` is compiled from the config's `autostart`, the next `ky start` or `ky restart` silently reverted the change — a plain reboot honoured it, so the setting looked like it had worked. `ky autostart` now writes `projects.toml` first, then the plist. A simple entry (`name = "~/dir"`) has nowhere to hold the key, so it is promoted to table form, appended at the end of the file; every other line keeps its formatting, comments and order.

## [0.15.0-alpha.3] - 2026-07-30

Prerelease, like alpha.1 and alpha.2 — `releases/latest` stays on 0.14.1, so the
install script and `ky self update` are unaffected.

### Fixed

- **`ky restart` honours `depends_on`.** The restart path walked the topo-ordered units and restarted each back to back with no readiness barrier, so dependencies and dependents restarted concurrently. A service whose process depended on a `type = "task"` build could come back up against a half-written build directory and stay broken until restarted a second time.
- **A `type = "task"` no longer reads ready before launchd has spawned it.** Readiness was `!is_running_label(label)`, true both when a task had finished and when it had not started yet; launchd takes ~250ms to spawn after `kickstart`, well under the 500ms readiness poll. Readiness now tracks launchd's `runs` counter from a baseline captured before the unit is kicked, tolerating both counter jumps greater than one and the reset to 0 across `bootout`/`bootstrap`. This affected `ky start` too, where it was masked by `RunAtLoad` usually spawning the task in time.
- **`ky start` no longer reports "started" without starting anything.** With autostart off, `bootstrap` loads a job idle rather than running it, so `ky start` on such a service reported success while starting nothing — and any dependent waiting on it blocked until its `ready_timeout`.

## [0.15.0-alpha.2] - 2026-07-09

### Removed

- `ky cron` and the koku/muzan dependencies — cron management belongs to koku's own CLI; kagaya now has zero path dependencies.
- Unused dependencies: `nix`, `tower-http`, `tracing`, `tracing-subscriber`.

### Changed

- All dependencies bumped to latest (`toml` 1, `listeners` 0.6, and a full `cargo update`).

## [0.15.0-alpha.1] - 2026-07-09

Pre-release of the launchd API overhaul — published as a GitHub prerelease, so
`releases/latest` (install script, `ky self update`) stays on 0.14.1.

### Added

- **`--wait` works**: `ky start db --wait` blocks until every started process is ready (readiness priority: `ready` command exit 0 > all `ports` listening > task exited > running), bounded by `ready_timeout`.
- **`depends_on` works**: processes start in dependency order; a dependency must be ready before its dependents start, and an unready dependency skips them with a clear error.
- **`..` chains work**: `ky start db..api` starts `db`, waits until it is ready, then starts `api`. `ky start --autostart` applies the same sequencing to `depends_on` chains in projects.toml.
- **`--force` works**: `ky start`/`ky restart` with `--force` kill foreign processes holding the service's configured ports (SIGTERM, bounded wait, then SIGKILL).
- **Sync-on-start**: `ky start` and `ky restart` re-sync plists from services.toml/projects.toml first, so config edits take effect without `ky reload-config`.
- **Unsupported-key warnings**: unknown keys in services.toml entries now warn loudly instead of being silently ignored.
- `ky serve restart`; `ky serve` itself is now idempotent and reports `already running`.

### Added

- **Log rotation** (#6): launchd has no log rotation, so per-service logs grew unbounded (they reached 4.3 GB). kagaya now copytruncates any log stream that exceeds its cap — copying the recent tail to `<name>.log.1` and truncating the live file to zero (safe because launchd holds the fd with `O_APPEND`). Enforced on every `ky start`/`ky restart` and every 10 minutes while `ky serve` runs (which also trims existing oversized logs on startup). The cap defaults to **100 MB per stream**; set `[logs] max_mb` in `~/.config/kagaya/config.toml` (`0` disables), or `log_max_mb` at the root of a project's `services.toml`. `ky echo` and the web-UI tail survive rotation. Old `[logs]` keys (`max_size_bytes`, `max_age_days`, `max_files`) are now ignored.

### Fixed

- `ky status --watch 10` parses (previously: `unknown service: 10`).
- `ky restart --all` restarts everything (previously fell back to the cwd project).
- The `serve` row in `ky status` reflects the daemon's real state (was always `off`), and the web server binds `daemon.port` from config.toml instead of a hardcoded 13369.
- launchctl calls run under a hard 15s deadline, and generated plists use `ThrottleInterval = 5` without `kickstart -p` — `ky restart` can no longer hang for 30+ seconds in launchd's spawn throttle.
- **Port-safe restart**: restarting a service that binds ports now fully stops the running instance, waits (bounded, 5s) for it to release its ports, and only then starts a fresh instance — removing the rapid-restart race that could leave two listeners or a service that failed to rebind. If a port is held by an unrelated process, `ky restart` now fails with a clear error (`port N held by pid P (name)`) instead of letting launchd crash-loop. Portless services keep the cheap `kickstart` path, so they don't raise a macOS "Login Items" notification on every restart.

### Removed

- `ky launchd`/`lctl` (generic launchd agent manager), `ky tail` (use `ky echo`), `ky migrate` (`ky add` detects Procfiles), the unused in-process supervisor, and the `ubermind-cli` crate.
- Config keys `max_retries`, `restart_delay`, `pre_start`, and the `[logs]` block — launchd owns restart pacing (`KeepAlive` + throttle); log rotation was supervisor-era.

### Changed

- One vocabulary everywhere: a registered unit is a **service**, its units are **processes** (docs previously mixed project/service/process/agent).
- README, completions, and `docs/api.md` (full command tree) rewritten for the launchd backend.

## [0.14.1] - 2026-05-15

### Fixed

- **`project.process` targets now stay process-scoped**: `ky start jobs.ui`, `ky stop jobs.ui`, and `ky restart jobs.ui` now operate only on the requested process instead of fanning out to every launchd plist in the project. Missing process targets now fail with a clear error before launchd is touched.

## [0.14.0] - 2026-04-15

### Changed

- **`ky rc` is now a no-op when nothing changed**: `reload-config` compares each generated plist structurally (order-independent) against the on-disk version and skips `bootout`/`bootstrap` when identical. Previously every reload restarted all services, flooding macOS "Login Items" notifications. Output now reports `all N service(s) up to date` or `synced N plist(s) (M unchanged)`.

## [0.12.1] - 2026-03-28

### Added

- **Process CPU/RAM metrics**: Running processes now report `cpu_percent` and `memory_bytes` via the `sysinfo` crate. Visible in the web UI's expanded process rows as compact `X% · YM` badges, and available in the JSON API.

### Changed

- **HTTP server on by default**: The daemon now starts the web UI HTTP server automatically. Use `--no-http` to disable. `ky serve` is now just an alias for `ky daemon start`.
- **HTTP bind failure is non-fatal**: If the HTTP port is already in use, the daemon logs a warning and continues operating via the Unix socket instead of exiting.

## [0.12.0] - 2026-03-17

### Added

- **Service dependencies**: New `depends_on` field in `services.toml` for startup ordering
  - `depends_on = "db"` or `depends_on = ["db", "cache"]` — start dependencies first
  - Transitive resolution: if `worker` depends on `api` which depends on `db`, starting `worker` starts all three
  - Circular dependency detection with descriptive error messages
  - Dependencies are auto-started even if not explicitly requested

- **Readiness detection**: Processes can declare how to check if they're ready
  - `ready = "pg_isready -h localhost"` — polls a command every 500ms until exit 0
  - `ports = [8080]` — waits for TCP connection on configured ports
  - `type = "task"` — ready on successful exit (exit code 0)
  - `ready_timeout = 30` — configurable timeout in seconds (default: 10)
  - Dependent services wait for readiness before starting

- **Ad-hoc chains** (`..` syntax): Sequence processes from the CLI without config changes
  - `ky start db..api worker` — start db, wait for ready, then api; worker starts immediately
  - `ky start db..api..worker` — all three in sequence
  - Chains are overlaid on top of config-level `depends_on`

- **`--wait` flag**: Block `ky start` until all started processes are ready
  - `ky start db --wait && echo "db is up"` — composable with shell
  - Useful for CI/CD scripts and automation

## [0.11.0] - 2026-03-03

### Added

- **Smart adaptive watch mode**: Post-command status watching now intelligently adapts to what happened
  - Auto-escalates from condensed to detailed view when any process fails, so you immediately see which process and why
  - Failed/crashed processes show their last 10 log lines inline below the status table
  - State transition annotations: processes that just crashed or just started are highlighted with bold colored annotations
  - Faster initial polling: 250ms intervals for the first 2 seconds (catches immediate failures), then 1s intervals
  - Command-specific exit conditions:
    - `ky stop` exits immediately once all processes confirm stopped
    - `ky start` runs full 4s window to catch late crashes
    - `ky restart` watches for 6s, exits early once all processes are running and stable
  - Non-zero exit code (1) if processes are crashed/failed at end of watch window (scriptable)
  
- **Port readiness detection**: Processes with configured ports now show accurate port status
  - New "starting" state (cyan `◌`) for processes that are running but ports aren't listening yet
  - Transitions to "on" (green `●`) once port scanner confirms the port is open
  - Watch mode exit conditions wait for port readiness, not just process start
  - Fixes false positives where process spawned but service wasn't actually ready
  
- **`--no-watch` / `-W` flag**: Skip the automatic post-command status watch
  - Works on `ky start`, `ky stop`, `ky restart`
  - Useful for scripting or when you don't need visual confirmation
  
### Changed

- **Condensed status now auto-expands on failure**: The detailed view is no longer just a manual `--detailed` flag — it automatically activates when needed
- **Process status API**: Added `ports_expected` field to `ProcessStatus` (backward-compatible via `#[serde(default)]`)

## [0.6.5] - 2026-02-16

### Added

- **Port detection in status**: Running processes now show which TCP ports they're listening on
  - Automatically detects listening ports via system APIs (no configuration needed)
  - Resolves child process ports through process group expansion (handles `sh -c` wrappers)
  - Displayed in CLI status output and web UI
  - Available in HTTP API responses (`ports` field on process info)

## [0.6.4] - 2026-02-16

### Added

- **Auto-watch after modifications**: Commands that modify services now automatically watch status for 4 seconds
  - `ub start myapp` — automatically watches for 4s after starting
  - `ub stop myapp` — automatically watches for 4s after stopping
  - `ub reload myapp` — automatically watches for 4s after reloading
  - `ub restart myapp web` — automatically watches for 4s after restarting a process
  - Override default: `ub start myapp --watch 8` (custom duration)
  - Disable watch: `ub start myapp --watch 0`
- **Continuous echo streaming**: `ub echo` now runs continuously until stopped (Ctrl+C)
  - Previously only printed one snapshot then exited
  - Now properly streams live logs in real-time
- **Simplified `ub add` command**: Register projects more easily
  - `ub add` (from project dir) — auto-detects name from directory
  - `ub add myapp` (from project dir) — uses cwd with custom name
  - `ub add myapp ~/dev/myapp` — full form with explicit path

### Changed

- **Watch duration defaults**: Different defaults for different commands
  - `ub status --watch` — indefinite (until stopped), 1s refresh interval
  - `ub start/stop/reload/restart` — automatic 4s watch
  - All watch durations can be overridden with explicit values

### Fixed

- Echo command now properly loops and streams output continuously
- Watch mode default duration logic fixed for status vs modification commands

## [0.6.2] - 2026-02-15

### Added

- **`tail` command**: Follow log files in real-time (`ub tail matrix.automation`)
- **Dot syntax targeting**: Use `service.process` to target a specific process
  - `ub status matrix.automation` — show only the automation process
  - `ub logs matrix.baibot` — view logs for a specific process
  - `.process` shorthand from within a project directory (e.g., `ub status .api`)
- **`--watch` / `-w` flag**: Live-updating status display
  - `ub status matrix -w` — watch for 4s (default), refresh every 1s
  - `ub status --all -w 10` — watch all services for 10s
  - `ub start matrix -w` — start then watch status
  - `--watch-interval N` to customize refresh rate
  - Uses cursor-up rewrite for flicker-free updates
- **Human-readable uptime**: `6h10m` instead of `22255s`
- **Text status labels**: `on`, `off`, `failed`, `crashed` as a final column
- **Color distinction**: Crashed processes (retrying) shown in yellow vs failed (terminal) in red

## [0.6.0] - TBD

### Changed

**Complete rewrite: Native Rust process supervision**

ubermind v0.6 removes all dependencies on overmind and tmux, replacing them with native Rust process management:

- **Native process supervision**: Direct PID-based process management without external dependencies
- **Auto-restart**: Configurable crash recovery with retry limits
- **Log management**: Automatic log rotation with timestamped files
- **Live streaming**: Ring buffers for real-time log output
- **Unified daemon**: Single daemon handles both process supervision and web UI
- **No external dependencies**: No longer requires overmind or tmux installation

### Breaking Changes

- Configuration format and APIs may differ from v0.5
- Migration from v0.5 projects should be straightforward (same Procfile format)
- Users upgrading should review the new documentation

### For Users of v0.1-v0.5

Earlier versions of ubermind were thin wrappers around [overmind](https://github.com/DarthSim/overmind). Version 0.6 represents a complete architectural shift to native process management while maintaining the same user-facing Procfile format and CLI commands.

## [0.5.1] - 2025-02-14

### Added

- Shell autocomplete for bash, zsh, and fish
  - Completes commands: `start`, `stop`, `status`, `restart`, etc.
  - Completes project names from config
  - Completes flags: `--all`, `-a`, `--daemon`, etc.
  - Example: `ub start appli<tab>` → `ub start appligator`
  - Install script automatically downloads completion files
  - Completions available in `completions/` directory

## [0.4.0] - 2024-12-19

### Changed

**Configuration file names renamed for clarity**

- `~/.config/ubermind/services` → `~/.config/ubermind/projects`
  - Clarifies that each entry is a project directory with its own Procfile
  
- `~/.config/ubermind/Procfile` → `~/.config/ubermind/commands`
  - Distinguishes ubermind's config from actual project Procfiles
  - Uses Procfile format for standalone commands
  
- `~/.config/ubermind/proc/` → `~/.config/ubermind/_commands/`
  - Underscore prefix signals this is an internal/auto-generated directory

### Improved

- Clearer mental model: **projects** (mapped directories) vs **commands** (standalone entries)
- Better documentation explaining the two-layer architecture
- All user-facing output now uses consistent "projects" terminology
- UI displays "No projects configured" message
- Help text and error messages updated throughout

### Technical

- Renamed `load_config_services()` → `load_projects()`
- Renamed `load_procfile_services()` → `load_commands()`
- Added `projects_config_path()` helper function
- Updated UI backend to use projects config path
- No API changes - internal refactoring only

## [0.3.5] - 2024-01-XX

### Added

- Initial stable release
- Multi-project management with Procfile support
- Web UI for monitoring and control
- Built on overmind/tmux (later replaced in v0.6)
