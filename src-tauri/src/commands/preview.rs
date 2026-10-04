use crate::error::{AppError, Result};
use crate::state::{Runner, Sidework, Task};
use karui_core::options::CompressOptions;
use karui_core::preview::{compare, fresh_dir, thumbnail, Comparison, Request, Stage};
use karui_core::probe::MediaInfo;
use karui_core::tools::Tools;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Manager, State};

/// Where comparison stills are written. Cleared by each new comparison and
/// when the app quits.
pub fn preview_root(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_cache_dir()
        .map(|dir| dir.join("preview"))
        .map_err(|e| AppError::Invalid(e.to_string()))
}

/// Compare one frame of `path` before and after compression. `output` is the
/// finished file when there is one; otherwise a short sample is encoded with
/// `options`. `on_stage` gets the original still as soon as it exists, then
/// the sample encode's progress.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn compare_preview(
    app: AppHandle,
    path: PathBuf,
    output: Option<PathBuf>,
    options: CompressOptions,
    at_secs: f64,
    on_stage: Channel<Stage>,
    runner: State<'_, Arc<Runner>>,
    sidework: State<'_, Arc<Sidework>>,
) -> Result<Comparison> {
    // A sample is a real encode, and running it beside a batch would only
    // slow both. Reading frames from a finished output is cheap.
    if output.is_none() && runner.running() {
        return Err(AppError::Busy);
    }
    let root = preview_root(&app)?;
    let cancel = sidework.begin(Task::Preview);
    let sidework = sidework.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _turn = sidework.turn();
        if cancel.load(Ordering::Relaxed) {
            return Err(karui_core::Error::Cancelled.into());
        }
        let tools = Tools::locate()?;
        let dir = fresh_dir(&root)?;
        let request = Request {
            input: path,
            output,
            options,
            at_secs,
        };
        // A closed channel only means the window moved on; the cancel flag
        // stops the work.
        let mut report = |stage| {
            let _ = on_stage.send(stage);
        };
        Ok(compare(&tools, &request, &dir, &cancel, &mut report)?)
    })
    .await
    .map_err(|e| AppError::Invalid(e.to_string()))?
}

/// The bytes of a still written by `compare_preview`, sent as raw binary
/// rather than JSON.
#[tauri::command]
pub async fn preview_image(app: AppHandle, path: PathBuf) -> Result<Response> {
    let root = preview_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        // Only stills this app wrote. Without the check, this command would
        // hand the webview any file on disk.
        let file = path.canonicalize()?;
        if !file.starts_with(root.canonicalize()?) {
            return Err(AppError::Invalid("not a preview image".into()));
        }
        Ok(Response::new(std::fs::read(file)?))
    })
    .await
    .map_err(|e| AppError::Invalid(e.to_string()))?
}

/// A small JPEG of `path` for its row in the list, sent as raw binary.
/// `info` is the probe the window already has, so the file is not probed
/// again.
#[tauri::command]
pub async fn video_thumbnail(path: PathBuf, info: MediaInfo) -> Result<Response> {
    tauri::async_runtime::spawn_blocking(move || {
        let tools = Tools::locate()?;
        Ok(Response::new(thumbnail(&tools, &path, &info)?))
    })
    .await
    .map_err(|e| AppError::Invalid(e.to_string()))?
}
