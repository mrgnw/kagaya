# plan-ideas

## Summary

Kagaya (binary: `ky`) is a launchd-backed service manager for macOS: it compiles
services.toml/projects.toml into launchd plists and drives launchctl for
start/stop/restart/status/logs, readiness ordering (`depends_on`, `ready`, `ports`),
and login autostart. The in-process supervisor era (v0.6–v0.13) is gone; launchd does
the supervision. See docs/api.md for the full command tree.

## Ideas / future work

- [ ] Publish kagaya to crates.io (lib is now just types + toposort)
- [ ] Publish muzan to crates.io (still a path dependency)
- [ ] Fix the muzan dependency path in workspace Cargo.toml (points outside the repo)
- [ ] Linux/systemd backend if ever needed (currently macOS-only by design)
