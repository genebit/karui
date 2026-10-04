//! Side-by-side stills for judging what compression costs.
//!
//! One frame from the source beside the same frame after compression, both
//! scaled to the same size and measured with SSIM and PSNR. Before a file is
//! compressed, the "after" comes from encoding a short sample with the
//! current settings; afterwards, from the output itself.
//!
//! The sample is a run of frames rather than one because a lone frame is
//! encoded as a keyframe, which an encoder spends far more bits on than the
//! frames between keyframes. A preview built from one would flatter every
//! setting.

use crate::args::{fps_cap, frame_args, metric_args, sample_args, thumbnail_args};
use crate::encode;
use crate::options::CompressOptions;
use crate::probe::{probe, MediaInfo};
use crate::tools::{command, Tools};
use crate::{Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Frames of the sample before the compared one, so the encoder has
/// settled into inter frames by the time it reaches it.
pub const SAMPLE_LEAD_FRAMES: f64 = 12.0;

/// The whole sample, in frames. Counted in frames rather than seconds
/// because encode time is per frame: a one-second sample of 4K60 AV1 took
/// 80 s at the default preset on an M2, this one 23 s. Its SSIM came out
/// within 0.01 of the longer sample's, in the same rating band.
pub const SAMPLE_FRAMES: f64 = 24.0;

/// Assumed when the probe found no frame rate.
const FALLBACK_FPS: f64 = 30.0;

/// Stills are no larger than 1080p on their shorter side. A 4K PNG is tens
/// of megabytes to pass to the webview, and the pane zooms to show detail.
pub const FRAME_MAX_SHORT_SIDE: u32 = 1080;

/// Thumbnails are this many pixels on their shorter side: twice the list's
/// thumbnail height, for Retina screens.
pub const THUMBNAIL_SHORT_SIDE: u32 = 80;

/// Seeking this close to the end can land after the last frame.
const END_MARGIN_SECS: f64 = 0.5;

/// A rough reading of SSIM for people who do not think in SSIM. Measured on
/// one frame at display size, so it is a guide; the stills are the evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    /// No difference most people can see side by side.
    Transparent,
    /// Visible when looked for, mostly in fine texture.
    Slight,
    /// Softness or blocking without having to look for it.
    Noticeable,
    Heavy,
}

impl Rating {
    pub fn from_ssim(ssim: f64) -> Rating {
        if ssim >= 0.985 {
            Rating::Transparent
        } else if ssim >= 0.95 {
            Rating::Slight
        } else if ssim >= 0.90 {
            Rating::Noticeable
        } else {
            Rating::Heavy
        }
    }
}

/// What to compare.
#[derive(Clone, Debug)]
pub struct Request {
    pub input: PathBuf,
    /// The finished output, when there is one. `None` encodes a sample with
    /// `options` instead.
    pub output: Option<PathBuf>,
    pub options: CompressOptions,
    /// Clamped to the file's length.
    pub at_secs: f64,
}

/// Reported while a comparison is under way. Mirrored by `PreviewStage` in
/// `src/lib/bindings.ts`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Stage {
    /// The source's still is written. A sample encode is far slower than the
    /// grab, so the window shows this one first.
    #[serde(rename_all = "camelCase")]
    Original {
        at_secs: f64,
        width: u32,
        height: u32,
        original: PathBuf,
    },
    /// How far the sample encode has got, from 0 to 1.
    Sampling { fraction: f64 },
}

/// Mirrored by `Comparison` in `src/lib/bindings.ts`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// The moment compared, after clamping.
    pub at_secs: f64,
    /// Both stills share these dimensions.
    pub width: u32,
    pub height: u32,
    pub original: PathBuf,
    pub compressed: PathBuf,
    /// `compressed` came from the finished output, not a sample.
    pub from_output: bool,
    /// The compressed video's own dimensions, before scaling to match.
    pub encoded_width: u32,
    pub encoded_height: u32,
    pub video_codec: String,
    /// `1.0` is identical. `None` if ffmpeg could not measure it.
    pub ssim: Option<f64>,
    /// In dB. `None` for identical frames, where PSNR is infinite.
    pub psnr: Option<f64>,
    pub rating: Option<Rating>,
    /// The whole file's size at the sample's bitrate. Samples only.
    pub estimated_bytes: Option<u64>,
    /// Decisions the encode made that were not asked for.
    pub notes: Vec<String>,
}

