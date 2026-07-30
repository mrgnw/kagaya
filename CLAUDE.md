# Agent Instructions

Follow the Claude defaults in [/Users/m/dev/_agent/CLAUDE.md](/Users/m/dev/_agent/CLAUDE.md).

Use [Tiger Style](/Users/m/dev/_agent/TIGER_STYLE.md) for code design and review: prioritize safety, performance, then developer experience; keep control flow explicit; bound work; assert invariants; handle errors deliberately; and avoid abstractions that do not clearly simplify the system.

## Status

- **Who uses it**: `ky` — launchd-made-easy for macOS. Published public tool (crates.io `kagaya`, GitHub `mrgnw/kagaya`, install via ky.xcc.es). Single maintainer (me), but external users depend on it.
- **Worktrees**: solo maintainer — working on `main` is normal. Use a worktree for deliberate parallel or risky work.
- **Breaking changes**: the public contract is the `ky` CLI surface + the `services.toml` / `projects.toml` / `config.toml` schema. Breaking changes need a semver bump (`Cargo.toml`, currently `0.15.0-alpha.2`) and a `CHANGELOG.md` entry. Release via `just ship`.
- **paseo**: `paseo.json` has cargo `check`/`test`/`lint` + `ui-check` scripts. No dev service — the SvelteKit UI (`ui/`, vite `strictPort` :13369, proxies backend :13370) is kagaya-managed and its port isn't `$PASEO_PORT`-driven; a per-worktree instance would need a vite-config refactor.
- **Current focus / TODOs**: `plan-ideas.md` + `todos/`. TODO(user): confirm focus.

