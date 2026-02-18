# Launchd Output Improvements

## Alias

`lctl` — "launch control". Add to shell as `alias lctl='ky launchd'`.
Also add `"lctl"` to the match arm in `main.rs` alongside `"launchd" | "launch"`.

## List View Rework

Current:
```
 ● com.bjango.istatmenus.agent   /Users/m/Library/Application Support/iStat Menus 7/iStat Men pid 768
```

New — matches kagaya column format:
```
 ● istatmenus.agent    on       2h15m   768
 ● opencode.serve      on       2h15m   759   :3000
 ⚠ cloudflared        exit 1
 ◻ espanso            not loaded
 ◻ ccxprocess         exit 0
```

Column order: `{symbol} {short-label} {status-text} {uptime} {pid} {port}`

### Status mapping

| State                    | Symbol | Text         | Color  |
|--------------------------|--------|--------------|--------|
| Running (has PID)        | ●      | on           | green  |
| Loaded + exit 0          | ◻      | exit 0       | dimmed |
| Loaded + exit non-zero   | ⚠      | exit N       | yellow |
| Not loaded               | ◻      | not loaded   | dimmed |

### Short label algorithm

1. Strip TLD prefix (`com`, `org`, `io`, `net`, `homebrew`)
2. 3+ remaining segments → drop first (vendor), keep rest
3. 2 remaining → drop first only if it's a prefix of the second or a known noise word (`user`, `mxcl`)
4. 1 or 0 → keep as-is

Examples:
- `com.bjango.istatmenus.agent` → `istatmenus.agent`
- `com.cloudflare.cloudflared` → `cloudflared`
- `com.opencode.serve` → `opencode.serve`
- `homebrew.mxcl.syncthing` → `syncthing`
- `com.user.gymcalendar` → `gymcalendar`
- `com.adobe.ccxprocess` → `ccxprocess`

### Label coloring

First part dimmed, last segment brighter (matching status color for running, normal for others).

## Uptime

For running agents, call `ps -o etime=,pid= -p <pids>` (one subprocess for all PIDs).
Parse etime format (`MM:SS`, `HH:MM:SS`, `DD-HH:MM:SS`) into seconds.
Display using same compact `format_uptime` as kagaya (e.g. `2h15m`, `3d6h`).

## Port Detection

Copy `listening_ports_for_pids()` from `daemon/supervisor.rs` into `launchd.rs`.
Show `:PORT` column for running agents.

## Better resolve_label

On failure, find labels where any segment matches the input.
Show "did you mean: X?" suggestions — extends the existing kagaya-only suggestion to all agents.

## Detail View (status <label>)

Keep current key-value layout but add uptime + port info.

## Files

- `crates/kagaya/src/bin/kagaya/launchd.rs` — all output changes
- `crates/kagaya/src/bin/kagaya/main.rs` — add `"lctl"` alias
