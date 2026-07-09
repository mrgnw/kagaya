# plan-ideas

## Summary

Kagaya (binary: `ky`) is a launchd-backed service manager for macOS: it compiles
services.toml/projects.toml into launchd plists and drives launchctl for
start/stop/restart/status/logs, readiness ordering (`depends_on`, `ready`, `ports`),
and login autostart. The in-process supervisor era (v0.6–v0.13) is gone; launchd does
the supervision. See docs/api.md for the full command tree.

## Ideas / future work

- [ ] Ship the web UI in the crates.io package (ui/build lives outside crates/kagaya, so cargo-installed builds serve the API but show "ui not built"; GitHub binaries are full-fat)
- [ ] Linux/systemd backend if ever needed (currently macOS-only by design)
