//! Desktop shell.
//!
//! Deliberately thin: this crate wires plugins, owns the running batch,
//! bridges logging to the UI, and exposes IPC commands. Every decision about
//! which files to take, where outputs go, and how ffmpeg is invoked lives in
//! `karui-core`.

mod commands;
mod error;
mod logging;
mod state;

use state::{LaunchPaths, Runner};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// How long quitting waits for a running encode to stop and delete its
/// working file. ffmpeg dies on the next poll, so this is rarely reached.
const EXIT_GRACE: Duration = Duration::from_secs(3);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(logging::Backlog::default())
        .manage(Arc::new(Runner::default()))
        .manage(LaunchPaths::from_args())
        .setup(|app| {
            // Engine messages reach the UI log panel through this layer, so
            // `tracing::warn!` in the engine needs no knowledge of Tauri.
            let filter = tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into());
            let backlog = app.state::<logging::Backlog>().inner().clone();
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().without_time())
                .with(logging::UiLayer::new(app.handle().clone(), backlog))
                .init();

            tracing::info!("karui {} ready", env!("CARGO_PKG_VERSION"));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::tools::tool_status,
            commands::queue::probe_paths,
            commands::queue::launch_paths,
            commands::compress::start_compression,
            commands::compress::cancel_compression,
            logging::log_backlog,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the application");

    app.run(|handle, event| {
        // Closing the window does not end child processes. Without this an
        // encode would carry on after the app had gone, and its `.part` file
        // would never be cleaned up.
        if let tauri::RunEvent::Exit = event {
            handle.state::<Arc<Runner>>().cancel_and_wait(EXIT_GRACE);
        }
    });
}
