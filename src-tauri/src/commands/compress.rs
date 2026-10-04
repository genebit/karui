use crate::error::Result;
use crate::state::{RunGuard, Runner};
use karui_core::batch;
use karui_core::options::CompressOptions;
use karui_core::plan::plan;
use karui_core::tools::Tools;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Where `batch::Event`s go. Mirrored in `src/lib/ipc.ts`.
pub const COMPRESS_EVENT: &str = "compress://event";

/// Mirrored by `PlannedJob` in `src/lib/bindings.ts`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedJob {
    pub input: String,
    pub output: String,
}

/// Plan the batch, start it on its own thread, and return the plan at once.
/// Progress arrives as `compress://event`.
#[tauri::command]
pub fn start_compression(
    app: AppHandle,
    paths: Vec<PathBuf>,
    options: CompressOptions,
    runner: State<'_, Arc<Runner>>,
) -> Result<Vec<PlannedJob>> {
    options.validate()?;
    let tools = Tools::locate()?;
    let jobs = plan(&paths, options.output_dir.as_deref(), options.overwrite);
    let cancel = runner.begin()?;
    let guard = RunGuard(runner.inner().clone());

    let planned = jobs
        .iter()
        .map(|job| PlannedJob {
            input: job.input.display().to_string(),
            output: job.output.display().to_string(),
        })
        .collect();

    std::thread::spawn(move || {
        let _guard = guard;
        batch::run(&tools, &jobs, &options, &cancel, |event| {
            let _ = app.emit(COMPRESS_EVENT, event);
        });
    });

    Ok(planned)
}

/// Stop the running batch. The current file's working copy is deleted and
/// every file not yet started is reported cancelled.
#[tauri::command]
pub fn cancel_compression(runner: State<'_, Arc<Runner>>) -> bool {
    runner.cancel()
}
