# kagaya ui

Desktop dashboard for [kagaya](https://github.com/mrgnw/kagaya). View, start, stop, and reload services. Attach to live process output via terminal.

Built with SvelteKit 2 (Svelte 5), Tauri 2, and xterm.js.

## Prerequisites

- [Node.js](https://nodejs.org/) (or Bun)
- [Rust toolchain](https://rustup.rs/)
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- A running kagaya HTTP server (port 13369)

## Dev

```sh
cargo tauri dev
```

This starts both the SvelteKit dev server and the Tauri window.

## Build

```sh
cargo tauri build
```

## Architecture

The UI connects to kagaya's HTTP + WebSocket server on port 13369. When running as a Tauri app, it uses Tauri's IPC invoke layer instead. The service detail page streams live process output over a WebSocket (`/ws/echo/{name}`) rendered with xterm.js.
