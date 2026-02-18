<p align="center">
  <img src="logo.svg" width="128" height="128" alt="kagaya logo">
</p>

# kagaya

A native Rust process supervisor for managing multiple projects. Each project keeps its own `services.toml`, and kagaya orchestrates them all from anywhere with auto-restart, log management, and live monitoring.

Inspired by [overmind](https://github.com/DarthSim/overmind) and [foreman](https://github.com/ddollar/foreman).

## Install

```sh
# shell script (prebuilt binary)
curl -fsSL https://raw.githubusercontent.com/mrgnw/kagaya/main/install.sh | sh

# gah (github asset helper)
gah install mrgnw/kagaya

# cargo
cargo install kagaya       # from source
cargo binstall kagaya      # prebuilt binary
```

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

You can also define standalone commands directly:

```toml
[tunnel]
run = "ssh -N -L 5432:localhost:5432 prod-server"

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
ky init                # create projects config file
ky add [name] [dir]    # register a project directory (uses cwd if omitted)

ky status              # show all projects
ky start [name]        # start project(s)
ky stop [name]         # stop project(s)
ky reload [name]       # restart project(s) (picks up config changes)
ky kill [name]         # kill process(es) in project(s)
ky restart [name]      # restart process(es) in project(s)
ky echo [name]         # live stream logs from project(s)
ky logs [name]         # show last 100 lines of log file
ky tail [name]         # follow log file (tail -f)
ky serve [-p PORT]     # start web UI server (default port: 13369)
```

### Watch mode

Commands that modify services automatically watch status for 4 seconds:

```sh
ky start myapp               # starts and watches for 4 seconds
ky stop myapp                # stops and watches for 4 seconds
ky reload myapp              # reloads and watches for 4 seconds
ky restart myapp web         # restarts process and watches for 4 seconds
```

Override the default watch duration or use with status:

```sh
ky status --watch            # watch indefinitely (refreshes every 1s)
ky status --watch 10         # watch for 10 seconds
ky start myapp --watch 8     # start and watch for 8 seconds (overrides default)
ky reload myapp --watch 0    # reload without watching
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
frontend = "~/dev/frontend"

# standalone commands (no project directory needed)
[tunnel]
run = "ssh -N -L 5432:localhost:5432 prod-server"

[db-backup]
run = "pg_dump mydb > backup.sql"
type = "task"
```

Quick add from a project directory:
```sh
cd ~/dev/myapp && ky add              # infers name from directory
cd ~/dev/myapp && ky add myapp        # uses cwd, custom name
ky add myapp ~/dev/myapp              # full form with explicit path
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
```

### config.toml (optional)

Global settings at `~/.config/kagaya/config.toml`:

```toml
[daemon]
port = 13369

[logs]
max_size_bytes = 10485760    # 10MB, triggers rotation
max_age_days = 7
max_files = 5

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
- Log files with rotation (stored in `~/.local/share/kagaya/log/`)
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
