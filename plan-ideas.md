# plan-ideas

## Summary

Kagaya (binary: `ky`) is a native Rust process supervisor for managing multiple projects with auto-restart, log rotation, live monitoring, and a web UI. It was rewritten from an overmind/tmux wrapper (ubermind) into pure Rust at v0.6, and split into two crates: muzan (daemon lifecycle library) and kagaya (supervisor + CLI). Currently at v0.10.7 with active releases. Three todos remain: launchd output improvements, and publishing muzan + kagaya to crates.io.

## Action Items

- [ ] Implement the launchd_updates.md todo (list view rework, short labels, uptime, port detection, lctl alias)
- [ ] Publish muzan to crates.io (muzan-crate.md lists this as future work)
- [ ] Publish kagaya to crates.io
- [ ] Remove or archive the ubermind-cli crate (still in crates/ubermind-cli/)
- [ ] Update the CHANGELOG.md -- entries still reference `ub` command instead of `ky`
- [ ] Fix the muzan dependency path in workspace Cargo.toml (points to `../muzan` which is outside the repo)

## Detailed Assessment

### Project Status

Kagaya is a mature, working tool. The core supervisor handles process lifecycle (start/stop/restart/kill), auto-restart with configurable retries, log rotation, live log streaming via ring buffers, Unix socket IPC, and HTTP/WebSocket API for the web UI. The CLI supports status, start, stop, reload, restart, echo, logs, tail, serve, and autostart commands. Shell completions exist for bash, zsh, and fish.

Recent releases (v0.10.x) added condensed status view, port cleanup improvements, watch mode TUI with ratatui, JSON/TSV output modes, and migration to clap for argument parsing. The architecture split into muzan (reusable daemon lifecycle) and kagaya (supervisor-specific logic) is complete.

The web UI lives in `ui/` as a separate SvelteKit app with a Tauri wrapper.

### Git Status

- Branch: `main`, tracking `ubermind/main` (remote still named "ubermind")
- Working tree: clean
- Up to date with remote

### Last Activity

- Last commit: 2026-02-22 14:54 ("chore: update lockfile")
- Last file edit: 2026-02-22 14:47

### Existing Plans & TODOs

**kagaya-crate.md** -- Status: implemented. Documents the architecture split and what's done. Future work: publish to crates.io, remove/archive ubermind-cli.

**muzan-crate.md** -- Status: implemented. Describes muzan as a reusable daemon lifecycle crate for any Rust CLI. Core features (paths, server, client, daemon, clap integration) all done with tests. Future work: publish to crates.io.

**launchd_updates.md** -- The main remaining development todo:
- Add `lctl` alias for `ky launchd` command
- Rework list view to match kagaya column format (symbol, short-label, status-text, uptime, pid, port)
- Status mapping with colored symbols (running/exit/not loaded)
- Short label algorithm (strip TLD prefix, drop vendor segments)
- Label coloring (first part dimmed, last segment brighter)
- Uptime display via `ps` for running agents
- Port detection (reuse from supervisor)
- Better resolve_label with "did you mean?" suggestions
- Enhanced detail view with uptime + port info

No TODO/FIXME comments found in source code.

### Remaining Work

**Development**
- Launchd output improvements (launchd_updates.md) -- the most substantial remaining work
- The remote is still named "ubermind" -- should be renamed to "origin" or "kagaya"
- CHANGELOG.md v0.6.4 and v0.6.2 entries still reference `ub` commands instead of `ky`
- The workspace Cargo.toml references `muzan = { path = "../muzan" }` -- this means muzan lives outside the kagaya repo. For crates.io publishing, muzan needs to be a published crate or moved into the workspace.

**Publishing**
- muzan needs to be published to crates.io first (kagaya depends on it)
- kagaya can be published after muzan
- The ubermind-cli crate in crates/ should be archived or removed before publishing

**Cleanup**
- The `crates/ubermind-cli/` directory still exists
- The GitHub repo was renamed from ubermind to kagaya (confirmed done in kagaya-crate.md)

### Ideas & Considerations

- muzan could be a genuinely useful standalone crate. The todo correctly identifies a gap in the Rust ecosystem for composable daemon lifecycle management. Publishing it with good docs and examples could drive adoption and serve as portfolio/marketing material for the freelance work.
- The launchd improvements would make `ky launchd` a useful standalone tool for inspecting macOS launch agents, not just kagaya's own services. This could be a selling point.
- Consider whether the web UI (ui/) should be included in the binary distribution or kept separate. Currently it's a full SvelteKit + Tauri app which is a lot of machinery for a status dashboard.
- Linux support uses systemd but the launchd_updates todo is macOS-only. Consider parity.
- The install.sh + gah + cargo install + cargo binstall distribution story is good. Consider adding a Homebrew tap for macOS users.
