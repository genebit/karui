//! How long an encode will take on this machine.
//!
//! x264 and x265 spend roughly the same time per pixel at a given preset,
//! whatever the resolution, so a file's encode time is its output pixels
//! divided by a rate measured once per codec and preset. The first rate comes
//! from a few seconds of encoding a synthetic clip; every real encode then
//! replaces it with what this machine actually managed on real footage.

use crate::args::{fps_cap, output_size, video_args};
use crate::encode;
use crate::options::CompressOptions;
use crate::probe::MediaInfo;
use crate::tools::Tools;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Assumed for the rare source whose frame rate ffprobe cannot read.
const FALLBACK_FPS: f64 = 30.0;

/// The benchmark clip. 720p is large enough for x265 to spread across
/// cores, and noise stands in for the texture of real footage: a clean test
/// pattern encodes half again as fast as anything a camera records.
const BENCH_SOURCE: &str = "testsrc2=size=1280x720:rate=30,noise=alls=12:allf=t";
const BENCH_PIXELS: f64 = 1280.0 * 720.0;
const BENCH_FPS: f64 = 30.0;
/// Enough that even `ultrafast` cannot finish before it has been timed.
const BENCH_FRAMES: &str = "1800";
/// Ignored after the first frame comes out. x265 delivers its opening frames
/// in a burst while its frame threads fill, at several times the speed it
/// then holds.
const BENCH_WARMUP: Duration = Duration::from_secs(1);
/// Measuring stops once this much of steady encoding has been seen…
const BENCH_WINDOW: Duration = Duration::from_secs(2);
/// …or at this, for presets too slow to produce frames in the window.
const BENCH_LIMIT: Duration = Duration::from_secs(20);

/// Encodes shorter than this are mostly start-up, and would teach a rate
/// far below what a long encode reaches.
const MIN_LEARN_SECS: f64 = 5.0;

/// What one file asks of the encoder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Work {
    pub frames: f64,
    pub width: u32,
    pub height: u32,
}

impl Work {
    pub fn pixels(&self) -> f64 {
        self.frames * f64::from(self.width) * f64::from(self.height)
    }
}

/// `None` when the duration is unknown, as for a live capture.
pub fn work(info: &MediaInfo, opts: &CompressOptions) -> Option<Work> {
    let duration = info.duration_secs.filter(|d| *d > 0.0)?;
    let fps = match fps_cap(info, opts) {
        Some(cap) => f64::from(cap),
        None => info.fps.filter(|f| *f > 0.0).unwrap_or(FALLBACK_FPS),
    };
    let (width, height) = output_size(info, opts);
    Some(Work {
        frames: duration * fps,
        width,
        height,
    })
}

/// Where a rate came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    Benchmark,
    /// Learnt from finished encodes, which also pay for decoding and
    /// scaling the real source.
    Measured,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub pixels_per_sec: f64,
    pub basis: Basis,
}

impl Rate {
    pub fn seconds(&self, work: &Work) -> f64 {
        work.pixels() / self.pixels_per_sec
    }
}

/// Encode rates by codec and preset, for this machine only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rates {
    /// Logical CPUs when the rates were taken. A different count means a
    /// different machine, such as a home folder migrated to a new laptop,
    /// and the old rates would mislead.
    cpus: usize,
    rates: BTreeMap<String, Rate>,
}

/// Empty, and stamped with this machine's CPU count. Not derived: a zero
/// count would never match on the next load, and every rate would be
/// thrown away.
impl Default for Rates {
    fn default() -> Rates {
        Rates {
            cpus: cpus(),
            rates: BTreeMap::new(),
        }
    }
}

impl Rates {
    /// Read rates saved by [`Rates::save`]. A missing, unreadable, or foreign
    /// file starts afresh; the cost is one benchmark.
    pub fn load(path: &Path) -> Rates {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Rates>(&bytes).ok())
            .filter(|rates| rates.cpus == cpus())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        // Written aside and renamed, so a crash mid-write leaves the old file.
        let partial = path.with_extension("json.part");
        std::fs::write(&partial, json)?;
        if cfg!(windows) && path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(partial, path)
    }

    pub fn get(&self, opts: &CompressOptions) -> Option<Rate> {
        self.rates.get(&key(opts)).copied()
    }

    /// Never replaces a measured rate: real encodes know better.
    pub fn record_benchmark(&mut self, opts: &CompressOptions, pixels_per_sec: f64) {
        let entry = self.rates.entry(key(opts));
        let rate = Rate {
            pixels_per_sec,
            basis: Basis::Benchmark,
        };
        entry
            .and_modify(|r| {
                if r.basis == Basis::Benchmark {
                    *r = rate;
                }
            })
            .or_insert(rate);
    }

    /// Replaces a benchmark outright, and averages with earlier encodes so
    /// one unusual file moves the estimate only halfway.
    pub fn record_encode(&mut self, opts: &CompressOptions, pixels_per_sec: f64) {
        let entry = self.rates.entry(key(opts)).or_insert(Rate {
            pixels_per_sec,
            basis: Basis::Measured,
        });
        entry.pixels_per_sec = match entry.basis {
            Basis::Benchmark => pixels_per_sec,
            Basis::Measured => (entry.pixels_per_sec + pixels_per_sec) / 2.0,
        };
        entry.basis = Basis::Measured;
    }
}

