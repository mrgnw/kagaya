# kagaya

Process supervisor toolkit for Rust CLIs.

Spawn, monitor, restart, and capture output from child processes. Named after Ubuyashiki Kagaya (Demon Slayer) — the leader who commands and oversees the Demon Slayer Corps.

Pairs with [muzan](https://crates.io/crates/muzan) for daemon lifecycle (socket IPC, PID management, auto-start).

## Features

- **Supervisor** — Manages named services, each with one or more processes. Start, stop, reload, restart, kill.
- **Process lifecycle** — Spawns via `sh -c`, captures stdout/stderr, tracks PID and uptime, handles exit codes.
- **Restart policy** — Configurable per-process: max retries, delay between restarts. Tasks (run-once) never restart.
- **Output capture** — 64KB ring buffer for snapshots + broadcast channel for live streaming + log file with rotation.
- **Log management** — Date-based log files, automatic rotation by size, expiry by age and count.
- **Process groups** — Spawns in process groups for clean tree killing (SIGTERM then SIGKILL).

## Quick start

```rust
use kagaya::{Supervisor, SupervisorConfig, ProcessDef, ServiceType};
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    let sup = Supervisor::new(SupervisorConfig {
        log_dir: "/tmp/myapp/logs".into(),
        max_log_size: 10 * 1024 * 1024,
    });

    let procs = vec![ProcessDef {
        name: "web".into(),
        command: "python -m http.server 8000".into(),
        service_type: ServiceType::Service,
        restart: true,
        max_retries: 3,
        restart_delay_secs: 1,
        env: HashMap::new(),
        autostart: true,
    }];

    sup.start_service("myapp", "/path/to/app".as_ref(), &procs, true, &[])
        .await
        .unwrap();

    // Check status
    let statuses = sup.status().await;
    for s in &statuses {
        println!("{}: {:?}", s.name, s.processes[0].state);
    }

    // Get output
    let output = sup.get_output("myapp", Some("web")).await.unwrap();
    let snapshot = output.snapshot().await;
    println!("{}", String::from_utf8_lossy(&snapshot));

    // Stop
    sup.stop_service("myapp").await.unwrap();
}
```

## Architecture

```
kagaya::Supervisor
├── services: HashMap<String, ManagedService>
│   └── processes: HashMap<String, ManagedProcess>
│       ├── def: ProcessDef (command, restart policy, env)
│       ├── state: ProcessState (Running/Stopped/Crashed/Failed)
│       └── output: OutputCapture (ring buffer + log file + broadcast)
├── logs module (date naming, rotation, expiry)
└── process lifecycle (spawn via sh -c, SIGTERM/SIGKILL tree kill)
```
