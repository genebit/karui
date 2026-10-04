use crate::error::Result;
use crate::state::{CardLedger, RateStore, RunGuard, Runner, Sidework};
use karui_core::batch::{self, Event};
use karui_core::devices::card_of;
use karui_core::options::CompressOptions;
use karui_core::plan::plan_for;
use karui_core::tools::Tools;
use serde::Serialize;
use std::path::{Path, PathBuf};
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
    sidework: State<'_, Arc<Sidework>>,
    rates: State<'_, Arc<RateStore>>,
    ledger: State<'_, Arc<CardLedger>>,
) -> Result<Vec<PlannedJob>> {
    options.validate()?;
    let tools = Tools::locate()?;
    let jobs = plan_for(&paths, &options);
    let cancel = runner.begin()?;
    sidework.cancel();
    let guard = RunGuard(runner.inner().clone());

    let planned = jobs
        .iter()
        .map(|job| PlannedJob {
            input: job.input.display().to_string(),
            output: job.output.display().to_string(),
        })
        .collect();

    let (rates, ledger) = (rates.inner().clone(), ledger.inner().clone());
    std::thread::spawn(move || {
        let _guard = guard;
        batch::run(&tools, &jobs, &options, &cancel, |event| {
            // Each real encode sharpens the estimates for the files after it.
            if let Event::Finished {
                input,
                pixels_per_sec,
                ..
            } = &event
            {
                if let Some(rate) = *pixels_per_sec {
                    rates.update(|r| r.record_encode(options.codec, options.preset, rate));
                }
                // So the card offers only newer clips next time.
                if card_of(Path::new(input)).is_some() {
                    ledger.record(Path::new(input));
                }
            }
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
