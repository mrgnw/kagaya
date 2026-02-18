<p align="center">
  <img src="logo.svg" width="128" height="128" alt="kagaya logo">
</p>

# kagaya

A native Rust process supervisor for managing multiple projects. Each project keeps its own `Procfile`, and kagaya orchestrates them all from anywhere with auto-restart, log management, and live monitoring.

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

This creates a projects config at `~/.config/kagaya/projects`.

### 2. Create a Procfile in your project

Each project you want to manage needs a `Procfile` in its root directory. A Procfile lists the processes to run — one per line, in `name: command` format:

```sh
# ~/dev/myapp/Procfile
web: npm run dev
api: python server.py
worker: ruby worker.rb
```

This is the standard [Procfile](https://devcenter.heroku.com/articles/procfile) format. Each line becomes a named process that kagaya will manage.

### 3. Register your project with kagaya

```sh
ky add myapp ~/dev/myapp
```

This tells kagaya "there's a project called `myapp` at `~/dev/myapp` that has a Procfile."

**Shorthand:** If you're already in the project directory with a Procfile, just run:

```sh
cd ~/dev/myapp
ky add
# myapp: added (/Users/you/dev/myapp)
```

This automatically uses the directory name as the project name.

### 4. Start it

```sh
ky start myapp    # start one project
ky start          # or start everything
```

## How it fits together

kagaya has two layers of config:

**Projects file** (`~/.config/kagaya/projects`) — maps project names to directories:

```
myapp: ~/dev/myapp
api: ~/dev/api-server
frontend: ~/dev/frontend
```

**Commands file** (`~/.config/kagaya/commands`) — optional, defines standalone commands in Procfile format:

```
tunnel: ssh -N -L 5432:localhost:5432 prod-server
sync: watchman-wait . --max-events 0 -p '*.json' | xargs ./sync.sh
```

Each project directory has its own **Procfile** that defines what processes to run:

```
# ~/dev/myapp/Procfile
web: npm run dev
api: python server.py

# ~/dev/api-server/Procfile
server: cargo run
worker: cargo run --bin worker

# ~/dev/frontend/Procfile
dev: pnpm dev
```

When you run `ky start myapp`, kagaya looks up `myapp` → `~/dev/myapp`, then starts its daemon in that directory using the `Procfile`. Each project gets its own isolated supervisor instance — one project crashing won't affect the others.

Standalone commands from the `commands` file are auto-expanded into generated Procfiles under `~/.config/kagaya/_commands/`.

## Usage

```
ky init                # create projects config file
ky add [name] [dir]    # register a project directory (uses cwd if omitted)

ky status              # show all projects
ky start [name]        # start project(s)
ky stop [name]         # stop project(s)
ky reload [name]       # restart project(s) (picks up Procfile changes)
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

The projects file lives at `~/.config/kagaya/projects` (respects `$XDG_CONFIG_HOME`).

You can edit it directly or use `ky add`:

```
# name: directory
myapp: ~/dev/myapp
api: ~/dev/api-server
frontend: ~/dev/frontend
```

Quick add from a project directory:
```sh
cd ~/dev/myapp && ky add              # infers name from directory
cd ~/dev/myapp && ky add myapp        # uses cwd, custom name
ky add myapp ~/dev/myapp              # full form with explicit path
```

Optionally, define standalone commands in `~/.config/kagaya/commands`:

```
tunnel: ssh -N -L 5432:localhost:5432 prod-server
sync: watchman-wait . --max-events 0 -p '*.json' | xargs ./sync.sh
```

See [tmux cheatsheet](tmux.md) for navigating connected sessions (scrolling, copying error text, etc).

## How it works

kagaya uses native Rust process supervision with:
- Direct PID-based process management
- Auto-restart on crash with configurable retry limits
- Log files with rotation (stored in `~/.local/share/kagaya/log/`)
- Live log streaming via ring buffers
- Unix socket communication for CLI commands
- HTTP/WebSocket API for the web UI

Each project directory gets its own independent supervisor instance. kagaya knows where each project lives and dispatches commands to the right supervisor.

Standalone commands are auto-expanded into generated Procfiles under `~/.config/kagaya/_commands/` (an internal directory that you shouldn't edit directly).

## License

MIT

## History

Formerly known as **ubermind**. Renamed to **kagaya** in v0.9.

kagaya v0.6+ uses native Rust process management. Earlier versions (v0.1-v0.5) were thin wrappers around [overmind](https://github.com/DarthSim/overmind) and tmux.
