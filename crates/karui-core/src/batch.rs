//! A whole run: probe and encode each job in turn, reporting as it goes.
//!
//! Jobs run one at a time. libx264 and libx265 already use every core, so two
//! encodes side by side only split the machine and finish no sooner.
//!
//! Events go to a callback rather than to Tauri or a terminal directly, so the
//! desktop shell and the CLI share this loop and the engine knows of neither.

use crate::encode::{encode, file_name};
use crate::options::CompressOptions;
use crate::plan::Job;
use crate::probe::probe;
use crate::tools::Tools;
use crate::{estimate, units, Error};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Mirrored by `CompressEvent` in `src/lib/bindings.ts`. Paths are strings
/// because the frontend keys its queue on the exact text it sent.
#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    Started {
        index: usize,
        total: usize,
        input: String,
        output: String,
    },
    Progress {
        index: usize,
        input: String,
        /// `None` when the source's duration is unknown.
        fraction: Option<f64>,
        speed: Option<f64>,
        eta_secs: Option<f64>,
        out_time_secs: f64,
    },
    Finished {
        index: usize,
        input: String,
        output: String,
        input_bytes: u64,
        output_bytes: u64,
        elapsed_secs: f64,
        /// The encode rate this file achieved, for future estimates. `None`
        /// when it was too short to say.
        pixels_per_sec: Option<f64>,
    },
    Failed {
        index: usize,
        input: String,
        message: String,
    },
    Cancelled {
        index: usize,
        input: String,
    },
    Done {
        summary: Summary,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
    /// Totals over the jobs that succeeded.
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub elapsed_secs: f64,
}

pub fn run(
    tools: &Tools,
    jobs: &[Job],
    opts: &CompressOptions,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(Event),
) -> Summary {
    let started = Instant::now();
    let mut summary = Summary::default();
    let total = jobs.len();

    for (index, job) in jobs.iter().enumerate() {
        let input = job.input.display().to_string();
        let name = file_name(&job.input);

        if cancel.load(Ordering::Relaxed) {
            summary.cancelled += 1;
            on_event(Event::Cancelled { index, input });
            continue;
        }

        on_event(Event::Started {
            index,
            total,
            input: input.clone(),
            output: job.output.display().to_string(),
        });

        let result = probe(tools, &job.input).and_then(|info| {
            let duration = info.duration_secs;
            let outcome = encode(tools, job, &info, opts, cancel, &mut |snapshot| {
                on_event(Event::Progress {
                    index,
                    input: input.clone(),
                    fraction: snapshot.fraction(duration),
                    speed: snapshot.speed,
                    eta_secs: snapshot.eta_secs(duration),
                    out_time_secs: snapshot.out_time_secs,
                });
            })?;
            let rate = estimate::learnt(&info, opts, outcome.elapsed.as_secs_f64());
            Ok((outcome, rate))
        });

        match result {
            Ok((outcome, pixels_per_sec)) => {
                summary.succeeded += 1;
                summary.input_bytes += outcome.input_bytes;
                summary.output_bytes += outcome.output_bytes;
                // Before the log lines, so a terminal finishes its progress
                // line before anything else is printed under it.
                on_event(Event::Finished {
                    index,
                    input,
                    output: job.output.display().to_string(),
                    input_bytes: outcome.input_bytes,
                    output_bytes: outcome.output_bytes,
                    elapsed_secs: outcome.elapsed.as_secs_f64(),
                    pixels_per_sec,
                });
                tracing::info!(
                    "{name}: {} → {} ({}) in {}",
                    units::bytes(outcome.input_bytes),
                    units::bytes(outcome.output_bytes),
                    units::change(outcome.input_bytes, outcome.output_bytes),
                    units::duration(outcome.elapsed.as_secs_f64()),
                );
                if outcome.output_bytes >= outcome.input_bytes {
                    // Kept rather than deleted: the user may still want the
                    // MP4 or the H.265. But it is not what they came for.
                    tracing::warn!(
                        "{name} came out no smaller; the source is already compressed \
                         harder than CRF {}. Raise the CRF or lower the resolution.",
                        opts.crf()
                    );
                }
            }
            Err(Error::Cancelled) => {
                summary.cancelled += 1;
                on_event(Event::Cancelled { index, input });
                tracing::info!("{name}: cancelled");
            }
            Err(error) => {
                summary.failed += 1;
                on_event(Event::Failed {
                    index,
                    input,
                    message: error.to_string(),
                });
                tracing::error!("{error}");
            }
        }
    }

    summary.elapsed_secs = started.elapsed().as_secs_f64();
    on_event(Event::Done {
        summary: summary.clone(),
    });
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialise_as_tagged_camel_case() {
        let event = Event::Progress {
            index: 0,
            input: "/a.mp4".into(),
            fraction: Some(0.5),
            speed: None,
            eta_secs: Some(3.0),
            out_time_secs: 1.0,
        };
        let json = serde_json::to_value(&event).expect("serialise");
        assert_eq!(json["type"], "progress");
        assert_eq!(json["etaSecs"], 3.0);
        assert_eq!(json["outTimeSecs"], 1.0);
    }

    #[test]
    fn cancelled_before_start_touches_nothing() {
        let tools = Tools {
            ffmpeg: "/nonexistent/ffmpeg".into(),
            ffprobe: "/nonexistent/ffprobe".into(),
        };
        let jobs = vec![Job {
            input: "/nonexistent/a.mp4".into(),
            output: "/nonexistent/a-compressed.mp4".into(),
        }];
        let cancel = AtomicBool::new(true);
        let mut seen = Vec::new();
        let summary = run(&tools, &jobs, &CompressOptions::default(), &cancel, |e| {
            seen.push(e)
        });
        assert_eq!(summary.cancelled, 1);
        assert!(matches!(seen[0], Event::Cancelled { .. }));
        assert!(matches!(seen[1], Event::Done { .. }));
    }
}