pub fn compare(
    tools: &Tools,
    request: &Request,
    dir: &Path,
    cancel: &AtomicBool,
    on_stage: &mut dyn FnMut(Stage),
) -> Result<Comparison> {
    request.options.validate()?;
    let info = probe(tools, &request.input)?;
    let at = clamp_time(request.at_secs, info.duration_secs);
    let size = frame_size(&info);
    std::fs::create_dir_all(dir)?;

    let original = dir.join("original.png");
    grab(
        tools,
        &request.input,
        info.video_stream,
        at,
        size,
        &original,
    )?;
    on_stage(Stage::Original {
        at_secs: at,
        width: size.0,
        height: size.1,
        original: original.clone(),
    });

    let (source, source_at, encoded, notes, estimated_bytes) = match &request.output {
        Some(output) => {
            let encoded = probe(tools, output)?;
            (output.clone(), at, encoded, Vec::new(), None)
        }
        None => {
            // The chosen encoder, hardware included, so the sample shows
            // what the real encode will.
            let options = request.options.resolved(tools)?;
            let (lead, secs) = sample_span(&info, &options);
            let start = (at - lead).max(0.0);
            let sample = dir.join("sample.mp4");
            let plan = sample_args(&request.input, &sample, &info, &options, start, secs);
            tracing::debug!(args = ?plan.args, "encoding preview sample");
            encode::run(tools, &plan.args, &request.input, cancel, &mut |snapshot| {
                if let Some(fraction) = snapshot.fraction(Some(secs)) {
                    on_stage(Stage::Sampling { fraction });
                }
            })?;
            let encoded = probe(tools, &sample)?;
            let estimated = estimate(
                encoded.size_bytes,
                encoded.duration_secs,
                info.duration_secs,
            );
            // The sample's clock starts at zero where the seek landed.
            (sample, at - start, encoded, plan.notes, estimated)
        }
    };

    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let compressed = dir.join("compressed.png");
    grab(
        tools,
        &source,
        encoded.video_stream,
        source_at,
        size,
        &compressed,
    )?;

    let (ssim, psnr) = measure(tools, &compressed, &original);
    let (encoded_width, encoded_height) = encoded.display_size();
    Ok(Comparison {
        at_secs: at,
        width: size.0,
        height: size.1,
        original,
        compressed,
        from_output: request.output.is_some(),
        encoded_width,
        encoded_height,
        video_codec: encoded.video_codec,
        ssim,
        psnr,
        rating: ssim.map(Rating::from_ssim),
        estimated_bytes,
        notes,
    })
}

/// The sample's lead-in and length in seconds, from its length in frames at
/// the rate the encode runs at.
fn sample_span(info: &MediaInfo, opts: &CompressOptions) -> (f64, f64) {
    let fps = fps_cap(info, opts)
        .map(f64::from)
        .or(info.fps)
        .filter(|fps| *fps > 0.0)
        .unwrap_or(FALLBACK_FPS);
    (SAMPLE_LEAD_FRAMES / fps, SAMPLE_FRAMES / fps)
}

/// A small JPEG of the frame a third of the way into `input`, the moment the
/// preview pane opens on. That far in skips the fade-in or title card many
/// clips open on.
pub fn thumbnail(tools: &Tools, input: &Path, info: &MediaInfo) -> Result<Vec<u8>> {
    let at = clamp_time(info.duration_secs.unwrap_or(0.0) / 3.0, info.duration_secs);
    let size = thumbnail_size(info);
    let fail = |message: String| Error::Encode {
        path: input.to_path_buf(),
        message,
    };
    let result = command(&tools.ffmpeg)
        .args(thumbnail_args(input, info.video_stream, at, size))
        .output()
        .map_err(|e| fail(format!("could not start ffmpeg: {e}")))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(fail(encode::summarise(&stderr, &result.status.to_string())));
    }
    if result.stdout.is_empty() {
        return Err(fail(format!("no frame at {at:.1}s")));
    }
    Ok(result.stdout)
}

/// An empty directory under `root` for one comparison, with every earlier
/// one removed. Only the latest comparison is ever on screen.
pub fn fresh_dir(root: &Path) -> std::io::Result<PathBuf> {
    match std::fs::remove_dir_all(root) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = root.join(stamp.to_string());
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn grab(
    tools: &Tools,
    input: &Path,
    stream: u32,
    at: f64,
    size: (u32, u32),
    output: &Path,
) -> Result<()> {
    let args = frame_args(input, stream, at, size, output);
    let fail = |message: String| Error::Encode {
        path: input.to_path_buf(),
        message,
    };
    let result = command(&tools.ffmpeg)
        .args(&args)
        .output()
        .map_err(|e| fail(format!("could not start ffmpeg: {e}")))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(fail(encode::summarise(&stderr, &result.status.to_string())));
    }
    // ffmpeg exits cleanly having written nothing when the seek lands past
    // the last frame.
    if !output.exists() {
        return Err(fail(format!("no frame at {at:.1}s")));
    }
    Ok(())
}

