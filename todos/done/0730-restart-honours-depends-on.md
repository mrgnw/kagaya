---
branch: launchd-api-overhaul
---

# `ky restart` ignores `depends_on`, and a task reads ready before it starts

> **Done 2026-07-30.** Both defects fixed in `a4e05f1`. Step 0 findings and the
> before/after measurements are recorded at the bottom of this file.

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

---

## Outcome

### Step 0 findings (measured, macOS 25.5, uid 501)

`analysis/step0_runs_signal.py`. `runs` holds up, with two tolerances:

| Question | Answer |
| --- | --- |
| `runs` on a bootstrapped job that never ran | present as `runs = 0`, **not** absent; `last exit code = (never exited)` |
| Does `runs` increment per spawn? | Yes, but **not by 1** — observed `0 -> 1 -> 3` |
| Does `runs` survive `bootout` + `bootstrap`? | **No, resets to 0** (observed `3 -> 0`) |
| Delay from `kickstart` to a visible pid | **~0.26s**, well under the 500ms `READY_POLL_INTERVAL` |

The 0.26s spawn delay is the direct cause of defect 2: a just-kicked task reads
"not running" purely because launchd has not spawned it yet.

Only the direction of travel of `runs` is therefore usable, never the delta, and
a count *below* the baseline must be read as a fresh epoch rather than as
"never ready".

### Before / after (`analysis/repro_restart_depends_on.py`)

Isolated repro: `[build]` is a `type = "task"` that sleeps 4s then stamps a
file; `[web]` has `depends_on = "build"` and stamps a file the moment it starts.
The number is `web_start - build_done`, so negative means the dependent started
while the task was still running.

```
                                ky start     ky restart
autostart=on     before            +0.69          -3.89
autostart=on     after             +1.68          +0.75
autostart=off    before   BUG web-no-build        -2.96
autostart=off    after     ok both skipped        +1.30
```

- `ky restart` is the headline fix: negative (dependent started ~3-4s early)
  before, positive after, in both autostart modes.
- `autostart=off` / `ky start` shows defect 2 on its own: the task never ran at
  all (`runs = 0`), the old code called it ready and started `web` regardless;
  the new code withholds `web` and reports
  `web: skipped — build not ready after 10s`.
- `autostart=on` / `ky start` held before the fix too — that is the masking the
  plan predicted: `RunAtLoad` spawns the task fast enough that the old
  `!is_running_label` check usually caught it mid-run.

### Third defect found while testing — also fixed (`146d100`)

`start_one_label` (`plist_sync.rs`) bootstraps and reports `"started"` without
kickstarting:

```rust
bootstrap(path)?;
Ok("started")
```

With `RunAtLoad = false` the job loads idle and never runs, so `ky start` on an
autostart-off service reports success while starting nothing, and any dependent
waiting on that unit blocks until its `ready_timeout`. This is why the
`autostart=off` rows above show the task never running.

Fixed by kickstarting when the plist's `RunAtLoad` is false. Deliberately *not*
done the way `restart_one_label` does it
(`bootstrap(path)?; if !is_running_label(label) { kickstart_label(label)?; }`):
the Step 0 measurement shows launchd takes ~250ms to spawn, so a RunAtLoad job
still reads "not running" at that point and `kickstart -k` would kill the
instance it had just spawned — running a `type = "task"` twice.

Re-measured after the fix, with both binaries carrying the `depends_on` fix so
the run isolates this change:

```
                                ky start     ky restart
autostart=on     before            +0.22          +1.28
autostart=on     after             +0.75          +0.77
autostart=off    before   ok both skipped         +0.43
autostart=off    after             +5.44          +0.71
```

`autostart=off` / `ky start` goes from "task never ran, dependent correctly
withheld" to the dependent starting 5.44s after the task finishes — the barrier
now has a task that actually runs to wait on.
