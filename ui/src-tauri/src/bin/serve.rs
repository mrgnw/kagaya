use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let port: u16 = args
        .iter()
        .position(|a| a == "--port" || a == "-p")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())
        .unwrap_or(app_lib::server::DEFAULT_PORT);

    let static_dir = args
        .iter()
        .position(|a| a == "--dir" || a == "-d")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .or_else(|| {
            let build = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|p| p.join("build"))?;
            if build.exists() {
                Some(build)
            } else {
                None
            }
        });

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("kagaya-serve - headless web UI server");
        eprintln!();
        eprintln!("usage: kagaya-serve [options]");
        eprintln!(
            "  -p, --port PORT  HTTP port (default: {})",
            app_lib::server::DEFAULT_PORT
        );
        eprintln!("  -d, --dir DIR    Static files directory (default: auto-detect)");
        eprintln!("  -h, --help       Show this help");
        return;
    }

    eprintln!("kagaya-serve starting on port {port}");
    if let Some(ref dir) = static_dir {
        eprintln!("serving static files from {}", dir.display());
    } else {
        eprintln!("no static files directory found (API-only mode)");
    }

    app_lib::run_server(port, static_dir);
}
