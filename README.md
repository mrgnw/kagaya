<p align="center">
  <img src="logo.svg" width="128" height="128" alt="kagaya logo">
</p>

# kagaya (pre-alpha, under construction)

A native Rust process supervisor for managing multiple projects. Each project keeps its own `services.toml`, and kagaya orchestrates them all from anywhere with auto-restart, log management, and live monitoring.

Inspired by [overmind](https://github.com/DarthSim/overmind) and [foreman](https://github.com/ddollar/foreman).

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

### Dev mode

For CLI development, run the binary from the workspace:

```sh
cargo run -p kagaya -- status
```

For UI development with HMR, work from the `ui/` package:

```sh
cd ui
pnpm install
pnpm dev
```

`ky serve` manages the installed launchd UI service. It does not switch behavior through a Cargo `dev` feature.

### Shell completion

Tab completion for commands, project names, and flags:

```sh
ky start appli<tab>    # completes to: ky start appligator
ky st<tab>             # completes to: ky status / ky start / ky stop
ky status <tab>        # shows all project names
```

**Setup:**

If installed via the install script, completions are in `~/.local/share/kagaya/completions/`.

**Bash:**
```sh
echo 'source ~/.local/share/kagaya/completions/ky.bash' >> ~/.bashrc
```

**Zsh:**
```sh
# Add to ~/.zshrc
fpath=(~/.local/share/kagaya/completions $fpath)
autoload -Uz compinit && compinit
```

**Fish:**
```sh
ln -s ~/.local/share/kagaya/completions/ky.fish ~/.config/fish/completions/
```

## Quick start

### 1. Initialize kagaya

```sh
ky init
```

This creates `~/.config/kagaya/projects.toml`.

### 2. Create a services.toml in your project

Each project you want to manage needs a `services.toml` in its root directory. It defines the processes to run:

```toml
# ~/dev/myapp/services.toml
web = "npm run dev"
api = "python server.py"
worker = "ruby worker.rb"
```

Each key becomes a named process that kagaya will manage. For more control:

```toml
[web]
run = "npm run dev"

[migrate]
run = "python manage.py migrate"
type = "task"          # runs once, no auto-restart
depends_on = "db"      # start after db is ready
```

### 3. Register your project

```sh
ky add myapp ~/dev/myapp
```

This tells kagaya "there's a project called `myapp` at `~/dev/myapp` that has a services.toml."

**Shorthand:** If you're already in the project directory:

```sh
cd ~/dev/myapp
ky add
# myapp: added (/Users/you/dev/myapp)
```

**Standalone commands:** Register a single command without a project directory:

```sh
ky add opencode --run 'opencode serve --hostname 0.0.0.0'
# opencode: added (run: opencode serve --hostname 0.0.0.0)
```

### 4. Start it

```sh
ky start myapp    # start one project
ky start          # or start everything
```

## How it fits together

kagaya uses two config files:

**Projects** (`~/.config/kagaya/projects.toml`) — maps project names to directories:

```toml
myapp = "~/dev/myapp"
api = "~/dev/api-server"
frontend = "~/dev/frontend"
```

For more options (like autostart on login), use the table form:

```toml
[myapp]
dir = "~/dev/myapp"
autostart = true           # start on login (ky autostart on)
```

You can also define standalone commands directly:

```toml
[tunnel]
run = "ssh -N -L 5432:localhost:5432 prod-server"
autostart = true

[sync]
run = "watchman-wait . --max-events 0 -p '*.json' | xargs ./sync.sh"
type = "task"
```

Each project directory has its own **services.toml** that defines what processes to run:

```toml
# ~/dev/myapp/services.toml
web = "npm run dev"
api = "python server.py"

# ~/dev/api-server/services.toml
server = "cargo run"
worker = "cargo run --bin worker"

# ~/dev/frontend/services.toml
dev = "pnpm dev"
```

When you run `ky start myapp`, kagaya looks up `myapp` → `~/dev/myapp`, reads `services.toml`, and starts those processes. Each project gets its own isolated supervisor — one project crashing won't affect the others.

## Usage

```
ky init                        # create projects config file
ky add [name] [dir]            # register a project directory (uses cwd if omitted)
ky add <name> --run <command>  # register a standalone command

ky status              # show all projects
ky start [name]        # start project(s), or one process with project.process
ky stop [name]         # stop project(s), or one process with project.process
ky reload [name]       # restart project(s) (picks up config changes)
ky kill [name]         # kill process(es) in project(s)
ky restart [name]      # restart project(s), or one process with project.process
ky echo [name]         # live stream logs from project(s)
ky logs [name]         # show last 100 lines of log file
ky tail [name]         # follow log file (tail -f)
ky serve [-p PORT]     # start web UI server (default port: 13369)

ky autostart on        # install boot agent (start services on login)
ky autostart off       # remove boot agent
ky autostart status    # show autostart status
```

### Dependencies & sequencing

Use `depends_on` in `services.toml` to start services in order:

```toml
[db]
run = "docker compose up postgres"
ready = "pg_isready -h localhost"

[api]
run = "python server.py"
depends_on = "db"
```

Starting `api` automatically starts `db` first and waits for it to be ready. Dependencies are transitive — if `worker` depends on `api` which depends on `db`, starting `worker` starts all three in order.

**Readiness** is detected automatically:
- `ready = "command"` — polls until exit 0 (every 500ms)
- `ports = [8080]` — waits for TCP connection
- `type = "task"` — ready on successful exit
- No check configured — ready immediately

**Ad-hoc chains** from the CLI with `..`:

```sh
ky start db..api worker    # db→api sequential, worker starts immediately
ky start db..api..worker   # all three in sequence
```

**`--wait`** blocks until processes are ready (useful for scripting):

```sh
ky start db --wait && echo "db is up"
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

Start services automatically when you log in. Uses LaunchAgent on macOS and systemd on Linux.

**1. Mark projects for boot** in `~/.config/kagaya/projects.toml`:

```toml
[myapp]
dir = "~/dev/myapp"
autostart = true

[tunnel]
run = "ssh -N -L 5432:localhost:5432 prod-server"
autostart = true
```

**2. Install the boot agent:**

```sh
ky autostart on
```

On login, kagaya runs `ky start --autostart` which starts only projects with `autostart = true`.

```sh
ky autostart status    # check if agent is installed + which projects autostart
ky autostart off       # remove the boot agent
```

### Watch mode

Commands that modify services automatically watch status afterward to confirm the operation worked:

```sh
ky start myapp               # starts, watches for 4s
ky stop myapp                # stops, exits once confirmed stopped
ky restart myapp             # restarts, watches for 6s
ky restart myapp web         # restarts process, watches for 6s
```

The watch window adapts based on what happened:
- **Condensed → detailed** — if any process fails, the view auto-expands to show which process and why
- **Log tailing** — failed processes show their last 10 log lines inline
- **Port readiness** — processes with configured ports show "starting" until the port is actually listening
- **Exit code** — exits with code 1 if processes are crashed/failed at the end of the watch window

Override the default watch behavior:

```sh
ky status --watch            # watch indefinitely (refreshes every 1s)
ky status --watch 10         # watch for 10 seconds
ky start myapp --watch 8     # start and watch for 8 seconds
ky start myapp -W            # start without watching (--no-watch)
```

### Live logs

```sh
ky echo myapp          # live stream logs from myapp (runs until stopped)
ky echo myapp web      # live stream from specific process
ky logs myapp          # show last 100 lines from log file
ky tail myapp          # follow log file (like tail -f)
```

### Targeting

Pass project names to target specific projects:

```
ky status myapp        # show status of myapp
ky myapp status        # same thing, flexible arg ordering
```

Use `project.process` to target one process inside a multi-process project:

```
ky start jobs.ui       # start/restart only the ui process in jobs
ky stop jobs.sync      # stop only the sync process in jobs
ky restart jobs.ui     # restart only the ui process in jobs
ky status jobs.ui      # show only the ui process in jobs
```

Omit the name to target all projects (or current project if in a registered directory):

```
ky start               # start all projects
ky stop                # stop all projects
cd ~/dev/myapp && ky status  # show status of myapp (context-aware)
```

## Config

### projects.toml

Lives at `~/.config/kagaya/projects.toml` (respects `$XDG_CONFIG_HOME`).

```toml
# directory-based projects (each has its own services.toml)
myapp = "~/dev/myapp"
api = "~/dev/api-server"

# table form — supports autostart
[frontend]
dir = "~/dev/frontend"
autostart = true

# standalone commands (no project directory needed)
[tunnel]
run = "ssh -N -L 5432:localhost:5432 prod-server"
autostart = true

[db-backup]
run = "pg_dump mydb > backup.sql"
type = "task"
```

Quick add from a project directory:
```sh
cd ~/dev/myapp && ky add              # infers name from directory
cd ~/dev/myapp && ky add myapp        # uses cwd, custom name
ky add myapp ~/dev/myapp              # full form with explicit path
ky add tunnel --run 'ssh -N -L 5432:localhost:5432 prod-server'  # standalone command
```

### services.toml

Each project directory contains a `services.toml`:

```toml
# simple form — just the command
web = "npm run dev"
api = "python server.py"

# full form — with options
[worker]
run = "ruby worker.rb"
restart = true
max_retries = 5
restart_delay = 2
env = { RAILS_ENV = "development" }

# tasks — run once, no auto-restart
[migrate]
run = "python manage.py migrate"
type = "task"

# dependencies — start services in order
[db]
run = "docker compose up postgres"
ready = "pg_isready -h localhost"    # polled until exit 0
ready_timeout = 30                   # seconds (default: 10)

[api]
run = "python server.py"
depends_on = "db"                    # waits for db to be ready first
ports = [8080]                       # ready when port accepts connections

[worker]
run = "python worker.py"
depends_on = ["db", "api"]           # multiple dependencies
```

### config.toml (optional)

Global settings at `~/.config/kagaya/config.toml`:

```toml
[daemon]
port = 13369
public_base_url = "https://ky.xcc.es"
release_dir = "/srv/ky/releases"

[logs]
max_mb = 100                 # per log stream; 0 disables rotation

[defaults]
restart = true
max_retries = 3
restart_delay = 1
env = { FORCE_COLOR = "1", CLICOLOR_FORCE = "1" }
```

## How it works

kagaya uses native Rust process supervision with:
- Direct PID-based process management
- Auto-restart on crash with configurable retry limits
- Log files with rotation (stored in `~/.local/state/kagaya/logs/`)
- Live log streaming via ring buffers
- Unix socket communication for CLI commands
- HTTP/WebSocket API for the web UI

Each project gets its own isolated supervisor. kagaya knows where each project lives and dispatches commands to the right supervisor.

Standalone commands from `projects.toml` are auto-expanded into synthetic services under `~/.config/kagaya/_commands/`.

## License

MIT

## History

Formerly known as **ubermind**. Renamed to **kagaya** in v0.9.

kagaya v0.6+ uses native Rust process management. Earlier versions (v0.1-v0.5) were thin wrappers around [overmind](https://github.com/DarthSim/overmind) and tmux.
