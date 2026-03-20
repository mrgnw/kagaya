mod commands;
mod remote_control;
pub mod server;
mod services;
mod tmux;

use std::path::PathBuf;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let static_dir: Option<PathBuf> = app
                .path()
                .resource_dir()
                .ok()
                .map(|d: PathBuf| d.join("_up_/build"))
                .filter(|d: &PathBuf| d.exists())
                .or_else(|| {
                    let dev_build = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .map(|p| p.join("build"))?;
                    if dev_build.exists() {
                        Some(dev_build)
                    } else {
                        None
                    }
                });

            let port = server::DEFAULT_PORT;
            tauri::async_runtime::spawn(async move {
                server::start(port, static_dir).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_services,
            commands::get_service_detail,
            commands::start_service,
            commands::stop_service,
            commands::reload_service,
            commands::restart_process,
            commands::kill_process,
            commands::echo_service,
            commands::get_panes,
            commands::capture_pane,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub fn run_server(port: u16, static_dir: Option<PathBuf>) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        server::start(port, static_dir).await;
    });
}