/// `h265/medium` for software, as before hardware existed, so saved rates
/// still apply. A hardware encoder gets its own: `h265/videotoolbox`, with
/// the preset only for encoders that have presets.
fn key(opts: &CompressOptions) -> String {
    match opts.hardware() {
        None => format!("{}/{}", opts.codec, opts.preset),
        Some(hw) if hw.backend.has_presets() => {
            format!("{}/{}/{:?}", opts.codec, opts.preset, hw.backend).to_lowercase()
        }
        Some(hw) => format!("{}/{:?}", opts.codec, hw.backend).to_lowercase(),
    }
}

fn cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Seconds to encode each of `infos` at `rate`, in order. `None` where the
/// duration is unknown.
pub fn times(infos: &[MediaInfo], opts: &CompressOptions, rate: Rate) -> Vec<Option<f64>> {
    infos
        .iter()
        .map(|info| work(info, opts).map(|w| rate.seconds(&w)))
        .collect()
}

/// The rate a finished encode achieved, when it ran long enough to say.
pub fn learnt(info: &MediaInfo, opts: &CompressOptions, elapsed_secs: f64) -> Option<f64> {
    if elapsed_secs < MIN_LEARN_SECS {
        return None;
    }
    work(info, opts).map(|w| w.pixels() / elapsed_secs)
}

/// The ffmpeg arguments for a benchmark with `opts`' codec, preset, and CRF.
/// Encodes to nowhere, so it writes nothing to disk.
pub fn benchmark_args(opts: &CompressOptions) -> Vec<OsString> {
    let video = video_args(opts, false);
    let mut args: Vec<String> = [
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
    ]
    .map(String::from)
    .to_vec();
    args.extend(video.global);
    args.extend(["-f", "lavfi", "-i", BENCH_SOURCE, "-frames:v", BENCH_FRAMES].map(String::from));
    if let Some(upload) = video.upload {
        args.extend(["-vf".to_string(), upload]);
    }
    args.extend(video.codec);
    args.extend(["-f", "null", "-"].map(String::from));
    args.into_iter().map(OsString::from).collect()
}

/// Pixels per second this machine encodes with `opts`, measured over the
/// steady part of a short synthetic encode.
///
/// Timed between progress reports after a warm-up, so neither ffmpeg's
/// start-up nor the encoder's opening burst, which a long encode sees once,
/// skews the rate. It measures the machine as it is: a busy one benchmarks
/// slow, and the first real encode corrects it.
pub fn benchmark(tools: &Tools, opts: &CompressOptions, cancel: &AtomicBool) -> Result<f64> {
    let args = benchmark_args(&opts.resolved(tools)?);
    let stop = AtomicBool::new(false);
    let started = Instant::now();
    let mut points: Vec<(Instant, f64)> = Vec::new();

    let mut first_output: Option<Instant> = None;

    let result = encode::run(
        tools,
        &args,
        Path::new("benchmark"),
        &stop,
        &mut |snapshot| {
            let now = Instant::now();
            if snapshot.out_time_secs > 0.0 {
                let first = *first_output.get_or_insert(now);
                if now.duration_since(first) >= BENCH_WARMUP {
                    points.push((now, snapshot.out_time_secs));
                }
            }
            let window = points
                .first()
                .map(|(first, _)| now.duration_since(*first))
                .unwrap_or_default();
            if cancel.load(Ordering::Relaxed)
                || window >= BENCH_WINDOW
                || now.duration_since(started) >= BENCH_LIMIT
            {
                stop.store(true, Ordering::Relaxed);
            }
        },
    );

    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    match result {
        // Stopping early is how a benchmark normally ends.
        Ok(()) | Err(Error::Cancelled) => {}
        Err(error) => return Err(error),
    }
    rate_between(&points).ok_or_else(|| Error::Tool {
        tool: "ffmpeg",
        message: "too slow to measure an encoding speed".into(),
    })
}

