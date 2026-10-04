//! How big a file will come out with given settings, measured rather than
//! guessed.
//!
//! Output size depends on the footage far more than on the settings: the same
//! CRF shrinks a static interview by 90% and grainy night footage by 30%, so
//! no formula over resolution and bitrate gets close. Instead a few frames
//! from three points in the file are encoded with the real settings, and
//! their per-frame cost is scaled up to the whole file.
//!
//! Each sample opens on a keyframe, which costs many times an ordinary frame.
//! Averaging it in would inflate the estimate several-fold for short samples,
//! so keyframes are counted apart and charged at the encoder's real keyframe
//! interval instead.

use crate::args::{sample_args, MP4_AUDIO};
use crate::encode;
use crate::estimate::work;
use crate::options::{Audio, CompressOptions, AAC_BITS_PER_SEC};
use crate::probe::{file_url, MediaInfo};
use crate::tools::{command, Tools};
use crate::{Error, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames per sample. Enough for the encoder's B-frames and rate control to
/// settle past the opening keyframe, few enough that three samples of 4K
/// footage encode in a few seconds.
const SEGMENT_FRAMES: f64 = 12.0;

/// Where in the file the samples are taken, as fractions of its length.
/// Away from the ends, which are often a static title or a fade.
const SEGMENT_POINTS: [f64; 3] = [0.2, 0.5, 0.8];

/// x264's and x265's default maximum keyframe interval, which they reach on
/// footage without scene cuts. Scene-cut keyframes are part of what the
/// samples measure.
const KEYINT: f64 = 250.0;

/// MP4's index costs a dozen or so bytes per frame.
const MP4_BYTES_PER_FRAME: f64 = 12.0;

/// One stretch of the source to encode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub secs: f64,
}

/// Where to sample a file `duration` seconds long at `fps` output frames a
/// second. A file too short to sample three times is encoded whole, which
/// makes the estimate exact.
pub fn segments(duration: f64, fps: f64) -> Vec<Segment> {
    let secs = SEGMENT_FRAMES / fps;
    if duration <= secs * SEGMENT_POINTS.len() as f64 * 2.0 {
        return vec![Segment {
            start: 0.0,
            secs: duration,
        }];
    }
    SEGMENT_POINTS
        .iter()
        .map(|point| Segment {
            start: (duration * point - secs / 2.0).max(0.0),
            secs,
        })
        .collect()
}

/// Video packet sizes from the samples.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Packets {
    key_bytes: u64,
    keys: u32,
    other_bytes: u64,
    others: u32,
}

impl Packets {
    /// Add one sample's `ffprobe -show_entries packet=size,flags -of csv=p=0`
    /// output: `size,flags` per line, in decode order. Only the opening
    /// keyframe counts as a keyframe; any later one is a scene cut, which
    /// belongs to the footage and is averaged in.
    fn add(&mut self, csv: &str) {
        let mut first = true;
        for line in csv.lines() {
            let Some(size) = line
                .split(',')
                .next()
                .and_then(|s| s.trim().parse::<u64>().ok())
            else {
                continue;
            };
            if first {
                self.key_bytes += size;
                self.keys += 1;
                first = false;
            } else {
                self.other_bytes += size;
                self.others += 1;
            }
        }
    }

    fn total(&self) -> f64 {
        (self.key_bytes + self.other_bytes) as f64
    }

    /// Video bytes for `frames` frames.
    fn extrapolate(&self, frames: f64) -> Option<f64> {
        if self.keys == 0 || self.others == 0 {
            return None;
        }
        let key = self.key_bytes as f64 / f64::from(self.keys);
        let other = self.other_bytes as f64 / f64::from(self.others);
        let keyframes = (frames / KEYINT).ceil().max(1.0);
        Some(frames * other + keyframes * (key - other).max(0.0))
    }
}

/// Audio bytes for the whole file, from its bitrate rather than by encoding.
fn audio_bytes(info: &MediaInfo, opts: &CompressOptions, duration: f64) -> f64 {
    let bits_per_sec = match (opts.audio, info.audio_codec.as_deref()) {
        (_, None) | (Audio::Remove, _) => 0,
        (Audio::Copy, Some(codec)) if MP4_AUDIO.contains(&codec) => {
            info.audio_bitrate.unwrap_or(AAC_BITS_PER_SEC)
        }
        // Re-encoded to AAC, whether asked for or because MP4 cannot hold it.
        _ => AAC_BITS_PER_SEC,
    };
    bits_per_sec as f64 / 8.0 * duration
}

