use crate::error::{AppError, Result};
use crate::state::{Runner, Sidework, Task};
use karui_core::options::CompressOptions;
use karui_core::probe::MediaInfo;
use karui_core::sizing::measure;
use karui_core::tools::Tools;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// Estimated output bytes for `path` with `options`, from three short sample
/// encodes. The window asks for one file at a time and caches the answers.
///
/// Refused with `busy` while a batch runs: the samples are real encodes.
#[tauri::command]
pub async fn estimate_size(
    app: AppHandle,
    path: PathBuf,
    info: MediaInfo,
    options: CompressOptions,
    runner: State<'_, Arc<Runner>>,
    sidework: State<'_, Arc<Sidework>>,
) -> Result<u64> {
    if runner.running() {
        return Err(AppError::Busy);
    }
    let dir = app
        .path()
        .app_cache_dir()
        .map(|dir| dir.join("sizing"))
        .map_err(|e| AppError::Invalid(e.to_string()))?;
    let cancel = sidework.begin(Task::Sizing);
    let sidework = sidework.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _turn = sidework.turn();
        if cancel.load(Ordering::Relaxed) {
            return Err(karui_core::Error::Cancelled.into());
        }
        Ok(measure(
            &Tools::locate()?,
            &path,
            &info,
            &options,
            &dir,
            &cancel,
        )?)
    })
    .await
    .map_err(|e| AppError::Invalid(e.to_string()))?
}
