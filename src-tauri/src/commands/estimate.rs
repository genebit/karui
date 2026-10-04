use crate::error::{AppError, Result};
use crate::state::{RateStore, Runner, Sidework, Task};
use karui_core::estimate::{benchmark, times, Basis};
use karui_core::options::CompressOptions;
use karui_core::probe::MediaInfo;
use karui_core::tools::Tools;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;

/// Mirrored by `Estimates` in `src/lib/bindings.ts`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimates {
    /// Seconds per item, in the order given. `None` where it cannot be known.
    pub secs: Vec<Option<f64>>,
    /// `None` when there is no rate yet for these settings.
    pub basis: Option<Basis>,
}

/// How long each file would take to compress with `options`.
///
/// Instant once this machine's rate for the codec and preset is known. The
/// first time, it benchmarks for a few seconds, unless a batch is running,
/// in which case it answers with no estimates rather than slow the batch.
#[tauri::command]
pub async fn estimate_times(
    items: Vec<MediaInfo>,
    options: CompressOptions,
    runner: State<'_, Arc<Runner>>,
    sidework: State<'_, Arc<Sidework>>,
    rates: State<'_, Arc<RateStore>>,
) -> Result<Estimates> {
    options.validate()?;
    let (codec, preset) = (options.codec, options.preset);
    let rate = match rates.get(codec, preset) {
        Some(rate) => rate,
        None if runner.running() => {
            return Ok(Estimates {
                secs: vec![None; items.len()],
                basis: None,
            })
        }
        None => {
            let cancel = sidework.begin(Task::Benchmark);
            let (sidework, rates, opts) = (
                sidework.inner().clone(),
                rates.inner().clone(),
                options.clone(),
            );
            tauri::async_runtime::spawn_blocking(move || {
                let _turn = sidework.turn();
                // Another request may have measured it while this one waited.
                if let Some(rate) = rates.get(codec, preset) {
                    return Ok(rate);
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err(karui_core::Error::Cancelled.into());
                }
                let measured = benchmark(&Tools::locate()?, &opts, &cancel)?;
                tracing::info!(
                    "Timed {codec} at the {preset} preset: about {:.0} megapixels a second",
                    measured / 1e6
                );
                rates.update(|r| r.record_benchmark(codec, preset, measured));
                rates
                    .get(codec, preset)
                    .ok_or_else(|| AppError::Invalid("rate not saved".into()))
            })
            .await
            .map_err(|e| AppError::Invalid(e.to_string()))??
        }
    };
    Ok(Estimates {
        secs: times(&items, &options, rate),
        basis: Some(rate.basis),
    })
}
