//! What a file holds, read with ffprobe.
//!
//! The duration turns ffmpeg's elapsed output time into a percentage; the
//! dimensions, rotation, and frame rate decide which filters an encode needs.

use crate::tools::{command, Tools};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Mirrored by `MediaInfo` in `src/lib/bindings.ts`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// `None` for live captures and some broken files, which then encode with
    /// an indeterminate progress bar rather than not at all.
    pub duration_secs: Option<f64>,
    pub size_bytes: u64,
    /// Coded dimensions, before rotation.
    pub width: u32,
    pub height: u32,
    /// Clockwise degrees a player turns the picture, normalised to 0, 90,
    /// 180, or 270. Phones record portrait as landscape plus 90.
    pub rotation: u32,
    pub fps: Option<f64>,
    pub video_codec: String,
    /// e.g. `yuv420p`, or `yuv420p10le` for an iPhone's HDR footage.
    pub pix_fmt: Option<String>,
    /// The ffprobe stream index to map, which skips cover art: an MP4 can
    /// carry a still image as its first "video" stream.
    pub video_stream: u32,
    pub audio_codec: Option<String>,
}

impl MediaInfo {
    /// Dimensions as a player shows them. ffmpeg auto-rotates before any
    /// filter runs, so these are what a `scale` filter sees.
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation % 180 == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

/// `file:` stops ffmpeg reading a name containing a colon as a protocol. A
/// file called `clip:1.mp4`, legal on macOS and Linux, would otherwise be
/// opened with the nonexistent `clip` protocol.
pub fn file_url(path: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(path.as_os_str());
    url
}

pub fn probe(tools: &Tools, path: &Path) -> Result<MediaInfo> {
    let fail = |message: String| Error::Probe {
        path: path.to_path_buf(),
        message,
    };

    let size_bytes = std::fs::metadata(path)
        .map_err(|e| fail(e.to_string()))?
        .len();

    let output = command(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-i",
        ])
        .arg(file_url(path))
        .output()
        .map_err(|e| fail(format!("could not start ffprobe: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.lines().last().unwrap_or("not a readable media file");
        return Err(fail(reason.trim().to_string()));
    }

    parse(&String::from_utf8_lossy(&output.stdout), size_bytes).map_err(fail)
}

/// Probe many files at once.
///
/// One ffprobe spends most of its time starting up, so a folder of a hundred
/// clips takes seconds one at a time and a fraction of that across cores.
/// Results come back in input order.
pub fn probe_many(tools: &Tools, paths: &[PathBuf]) -> Vec<Result<MediaInfo>> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    let chunk = paths.len().div_ceil(workers).max(1);

    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|batch| {
                let handle = scope.spawn(move || {
                    batch
                        .iter()
                        .map(|path| probe(tools, path))
                        .collect::<Vec<_>>()
                });
                (batch, handle)
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|(batch, handle)| {
                // A panicking probe is a bug, but it should cost its own
                // chunk and keep every other result aligned with its path.
                handle.join().unwrap_or_else(|_| {
                    batch
                        .iter()
                        .map(|path| {
                            Err(Error::Probe {
                                path: path.clone(),
                                message: "ffprobe reader panicked".into(),
                            })
                        })
                        .collect()
                })
            })
            .collect()
    })
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    index: u32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    pix_fmt: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    disposition: Disposition,
    #[serde(default)]
    tags: Tags,
    #[serde(default)]
    side_data_list: Vec<SideData>,
}

#[derive(Default, Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Default, Deserialize)]
struct Tags {
    rotate: Option<String>,
}

#[derive(Deserialize)]
struct SideData {
    rotation: Option<f64>,
}

