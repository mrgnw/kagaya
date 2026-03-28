# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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