<p align="center">
  <img src="logo.svg" width="128" height="128" alt="kagaya logo">
</p>

# kagaya (unmaintained)

**launchd made easy.** Register a project or command as a service, then reliably
`start` / `stop` / `restart` / `status` / `logs` it. kagaya compiles simple TOML into
launchd plists and drives `launchctl` for you — launchd does the supervision
(auto-restart, boot start), ky makes it usable.

> [!IMPORTANT]
> kagaya is no longer developed. Use [pitchfork](https://pitchfork.jdx.dev)
> instead — it does the same job (local daemons with good DX) and is actively
> maintained. Existing kagaya services keep working; `ky stop` + `ky remove`
> unregisters them.

> [!NOTE]
> This project was built with heavy LLM assistance and should be considered proof-of-concept. Contributions are welcome, but I can't guarantee the accuracy of the code or that I will continue to maintain it. If there are any mistakes in attribution or anything else, please let me know.

macOS only. See [docs/api.md](docs/api.md) for the full command tree.

## Install

```sh
# shell script (prebuilt binary)
curl -fsSL https://ky.xcc.es/install.sh | zsh

# gah (github asset helper)
gah install mrgnw/kagaya

# cargo
cargo install kagaya       # from source
cargo binstall kagaya      # prebuilt binary
```

## Quick start

```sh
cd ~/dev/myapp
ky add          # register (offers to create services.toml if missing)
ky start        # start it
ky              # status
```

Each service is either a **project directory** with a `services.toml`:

```toml
# ~/dev/myapp/services.toml
web = "npm run dev"
api = "python server.py"
```

…or a **standalone command** with no directory:

```sh
ky add tunnel --run 'ssh -N -L 5432:localhost:5432 prod-server'
```

Every key in `services.toml` becomes a named process. kagaya writes one launchd
plist per process (`com.kagaya.myapp.web`), and launchd keeps it alive.

## Usage

```
ky status [name]           # show status (ky, ky <name>, ky all also work)
ky start [name]            # start service(s), or one process: name.process
ky stop [name]             # stop
ky restart [name]          # restart (picks up services.toml edits automatically)
ky logs [name]             # print log file paths
ky echo [name]             # tail + live-stream output
ky show [name]             # show resolved config

ky add [name] [dir]        # register a service (cwd if omitted)
ky remove <name>           # unregister + delete plists
ky autostart <name> on     # start on login (RunAtLoad)
ky serve                   # web UI daemon (port from config.toml, default 13369)
ky self update             # update ky
```

Targeting is flexible: `ky start myapp.web` (one process), `ky myapp start`
(service-first), or just `ky start` from inside a registered directory.

### Startup order & readiness

`depends_on` starts dependencies first and waits until they're **ready** before
starting dependents:

```toml
[db]
run = "docker compose up postgres"
ready = "pg_isready -h localhost"    # polled every 500ms until exit 0
ready_timeout = 30                   # seconds (default 10)

[api]
run = "python server.py"
depends_on = "db"
ports = [8080]                       # ready when the port is listening
```

Readiness is detected in priority order: `ready` command exit 0 → all `ports`
listening → task exited → process running. An unready dependency stops its
dependents from starting.

From the CLI:

```sh
ky start db..api worker    # chain: db → ready → api; worker starts in parallel
ky start db --wait         # block until ready (for scripting: && ...)
ky start api --force       # kill whatever foreign process holds api's ports
```

### Config changes

`services.toml` and `projects.toml` are the source of truth — plists are a
compiled cache. `ky start` and `ky restart` re-sync plists automatically, so
editing config and running `ky restart` just works. `ky reload-config` (alias
`rc`) re-syncs everything at once.

### Watch mode

Commands that change state briefly watch status afterward to confirm the
operation worked — failures auto-expand with the last log lines, and the exit
code is 1 if anything is crashed at the end of the window.

```sh
ky status -w               # watch until q
ky status --watch 10       # watch for 10 seconds
ky start myapp -W          # skip the post-start watch
```

### Log rotation

launchd appends each process's stdout/stderr to one file forever, so a chatty service can fill the disk. kagaya bounds this: when a log passes its cap it copies the recent tail to `<name>.log.1` and truncates the live file to zero. It runs on every `ky start`/`ky restart` and every 10 minutes while the web server (`ky serve`) is running. `ky echo` and the web-UI tail keep streaming across a rotation.

The cap defaults to **100 MB per log stream**. Override globally in `~/.config/kagaya/config.toml`:

```toml
[logs]
max_mb = 250   # per stream; 0 disables rotation entirely
```

Or per project, at the root of its `services.toml`:

```toml
log_max_mb = 500

web = "npm run dev"
```

### Autostart

```sh
ky autostart               # list per-service state
ky autostart myapp on      # start myapp on login
ky autostart off           # disable for every service
```

In `projects.toml`, `autostart = true` plus service-level `depends_on` controls
boot ordering; `ky start --autostart` starts exactly that set.

### Machine output

`--json` or `--tsv` on any command produces machine-readable output and skips
the interactive watch.

## Config reference

**`~/.config/kagaya/projects.toml`** — the registry:

```toml
myapp = "~/dev/myapp"                # simple: name = project dir

[frontend]
dir = "~/dev/frontend"
autostart = true
depends_on = "myapp"                 # autostart ordering

[tunnel]                             # standalone command
run = "ssh -N -L 5432:localhost:5432 prod-server"
autostart = true
```

**`<dir>/services.toml`** — the processes of one service:

```toml
web = "npm run dev"                  # simple form

[worker]                             # full form
run = "python worker.py"
type = "task"                        # run once, don't keep alive
restart = true                       # keep alive unless clean exit (default)
env = { RUST_LOG = "debug" }
ports = [8080]
depends_on = ["db"]
ready = "curl -sf localhost:8080/health"
ready_timeout = 30
```

Unsupported keys warn loudly and are ignored — restart pacing is launchd's job
(`KeepAlive` + a 5s throttle), not config.

**`~/.config/kagaya/config.toml`** (optional):

```toml
[daemon]
port = 13369                         # web UI port

[logs]
max_mb = 100                         # per log stream; 0 disables rotation

[defaults]
restart = true
env = { FORCE_COLOR = "1", CLICOLOR_FORCE = "1" }
```

## Shell completion

Completions for bash/zsh/fish are installed to `~/.local/share/kagaya/completions/`
by the install script.

```sh
# zsh (~/.zshrc)
fpath=(~/.local/share/kagaya/completions $fpath)
autoload -Uz compinit && compinit

# bash (~/.bashrc)
source ~/.local/share/kagaya/completions/ky.bash

# fish
ln -s ~/.local/share/kagaya/completions/ky.fish ~/.config/fish/completions/
```

## How it works

- one launchd plist per process, labelled `com.kagaya.<service>[.<process>]`,
  in `~/Library/LaunchAgents/`
- `start`/`stop`/`restart` drive `launchctl bootstrap`/`bootout`/`kickstart`,
  every call under a hard 15s deadline
- restarts of port-binding services are port-safe: stop, wait for release,
  refuse (or `--force`) foreign holders, start fresh
- oversized logs are copytruncated to `<name>.log.1` (see [Log rotation](#log-rotation))
- logs go to `~/.local/state/kagaya/logs/<service>[.<process>].log` (+ `.err.log`)
- `ky serve` runs a local HTTP/WebSocket API + web UI (see [docs/api.md](docs/api.md))

### Dev mode

```sh
cargo run -p kagaya -- status      # CLI

cd ui && pnpm install && pnpm dev  # web UI with HMR
```

## License

[MIT](LICENSE)

## History

Formerly known as **ubermind**. Renamed to **kagaya** in v0.9. v0.6–v0.13 ran a
native Rust supervisor; kagaya now drives launchd directly — less machinery,
same commands, and services survive kagaya itself. Development stopped in
August 2026 in favor of [pitchfork](https://pitchfork.jdx.dev).