/// A failed measurement costs the numbers, not the stills.
fn measure(tools: &Tools, compressed: &Path, original: &Path) -> (Option<f64>, Option<f64>) {
    let result = command(&tools.ffmpeg)
        .args(metric_args(compressed, original))
        .output();
    match result {
        Ok(out) if out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            (parse_ssim(&stderr), parse_psnr(&stderr))
        }
        Ok(out) => {
            tracing::warn!(
                "could not measure the preview: {}",
                encode::summarise(
                    &String::from_utf8_lossy(&out.stderr),
                    &out.status.to_string()
                )
            );
            (None, None)
        }
        Err(e) => {
            tracing::warn!("could not measure the preview: {e}");
            (None, None)
        }
    }
}

/// `[Parsed_ssim_2 @ 0x…] SSIM R:0.83 (7.8) G:… All:0.822298 (7.503079)`
fn parse_ssim(stderr: &str) -> Option<f64> {
    let line = stderr.lines().find(|l| l.contains("SSIM "))?;
    let value = line.split_once("All:")?.1.split_whitespace().next()?;
    value.parse().ok().filter(|v: &f64| v.is_finite())
}

/// `[Parsed_psnr_3 @ 0x…] PSNR r:23.8 g:… average:24.465555 min:… max:…`
fn parse_psnr(stderr: &str) -> Option<f64> {
    let line = stderr.lines().find(|l| l.contains("PSNR "))?;
    let value = line.split_once("average:")?.1.split_whitespace().next()?;
    // Identical frames give `inf`, which has no useful number to show.
    value.parse().ok().filter(|v: &f64| v.is_finite())
}

fn clamp_time(at: f64, duration: Option<f64>) -> f64 {
    let at = if at.is_finite() { at.max(0.0) } else { 0.0 };
    match duration {
        Some(d) if d > 0.0 => at.min((d - END_MARGIN_SECS).max(0.0)),
        _ => at,
    }
}

/// Display size, shrunk to [`FRAME_MAX_SHORT_SIDE`] when larger. Both
/// stills use it, so a lower-resolution encode is scaled back up the way a
/// player would show it, and its lost detail is part of what is compared.
fn frame_size(info: &MediaInfo) -> (u32, u32) {
    fit_short_side(info, FRAME_MAX_SHORT_SIDE, |side| side.max(1))
}

/// Display size, shrunk to [`THUMBNAIL_SHORT_SIDE`]. Even, because the JPEG
/// encoder subsamples colour 2×2.
fn thumbnail_size(info: &MediaInfo) -> (u32, u32) {
    fit_short_side(info, THUMBNAIL_SHORT_SIDE, |side| (side & !1).max(2))
}

fn fit_short_side(info: &MediaInfo, max: u32, finish: fn(u32) -> u32) -> (u32, u32) {
    let (w, h) = info.display_size();
    let short = w.min(h);
    if short <= max || short == 0 {
        return (finish(w), finish(h));
    }
    let ratio = f64::from(max) / f64::from(short);
    let fit = |side: u32| finish((f64::from(side) * ratio).round() as u32);
    (fit(w), fit(h))
}

