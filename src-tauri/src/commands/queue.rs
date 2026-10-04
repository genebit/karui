use crate::error::{AppError, Result};
use crate::state::LaunchPaths;
use karui_core::discover::discover;
use karui_core::probe::{probe_many, MediaInfo};
use karui_core::tools::Tools;
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

/// Mirrored by `QueueItem` in `src/lib/bindings.ts`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub path: String,
    /// `None` when ffprobe could not read the file; `error` then says why.
    pub info: Option<MediaInfo>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probed {
    pub items: Vec<QueueItem>,
    pub skipped: Vec<Skipped>,
}

/// Expand files and folders into videos and read each one.
#[tauri::command]
pub async fn probe_paths(paths: Vec<PathBuf>) -> Result<Probed> {
    tauri::async_runtime::spawn_blocking(move || {
        let tools = Tools::locate()?;
        let found = discover(&paths);
        let results = probe_many(&tools, &found.files);

        let items = found
            .files
            .iter()
            .zip(results)
            .map(|(path, result)| {
                let (info, error) = match result {
                    Ok(info) => (Some(info), None),
                    // The row already names the file; the path in the
                    // engine's message would only repeat it.
                    Err(karui_core::Error::Probe { message, .. }) => (None, Some(message)),
                    Err(other) => (None, Some(other.to_string())),
                };
                QueueItem {
                    path: path.display().to_string(),
                    info,
                    error,
                }
            })
            .collect();
        let skipped = found
            .skipped
            .into_iter()
            .map(|(path, reason)| Skipped {
                path: path.display().to_string(),
                reason,
            })
            .collect();

        Ok(Probed { items, skipped })
    })
    .await
    .map_err(|e| AppError::Invalid(e.to_string()))?
}

/// Paths given on the command line. Empty after the first call.
#[tauri::command]
pub fn launch_paths(launch: State<'_, LaunchPaths>) -> Vec<String> {
    launch.take()
}