/// Parse `ffprobe -print_format json -show_format -show_streams`.
pub fn parse(json: &str, size_bytes: u64) -> std::result::Result<MediaInfo, String> {
    let probe: Probe =
        serde_json::from_str(json).map_err(|e| format!("unreadable ffprobe output: {e}"))?;

    let video = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video") && s.disposition.attached_pic == 0)
        .ok_or("no video stream")?;
    let audio = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"));

    let (width, height) = match (video.width, video.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
        _ => return Err("video stream has no dimensions".into()),
    };

    let duration_secs = probe
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .or(video.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d > 0.0);

    // `avg_frame_rate` is what a phone's variable-rate footage actually
    // averages; `r_frame_rate` is the timebase guess and can read 90000/1.
    let fps = video
        .avg_frame_rate
        .as_deref()
        .and_then(parse_rate)
        .or_else(|| video.r_frame_rate.as_deref().and_then(parse_rate));

    Ok(MediaInfo {
        duration_secs,
        size_bytes,
        width,
        height,
        rotation: rotation(video),
        fps,
        video_codec: video.codec_name.clone().unwrap_or_else(|| "unknown".into()),
        pix_fmt: video.pix_fmt.clone(),
        video_stream: video.index,
        audio_codec: audio.map(|a| a.codec_name.clone().unwrap_or_else(|| "unknown".into())),
    })
}

/// `30000/1001` → 29.97. `0/0`, which ffprobe reports when it does not know,
/// is `None`.
fn parse_rate(rate: &str) -> Option<f64> {
    let (num, den) = rate.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    (num > 0.0 && den > 0.0).then(|| num / den)
}

/// The display matrix stores rotation counter-clockwise and negative for a
/// phone held upright (`-90`); the older `rotate` tag stores it clockwise
/// (`90`). Both normalise to clockwise degrees.
fn rotation(stream: &Stream) -> u32 {
    let degrees = stream
        .side_data_list
        .iter()
        .find_map(|d| d.rotation)
        .map(|r| -r)
        .or_else(|| stream.tags.rotate.as_deref()?.parse::<f64>().ok())
        .unwrap_or(0.0);
    let quarter = (degrees / 90.0).round() as i64;
    (quarter.rem_euclid(4) * 90) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = r#"{
      "streams": [
        {"index": 0, "codec_type": "video", "codec_name": "hevc", "width": 1920, "height": 1080,
         "avg_frame_rate": "30000/1001", "r_frame_rate": "30/1",
         "side_data_list": [{"side_data_type": "Display Matrix", "rotation": -90}]},
        {"index": 1, "codec_type": "audio", "codec_name": "aac"}
      ],
      "format": {"duration": "12.345000"}
    }"#;

    #[test]
    fn reads_a_portrait_phone_clip() {
        let info = parse(PHONE, 1000).expect("parse");
        assert_eq!(info.rotation, 90);
        assert_eq!(info.display_size(), (1080, 1920));
        assert_eq!(info.duration_secs, Some(12.345));
        assert!((info.fps.expect("fps") - 29.97).abs() < 0.01);
        assert_eq!(info.audio_codec.as_deref(), Some("aac"));
        assert_eq!(info.video_stream, 0);
    }

    #[test]
    fn skips_cover_art() {
        let json = r#"{"streams": [
            {"index": 0, "codec_type": "video", "codec_name": "mjpeg", "width": 600, "height": 600,
             "disposition": {"attached_pic": 1}},
            {"index": 1, "codec_type": "video", "codec_name": "h264", "width": 1280, "height": 720,
             "avg_frame_rate": "0/0", "r_frame_rate": "25/1"}
          ], "format": {"duration": "N/A"}}"#;
        let info = parse(json, 0).expect("parse");
        assert_eq!(info.video_stream, 1);
        assert_eq!(info.video_codec, "h264");
        assert_eq!(info.fps, Some(25.0));
        assert_eq!(info.duration_secs, None);
        assert_eq!(info.audio_codec, None);
    }

    #[test]
    fn rejects_audio_only_files() {
        let json = r#"{"streams": [{"index": 0, "codec_type": "audio", "codec_name": "mp3"}]}"#;
        assert_eq!(parse(json, 0), Err("no video stream".into()));
    }

    #[test]
    fn reads_legacy_rotate_tag() {
        let json = r#"{"streams": [{"index": 0, "codec_type": "video", "width": 640, "height": 480,
            "tags": {"rotate": "270"}}]}"#;
        assert_eq!(parse(json, 0).expect("parse").rotation, 270);
    }

    #[test]
    fn prefixes_file_protocol() {
        assert_eq!(
            file_url(Path::new("/tmp/clip:1.mp4")),
            "file:/tmp/clip:1.mp4"
        );
    }
}