/// Scale the sample's size up to the whole file. Includes the sample's
/// container overhead and its opening keyframe, so it leans high on short
/// samples, which is the safer way to be wrong.
fn estimate(sample_bytes: u64, sample_secs: Option<f64>, total_secs: Option<f64>) -> Option<u64> {
    let sample_secs = sample_secs.filter(|s| *s > 0.0)?;
    let total_secs = total_secs.filter(|s| *s > 0.0)?;
    if total_secs <= sample_secs {
        return Some(sample_bytes);
    }
    Some((sample_bytes as f64 * total_secs / sample_secs).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(width: u32, height: u32, rotation: u32) -> MediaInfo {
        MediaInfo {
            duration_secs: Some(10.0),
            size_bytes: 1,
            width,
            height,
            rotation,
            fps: Some(30.0),
            video_codec: "h264".into(),
            pix_fmt: Some("yuv420p".into()),
            video_stream: 0,
            audio_codec: None,
            audio_bitrate: None,
        }
    }

    #[test]
    fn parses_ffmpeg_metric_lines() {
        let stderr = "\
[Parsed_psnr_3 @ 0xc6cc412c0] PSNR r:23.820621 g:25.004997 b:24.657742 average:24.465555 min:24.465555 max:24.465555
[Parsed_ssim_2 @ 0xc6cc41200] SSIM R:0.835405 (7.835843) G:0.813206 (7.286371) B:0.818283 (7.406041) All:0.822298 (7.503079)
";
        assert_eq!(parse_ssim(stderr), Some(0.822298));
        assert_eq!(parse_psnr(stderr), Some(24.465555));
    }

    #[test]
    fn identical_frames_have_no_psnr() {
        let stderr = "\
[Parsed_psnr_3 @ 0x1] PSNR r:inf g:inf b:inf average:inf min:inf max:inf
[Parsed_ssim_2 @ 0x2] SSIM R:1.000000 (inf) G:1.000000 (inf) B:1.000000 (inf) All:1.000000 (inf)
";
        assert_eq!(parse_ssim(stderr), Some(1.0));
        assert_eq!(parse_psnr(stderr), None);
        assert_eq!(parse_ssim("garbage"), None);
    }

    #[test]
    fn rating_bands() {
        assert_eq!(Rating::from_ssim(1.0), Rating::Transparent);
        assert_eq!(Rating::from_ssim(0.97), Rating::Slight);
        assert_eq!(Rating::from_ssim(0.92), Rating::Noticeable);
        assert_eq!(Rating::from_ssim(0.82), Rating::Heavy);
    }

    #[test]
    fn time_stays_inside_the_file() {
        assert_eq!(clamp_time(-4.0, Some(10.0)), 0.0);
        assert_eq!(clamp_time(30.0, Some(10.0)), 9.5);
        assert_eq!(clamp_time(3.0, Some(10.0)), 3.0);
        assert_eq!(clamp_time(f64::NAN, Some(10.0)), 0.0);
        assert_eq!(clamp_time(0.2, Some(0.3)), 0.0);
        assert_eq!(clamp_time(42.0, None), 42.0);
    }

    #[test]
    fn frames_keep_display_orientation_and_cap_the_short_side() {
        assert_eq!(frame_size(&info(1280, 720, 0)), (1280, 720));
        assert_eq!(frame_size(&info(3840, 2160, 0)), (1920, 1080));
        // A phone's portrait 4K is coded landscape with a 90° turn.
        assert_eq!(frame_size(&info(3840, 2160, 90)), (1080, 1920));
        assert_eq!(frame_size(&info(1281, 721, 0)), (1281, 721));
    }

    #[test]
    fn thumbnails_are_small_even_and_upright() {
        assert_eq!(thumbnail_size(&info(3840, 2160, 0)), (142, 80));
        assert_eq!(thumbnail_size(&info(1920, 1080, 90)), (80, 142));
        // Already small: kept, rounded down to even.
        assert_eq!(thumbnail_size(&info(75, 51, 0)), (74, 50));
    }

    #[test]
    fn sample_is_counted_in_frames_at_the_encoded_rate() {
        let opts = CompressOptions::default();
        assert_eq!(sample_span(&info(1920, 1080, 0), &opts), (0.4, 0.8));
        let sixty = MediaInfo {
            fps: Some(60.0),
            ..info(1920, 1080, 0)
        };
        assert_eq!(sample_span(&sixty, &opts), (0.2, 0.4));
        // Capped to 30, so the encode sees half the frames per second.
        let capped = CompressOptions {
            max_fps: Some(30),
            ..CompressOptions::default()
        };
        assert_eq!(sample_span(&sixty, &capped), (0.4, 0.8));
        let unknown = MediaInfo {
            fps: None,
            ..info(1920, 1080, 0)
        };
        assert_eq!(sample_span(&unknown, &opts), (0.4, 0.8));
    }

    #[test]
    fn estimate_scales_the_sample() {
        assert_eq!(estimate(1_000, Some(2.0), Some(60.0)), Some(30_000));
        assert_eq!(estimate(1_000, Some(2.0), Some(1.5)), Some(1_000));
        assert_eq!(estimate(1_000, None, Some(60.0)), None);
        assert_eq!(estimate(1_000, Some(2.0), None), None);
    }

    #[test]
    fn fresh_dir_clears_earlier_comparisons() {
        let root = std::env::temp_dir().join(format!("karui-preview-{}", std::process::id()));
        let first = fresh_dir(&root).expect("first");
        std::fs::write(first.join("original.png"), b"x").expect("write");
        let second = fresh_dir(&root).expect("second");
        assert!(second.is_dir());
        assert!(!first.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
