# ky — command tree

One tool, one model: a **service** is a name registered in `~/.config/kagaya/projects.toml`
(a project directory with a `services.toml`, or a standalone command). A service runs one or
more **processes**, each compiled to a launchd plist (`com.kagaya.<service>[.<process>]`).
Plists are a build artifact — the TOML is always the source of truth, and `ky start`/`restart`
re-sync them automatically.

```
ky                                     status of current service (in its dir) or all
│
├── status | st  [name…] [-a|--all] [-d]      show status
│                                             `ky <name>` and `ky all` are shortcuts
│
├── start    [name…] [-a|--all]               start service(s) / name.process
│            --wait                           block until ready (ready > ports > exit > running)
│            -f|--force                       kill foreign processes holding configured ports
│            --autostart                      only services marked autostart = true
│            a..b [c…]                        chain: start a, wait until ready, then b
│            -e|--echo  -d|--detailed  -W|--no-watch
│
├── stop     [name…] [-a|--all] [-e] [-d] [-W]
│
├── restart  [name…] [-a|--all] [-f|--force] [-e] [-d] [-W]
│                                             also `restart name.process`
│
├── logs     [name[.process]]                 print log file paths
├── echo     [name[.process]] [-n LINES]      tail + stream live output (Ctrl-C to stop)
│
├── show     [name[.process]]                 show resolved config; bare `ky show` lists services
│
├── add      [name] [dir]                     register a service (cwd if omitted; offers to
│            <name> --run <cmd>               create services.toml) / standalone command
├── remove | rm  [name]                       unregister + delete plists (cwd name if omitted)
├── init                                      create ~/.config/kagaya/projects.toml
│
├── autostart                                 list per-service login-start state
│   ├── <name> on|off                         toggle one service
│   └── on|off                                toggle every service
│
├── reload-config | rc                        re-sync every plist from config
│
├── serve                                     start web UI daemon (idempotent)
│   ├── stop / restart / status
│   └── foreground                            run in foreground (used by the launchd plist)
│
└── self update                               update ky to the latest release

global flags:  -w|--watch [SECS]   live status (bare -w = until q)
               -W|--no-watch       skip the post-action watch
               --json | --tsv      machine-readable output
               -h/--help  -V/--version
```

## Targeting

| form | meaning |
|---|---|
| `ky start` | current directory's service, or error if unregistered |
| `ky start myapp` | whole service |
| `ky start myapp.web` | one process |
| `ky myapp start` | service-first, same as above |
| `ky start --all` / `ky stop all` | every service |
| `ky start db..api worker` | `db` → ready → `api`; `worker` in parallel |

## services.toml (per service directory)

```toml
web = "npm run dev"                  # simple: name = command

[api]                                # full form
run = "python server.py"
type = "task"                        # run once, no KeepAlive (default: service)
restart = true                       # KeepAlive unless clean exit (default: true)
env = { RUST_LOG = "debug" }
ports = [8080]                       # readiness + port-guarded restart + --force
depends_on = "db"                    # or ["db", "cache"]; starts deps first, waits for ready
ready = "curl -sf localhost:8080/health"   # polled every 500ms until exit 0
ready_timeout = 30                   # seconds (default 10)
```

Anything else is warned about and ignored — restart pacing lives in launchd
(`KeepAlive` + 5s throttle), not in config.

## projects.toml (~/.config/kagaya/)

```toml
myapp = "~/dev/myapp"                # dir with services.toml

[frontend]
dir = "~/dev/frontend"
autostart = true                     # RunAtLoad: start on login
depends_on = "myapp"                 # autostart ordering

[tunnel]                             # standalone command, no directory
run = "ssh -N -L 5432:localhost:5432 prod"
autostart = true
```

## config.toml (~/.config/kagaya/, optional)

```toml
[daemon]
port = 13369                         # web UI port (ky serve)

[defaults]                           # applied to every process
restart = true
env = { FORCE_COLOR = "1" }
```

## HTTP API (ky serve)

```
GET  /api/version                    GET  /api/services
GET  /api/services/{name}            POST /api/services/{name}/start
POST /api/services/{name}/stop       POST /api/services/{name}/reload
POST /api/services/{name}/processes/{process}/restart
POST /api/services/{name}/processes/{process}/kill
GET  /ws/echo/{name}                 GET  /api/host-info
GET  /api/autostart                  POST /api/autostart/{on,off}
```