fn rate_between(points: &[(Instant, f64)]) -> Option<f64> {
    let (first, last) = (points.first()?, points.last()?);
    let secs = last.0.duration_since(first.0).as_secs_f64();
    let frames = (last.1 - first.1) * BENCH_FPS;
    (secs > 0.0 && frames > 0.0).then(|| frames * BENCH_PIXELS / secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{Backend, Hardware};
    use crate::options::{Codec, Engine, Preset};

    fn info() -> MediaInfo {
        MediaInfo {
            duration_secs: Some(127.0),
            size_bytes: 1,
            width: 3840,
            height: 2160,
            rotation: 0,
            fps: Some(60000.0 / 1001.0),
            video_codec: "av1".into(),
            pix_fmt: Some("yuv420p10le".into()),
            video_stream: 0,
            audio_codec: Some("aac".into()),
            audio_bitrate: None,
        }
    }

    #[test]
    fn work_follows_the_caps() {
        let full = work(&info(), &CompressOptions::default()).expect("work");
        assert_eq!((full.width, full.height), (3840, 2160));
        assert!((full.frames - 127.0 * 59.94).abs() < 0.1);

        let capped = CompressOptions {
            max_fps: Some(30),
            max_resolution: Some(1080),
            ..Default::default()
        };
        let small = work(&info(), &capped).expect("work");
        assert_eq!((small.width, small.height), (1920, 1080));
        assert_eq!(small.frames, 127.0 * 30.0);
        // Half the frames at a quarter of the pixels.
        assert!((full.pixels() / small.pixels() - 7.99).abs() < 0.01);

        let live = MediaInfo {
            duration_secs: None,
            ..info()
        };
        assert_eq!(work(&live, &capped), None);
    }

    #[test]
    fn seconds_at_a_rate() {
        // The 4K HDR sample from the issue: 7612 frames at 65 Mpx/s on an M2.
        let rate = Rate {
            pixels_per_sec: 65e6,
            basis: Basis::Measured,
        };
        let secs = times(&[info()], &CompressOptions::default(), rate)[0].expect("secs");
        assert!((secs / 60.0 - 16.2).abs() < 0.2, "{secs}");
    }

    #[test]
    fn encodes_override_benchmarks_and_then_average() {
        let opts = CompressOptions::default();
        let mut rates = Rates::default();
        rates.record_benchmark(&opts, 50e6);
        rates.record_encode(&opts, 70e6);
        let rate = rates.get(&opts).expect("rate");
        assert_eq!(rate.pixels_per_sec, 70e6);
        assert_eq!(rate.basis, Basis::Measured);

        rates.record_encode(&opts, 50e6);
        assert_eq!(rates.get(&opts).expect("rate").pixels_per_sec, 60e6);

        // A later benchmark cannot undo what real encodes taught.
        rates.record_benchmark(&opts, 10e6);
        assert_eq!(rates.get(&opts).expect("rate").pixels_per_sec, 60e6);
        let h264 = CompressOptions {
            codec: Codec::H264,
            ..Default::default()
        };
        assert_eq!(rates.get(&h264), None);
    }

    #[test]
    fn hardware_rates_are_kept_apart_from_software() {
        let software = CompressOptions::default();
        let hardware = |backend, preset| CompressOptions {
            engine: Engine::Hardware,
            preset,
            hardware: Some(Hardware {
                backend,
                name: String::new(),
                codecs: vec![Codec::H265],
                device: None,
            }),
            ..Default::default()
        };
        assert_eq!(key(&software), "h265/medium");
        // One speed only, so the preset is not part of it.
        assert_eq!(
            key(&hardware(Backend::Videotoolbox, Preset::Slow)),
            "h265/videotoolbox"
        );
        assert_eq!(
            key(&hardware(Backend::Nvenc, Preset::Slow)),
            "h265/slow/nvenc"
        );

        let mut rates = Rates::default();
        rates.record_encode(&hardware(Backend::Videotoolbox, Preset::Medium), 300e6);
        assert_eq!(rates.get(&software), None);
    }

    #[test]
    fn short_encodes_teach_nothing() {
        assert_eq!(learnt(&info(), &CompressOptions::default(), 2.0), None);
        assert!(learnt(&info(), &CompressOptions::default(), 600.0).is_some());
    }

    #[test]
    fn rates_round_trip_and_reject_other_machines() {
        let dir = std::env::temp_dir().join(format!("karui-rates-{}", std::process::id()));
        let path = dir.join("encode-rates.json");
        let mut rates = Rates::default();
        rates.record_encode(
            &CompressOptions {
                codec: Codec::H264,
                preset: Preset::Fast,
                ..Default::default()
            },
            1e8,
        );
        rates.save(&path).expect("save");
        assert_eq!(Rates::load(&path), rates);

        let foreign = Rates {
            cpus: cpus() + 1,
            ..rates
        };
        foreign.save(&path).expect("save");
        assert_eq!(Rates::load(&path), Rates::default());
        assert_eq!(Rates::load(&dir.join("missing.json")), Rates::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn benchmark_encodes_to_nowhere_with_the_chosen_settings() {
        let opts = CompressOptions {
            preset: Preset::Slow,
            crf: Some(30),
            ..Default::default()
        };
        let joined = benchmark_args(&opts)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("-progress pipe:1"));
        assert!(joined.contains("-c:v libx265 -preset slow -crf 30 -tag:v hvc1"));
        assert!(joined.ends_with("-f null -"));
        assert!(!joined.contains("file:"));
    }

    #[test]
    fn rate_needs_two_advancing_points() {
        let t = Instant::now();
        assert_eq!(rate_between(&[]), None);
        assert_eq!(rate_between(&[(t, 1.0)]), None);
        let later = t + Duration::from_secs(2);
        let rate = rate_between(&[(t, 1.0), (later, 3.0)]).expect("rate");
        // Two seconds of 30 fps 720p in two seconds of wall time.
        assert!((rate - 30.0 * BENCH_PIXELS).abs() < 1.0);
    }
}