/// Estimated output bytes for `input` compressed with `opts`. Writes its
/// samples to `dir` and removes them.
pub fn measure(
    tools: &Tools,
    input: &Path,
    info: &MediaInfo,
    opts: &CompressOptions,
    dir: &Path,
    cancel: &AtomicBool,
) -> Result<u64> {
    opts.validate()?;
    // Sized with the encoder that will do the work: hardware encoders
    // spend bits differently.
    let opts = &opts.resolved(tools)?;
    let (Some(duration), Some(work)) = (info.duration_secs, work(info, opts)) else {
        return Err(Error::Invalid("the length of this video is unknown".into()));
    };
    let fps = work.frames / duration;
    // Audio is sized from its bitrate, so the samples skip it.
    let video_only = CompressOptions {
        audio: Audio::Remove,
        ..opts.clone()
    };
    std::fs::create_dir_all(dir)?;

    let plan = segments(duration, fps);
    let mut packets = Packets::default();
    for (i, segment) in plan.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let sample = dir.join(format!("size-{i}.mp4"));
        let args = sample_args(
            input,
            &sample,
            info,
            &video_only,
            segment.start,
            segment.secs,
        )
        .args;
        let encoded = encode::run(tools, &args, input, cancel, &mut |_| {})
            .and_then(|()| read_packets(tools, &sample));
        let _ = std::fs::remove_file(&sample);
        packets.add(&encoded?);
    }

    let video = if plan.len() == 1 {
        Some(packets.total())
    } else {
        packets.extrapolate(work.frames)
    }
    .ok_or_else(|| Error::Encode {
        path: input.to_path_buf(),
        message: "the samples came out empty".into(),
    })?;
    let bytes = video + audio_bytes(info, opts, duration) + work.frames * MP4_BYTES_PER_FRAME;
    Ok(bytes.round() as u64)
}

fn read_packets(tools: &Tools, sample: &Path) -> Result<String> {
    let output = command(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=size,flags",
            "-of",
            "csv=p=0",
        ])
        .arg(file_url(sample))
        .output()
        .map_err(|e| Error::Tool {
            tool: "ffprobe",
            message: e.to_string(),
        })?;
    if !output.status.success() {
        return Err(Error::Tool {
            tool: "ffprobe",
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(audio: Option<&str>, bitrate: Option<u64>) -> MediaInfo {
        MediaInfo {
            duration_secs: Some(100.0),
            size_bytes: 1,
            width: 1920,
            height: 1080,
            rotation: 0,
            fps: Some(24.0),
            video_codec: "hevc".into(),
            pix_fmt: Some("yuv420p".into()),
            video_stream: 0,
            audio_codec: audio.map(Into::into),
            audio_bitrate: bitrate,
        }
    }

    #[test]
    fn samples_three_points_or_the_whole_of_a_short_file() {
        let long = segments(100.0, 24.0);
        assert_eq!(long.len(), 3);
        assert!((long[1].start - (50.0 - 0.25)).abs() < 1e-9);
        assert!((long[0].secs - 0.5).abs() < 1e-9);

        assert_eq!(
            segments(2.0, 24.0),
            vec![Segment {
                start: 0.0,
                secs: 2.0
            }]
        );
    }

    #[test]
    fn opening_keyframe_is_charged_at_the_keyframe_interval() {
        let mut packets = Packets::default();
        // A 100 kB keyframe, then eleven 10 kB frames, in each of 3 samples.
        let sample = std::iter::once("100000,K__".to_string())
            .chain((0..11).map(|_| "10000,___".to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        for _ in 0..3 {
            packets.add(&sample);
        }
        // 2400 frames: 10 keyframes at the 250-frame interval.
        let bytes = packets.extrapolate(2400.0).expect("bytes");
        assert_eq!(bytes, 2400.0 * 10_000.0 + 10.0 * 90_000.0);
        // Averaging the keyframe in would have claimed 17.5 kB a frame.
        assert!(bytes < 2400.0 * 17_500.0 * 0.6);
    }

    #[test]
    fn scene_cut_keyframes_count_as_footage() {
        let mut packets = Packets::default();
        packets.add("50000,K__\n8000,___\n40000,K__\n8000,___\n");
        assert_eq!(packets.keys, 1);
        assert_eq!(packets.others, 3);
        assert_eq!(Packets::default().extrapolate(10.0), None);
    }

    #[test]
    fn audio_is_sized_from_its_bitrate() {
        let aac = CompressOptions::default();
        assert_eq!(
            audio_bytes(&info(Some("aac"), Some(320_000)), &aac, 100.0),
            1_600_000.0
        );
        let copy = CompressOptions {
            audio: Audio::Copy,
            ..Default::default()
        };
        assert_eq!(
            audio_bytes(&info(Some("aac"), Some(320_000)), &copy, 100.0),
            4_000_000.0
        );
        // PCM cannot be copied into MP4, so it is AAC after all.
        assert_eq!(
            audio_bytes(&info(Some("pcm_s16le"), Some(1_536_000)), &copy, 100.0),
            1_600_000.0
        );
        let remove = CompressOptions {
            audio: Audio::Remove,
            ..Default::default()
        };
        assert_eq!(audio_bytes(&info(Some("aac"), None), &remove, 100.0), 0.0);
        assert_eq!(audio_bytes(&info(None, None), &aac, 100.0), 0.0);
    }
}
