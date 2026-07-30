# `ky restart` ignores `depends_on`, and a task reads ready before it starts

Found 2026-07-30 while debugging `blog-cms`, whose `web` process depended on a
`type = "task"` build. Every `ky restart blog-cms` left the service returning
500 until it was restarted a second time.

## The two defects

Both in `crates/kagaya/src/bin/kagaya/plist_sync.rs`.

### 1. `restart_one_service` has no readiness barrier

`start_one_service` (`:921-960`) walks the topo-ordered units and, for each
`depends_on` entry, calls `wait_unit_ready` before starting the dependent — an
unready dependency means the dependent is not started at all.

`restart_one_service` (`:1043-1057`) walks the same topo-ordered units (both
call `prepare_units`, which topo-sorts at `:880-884`) and calls
`restart_one_label` on each with no wait between them. Dependencies and
dependents restart concurrently.

### 2. `unit_ready` calls a task ready before launchd has spawned it

```rust
if def.service_type == ServiceType::Task {
    // ponytail: task = done when no longer running; exit code not checked
    return !is_running_label(label);
}
```

`:1106-1108`. "Not running" is true in two very different states: *finished*,
and *not started yet*. Immediately after `restart_one_label`/`start_one_label`
issues its kickstart, launchd has usually not spawned the task, so the unit
reads ready instantly.

This means **fixing (1) alone does not fix the bug** — the new barrier would
pass immediately on a just-kickstarted task. Both must land together.

## Why it breaks the dependent

`blog-cms` ran `[build]` (`vite build`, `type = "task"`) and `[web]`
(`node build/index.js`, `depends_on = "build"`). Node lazily imports route
chunks at request time, so when `web` booted against the old `build/` and the
still-running build renamed every chunk, the first request died:

```
ERR_MODULE_NOT_FOUND … build/server/chunks/nodes/0.js-Chd6twkM.js
```

while disk held `nodes/2.js-msgaqBFG.js`. The process had cached the stale
manifest, so it never recovered. Reproduced 3/3.

That repo has since sidestepped it (`vite build && node build/index.js` in one
process, `blog-cms@15815e8`), so it is no longer a reproduction case — but any
service with `depends_on` still has the bug.

## Proposed fix

### Step 0 — verify the signal before building on it

`launchctl print gui/<uid>/<label>` reports, for a task that has run:

```
state = not running
runs = 2
last exit code = 0
```

**Confirm empirically before relying on it:**

- Is `runs` present on a job that is bootstrapped but has never run? (Expected:
  absent, or `0`. If absent, "no `runs` key" must be treated as zero, not as an
  error.)
- Does `runs` increment on every spawn, including launchd-initiated restarts?
- Does it survive `bootout` + `bootstrap`? (Almost certainly resets — the fix
  must tolerate the counter going *backwards*, since `restart_one_label`
  bootstraps afresh for port-holding units.)

`~/dev/uli/services.toml` has a `type = "task"` process to test against; a
throwaway `ky add <name> --run 'sleep 3'` also works.

If `runs` turns out to be unusable, the fallback is to observe the transition
directly: poll until the task is seen *running*, then until it is *not* — with
the caveat that a task shorter than the 500ms `READY_POLL_INTERVAL` can be
missed entirely, so that path still needs a timeout-based escape. Prefer `runs`
if it holds up.

### Step 1 — make task readiness mean "finished this invocation"

Capture the task's `runs` value immediately *before* issuing the kickstart, and
treat the unit as ready only when `runs > baseline && !is_running_label(label)`.
Handle the counter resetting to 0 after a bootstrap (a reset that lands below
the baseline should be read as a fresh invocation, not as "never ready").

This changes `unit_ready`'s signature or requires threading a baseline through
`wait_unit_ready` — pick whichever keeps the call sites honest. `unit_ready` is
also reached from `wait_service_ready` (`:1114-1129`, used by `ky start a..b`
chains) and from the `--wait` loop (`:971-991`), so any signature change has
three call sites to settle.

### Step 2 — give `restart_one_service` the same barrier as `start_one_service`

Mirror `:927-960`: for each unit, wait on each `depends_on` dep that has not
already been marked ready this pass; on failure, skip the dependent and report
`"{unit}: skipped — {dep} {err}"`, matching the start path's wording.

The two barrier blocks will be near-identical. Extract one helper both call
rather than copying it — but only that block; do not merge start and restart.

## Acceptance criteria

- [ ] `ky restart <svc>` on a service whose process `depends_on` a
      `type = "task"` does not start the dependent until the task has exited.
- [ ] The same holds for `ky start` (defect 2 affects it too — it was only
      masked because the task was usually already built).
- [ ] A task that is bootstrapped but has never run does not read as ready.
- [ ] An unready/failed dependency skips its dependent on restart and says so,
      as it already does on start.
- [ ] `cargo test` green; `cargo clippy` clean.
- [ ] A unit test covers the new readiness logic. The existing `mod tests`
      (`:1451`) only tests pure functions (`parse_etime`, `parse_service_ports`,
      `filtered_project_plists`), so extract the `launchctl print` field parsing
      into a pure function taking `&str` and test that — same shape as
      `parse_etime`. Do not add a test that shells out to `launchctl`.

## Constraints

- Tiger Style, per `AGENTS.md`: bounded waits (`wait_unit_ready` is already
  deadline-bounded — keep it that way, no unbounded polling), explicit control
  flow, assert invariants.
- Do not change `services.toml` semantics or the documented `depends_on`
  behaviour. This makes the documented behaviour true, it does not redefine it.
- `restart_one_label`'s port-safe bootout/bootstrap path (`:1214-1261`) is
  load-bearing for services with `ports` — do not restructure it.
- Keep `ky restart` fast for the common case: a service with no `depends_on`
  must not gain any new waiting.
