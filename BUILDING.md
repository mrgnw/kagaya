# Building & Releasing

## Which command do I run?

**Developing locally?** `just build`
**Ready to release?** `just release`

```mermaid
flowchart TD
    subgraph "Development"
        build["just build<br/><i>cargo build --workspace</i>"]
        install["just install<br/><i>cargo install both crates</i>"]
    end

    subgraph "Release pipeline"
        build-ui["just build-ui<br/><i>pnpm install + build</i>"]
        dist["just dist<br/><i>cross-compile 4 targets → dist/ + latest assets</i>"]
        release["just release<br/><i>gh release create</i>"]
    end

    build-ui --> dist --> release
```

## Commands

| Command | When to use | What it does |
|---|---|---|
| `just build` | Day-to-day development | `cargo build --workspace` (debug) |
| `just install` | Test the installed binary | `cargo install` both cli + daemon |
| `just release` | Ship a new version | Builds UI, cross-compiles all targets, creates GitHub release |

### Intermediate commands

You probably don't need to run these directly — `just release` chains them automatically.

| Command | What it does |
|---|---|
| `just build-ui` | `pnpm install && pnpm build` in `ui/` |
| `just build-release` | `cargo build --workspace --release` (native target only) |
| `just build-all` | `build-ui` + `build-release` (native target only) |
| `just dist` | `build-ui` + cross-compile all 4 targets, package into `dist/` and `dist/latest/` |

### Release flow

```mermaid
sequenceDiagram
    participant Dev as Developer
    participant J as just release
    participant UI as pnpm (ui/)
    participant Cargo as cargo
    participant Zig as cargo zigbuild
    participant GH as gh (GitHub)

    Dev->>J: just release
    J->>UI: pnpm install && pnpm build
    UI-->>J: ui/build/ ready

    loop each target
        alt macOS (aarch64, x86_64)
            J->>Cargo: cargo build --release --target
        else Linux (aarch64, x86_64)
            J->>Zig: cargo zigbuild --release --target
        end
    end

    J->>J: tar archives + latest aliases → dist/
    J->>Dev: confirm? [y/N]
    Dev->>J: y
    J->>GH: gh release create vX.Y.Z dist/*.tar.gz
    GH-->>Dev: released
    Note over Dev: don't forget: cargo publish
```

### Cross-compilation targets

| Target | Build tool |
|---|---|
| `aarch64-apple-darwin` | cargo |
| `x86_64-apple-darwin` | cargo |
| `aarch64-unknown-linux-musl` | cargo-zigbuild |
| `x86_64-unknown-linux-musl` | cargo-zigbuild |

## Latest install assets

`just dist` now also prepares stable latest-only assets for hosted installs:

- `dist/latest/ky-aarch64-apple-darwin.tar.gz`
- `dist/latest/ky-x86_64-apple-darwin.tar.gz`
- `dist/latest/ky-aarch64-unknown-linux-musl.tar.gz`
- `dist/latest/ky-x86_64-unknown-linux-musl.tar.gz`
- `dist/latest/completions/ky.{bash,zsh,fish}`
- `dist/latest/install.sh`

This matches the installer flow: host latest assets first, then fall back to GitHub `releases/latest/download/...`.
