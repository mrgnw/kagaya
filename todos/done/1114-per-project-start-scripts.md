# Per-project start scripts for multi-process / dependency-aware services

**Resolved** — kagaya now writes one plist per process when services.toml has multiple entries (`com.kagaya.<project>.<proc>.plist`). Multi-process is native. See commit `be5bfc9`. Wrapper scripts remain useful for projects that genuinely need pre_start hooks or dependency ordering (e.g. openchamber waiting on opencode, matrix's tuwunel LOCK cleanup), but are no longer required for routine multi-process services.

## Context

After the launchctl-frontend refactor, kagaya dropped the daemon's built-in `depends_on`, `ready`, multi-process, and `pre_start` features. Most services re-run fine as single launchd agents, but some need orchestration kagaya no longer provides.

Known affected services in projects.toml:

- **matrix** — services.toml has 9 entries (whatsapp, gvoice, instagram, telegram, linkedin, fb, localai, baibot, tuwunel + automation). `tuwunel` has a `pre_start` hook that kills any stale tuwunel and removes `data/tuwunel/LOCK`. The bridges depend on tuwunel's homeserver being up.
- **openchamber** — depends on `opencode` being reachable on :4096 before it starts. Without orchestration, launchd fires openchamber immediately and it fails "connection refused" until KeepAlive retries line up with opencode being ready.

Autostart is currently disabled on both so launchd doesn't thrash them at boot.

## What to do

Inside each affected project, add a single entry-point script (`start.sh`, `justfile` target, `scripts/start`, etc.) that does the orchestration kagaya used to do:

1. Any pre_start cleanup (kill stale process, remove stale lockfiles)
2. Start dependencies or wait on readiness (`curl --retry`, loop with sleep)
3. Exec the main process with the right args

Then change the projects.toml entry to point at the script:

```toml
[matrix]
dir = "~/dev/matrix"
run = "./start.sh"

[openchamber]
dir = "/Users/m/dev/_run/openchamber"
run = "./start.sh"
```

Re-enable autostart with `ky autostart matrix on` / `ky autostart openchamber on` once the script is reliable.

## Notes

- Script should `exec` the final long-running process so launchd sees the right PID.
- For multi-process services (like matrix), options are: (a) one plist per bridge with a shared tuwunel plist that bootstraps first, or (b) one wrapper script that uses `&` + `wait`. (a) is cleaner and gets per-process KeepAlive; (b) is simpler and one status line.
- `ky add <project> <dir>` auto-detects Procfile/package.json; a plain `start.sh` is picked up if detection doesn't match anything else, but explicit `run = "./start.sh"` in projects.toml is better.
