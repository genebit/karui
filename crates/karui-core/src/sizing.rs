//! How big a file will come out with given settings, measured rather than
//! guessed.
//!
//! Output size depends on the footage far more than on the settings: the same
//! CRF shrinks a static interview by 90% and grainy night footage by 30%, so
//! no formula over resolution and bitrate gets close. Instead short stretches
//! from five points in the file are encoded with the real settings.
//!
//! They are scaled up by compression ratio, not by size: each sample's output
//! is compared with the source's own bytes for the same stretch, and that
//! ratio applied to the whole source. A dark, still scene is small in the
//! source and the output alike, so the ratio holds where raw sizes swing.
//! Scaling raw sizes from samples that happened to land on easy scenes put a
//! 4K demo reel at a third of its real size.
//!
//! Each sample opens on a keyframe, which costs many times an ordinary frame.
//! Averaging it in would inflate the estimate, so keyframes are counted apart
//! and charged at the encoder's real keyframe interval instead.

use crate::args::{audio_out, sample_args, AudioOut, KEYFRAME_INTERVAL};
use crate::encode;
use crate::estimate::work;
use crate::options::{Audio, CompressOptions, AAC_BITS_PER_SEC};
use crate::probe::{file_url, MediaInfo};
use crate::tools::{command, Tools};
use crate::{Error, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames per sample: past x264's 40-frame lookahead at its default preset
/// (x265 looks 20 ahead), so the encoder settles into the choices it makes
/// mid-file. Twelve-frame samples, shorter than the lookahead, came out at
/// about half the size of the same frames in a full encode.
const SEGMENT_FRAMES: f64 = 48.0;

/// Where in the file the samples are taken, as fractions of its length.
/// Spread over the whole file, since footage varies more along its length
/// than within any one stretch.
const SEGMENT_POINTS: [f64; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];

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

/// Video packet sizes from the samples, and the source's bytes for the same
/// stretches.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Packets {
    key_bytes: u64,
    keys: u32,
    other_bytes: u64,
    others: u32,
    source_bytes: u64,
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

    /// Video bytes for `frames` frames from a source with `source_video`
    /// bytes of video, by the samples' compression ratio. `None` without
    /// source bytes to compare against, so the caller can fall back.
    fn extrapolate_by_ratio(&self, frames: f64, source_video: f64) -> Option<f64> {
        if self.keys == 0 || self.others == 0 || self.source_bytes == 0 || source_video <= 0.0 {
            return None;
        }
        let key = self.key_bytes as f64 / f64::from(self.keys);
        let other = self.other_bytes as f64 / f64::from(self.others);
        // The samples as if their opening keyframes were ordinary frames;
        // keyframes are added back at their real interval.
        let ordinary = self.other_bytes as f64 + f64::from(self.keys) * other;
        let ratio = ordinary / self.source_bytes as f64;
        let keyframes = (frames / f64::from(KEYFRAME_INTERVAL)).ceil().max(1.0);
        Some(ratio * source_video + keyframes * (key - other).max(0.0))
    }

    /// Video bytes for `frames` frames.
    fn extrapolate(&self, frames: f64) -> Option<f64> {
        if self.keys == 0 || self.others == 0 {
            return None;
        }
        let key = self.key_bytes as f64 / f64::from(self.keys);
        let other = self.other_bytes as f64 / f64::from(self.others);
        let keyframes = (frames / f64::from(KEYFRAME_INTERVAL)).ceil().max(1.0);
        Some(frames * other + keyframes * (key - other).max(0.0))
    }
}

/// Audio bytes for the whole file, from its bitrate rather than by encoding.
fn audio_bytes(info: &MediaInfo, opts: &CompressOptions, duration: f64) -> f64 {
    let bits_per_sec = match audio_out(info, opts) {
        AudioOut::Silent => 0,
        AudioOut::Copy => info.audio_bitrate.unwrap_or(AAC_BITS_PER_SEC),
        AudioOut::Aac(bits_per_sec) => bits_per_sec,
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
    let offset = stream_start(tools, input, info.video_stream);
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
        // A failed read only costs the ratio; raw scaling is the fallback.
        packets.source_bytes +=
            source_bytes(tools, input, info.video_stream, offset, segment).unwrap_or(0);
    }

    let video = if plan.len() == 1 {
        Some(packets.total())
    } else {
        packets
            .extrapolate_by_ratio(work.frames, source_video_bytes(info, duration))
            .or_else(|| packets.extrapolate(work.frames))
    }
    .ok_or_else(|| Error::Encode {
        path: input.to_path_buf(),
        message: "the samples came out empty".into(),
    })?;
    let bytes = video + audio_bytes(info, opts, duration) + work.frames * MP4_BYTES_PER_FRAME;
    Ok(bytes.round() as u64)
}

/// The source's video bytes: the file less its audio, whose bitrate the
/// probe read. The container's own few kilobytes are lost in the rounding.
fn source_video_bytes(info: &MediaInfo, duration: f64) -> f64 {
    let audio = info.audio_bitrate.unwrap_or(0) as f64 / 8.0 * duration;
    (info.size_bytes as f64 - audio).max(info.size_bytes as f64 / 2.0)
}

/// When the video stream's clock starts. Camcorder `.MTS` files start at a
/// second or more, and `-ss` counts from there while ffprobe's intervals do
/// not.
fn stream_start(tools: &Tools, input: &Path, stream: u32) -> f64 {
    command(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", &stream.to_string()])
        .args(["-show_entries", "stream=start_time", "-of", "csv=p=0"])
        .arg(file_url(input))
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
        .filter(|start: &f64| start.is_finite())
        .unwrap_or(0.0)
}

/// The source's bytes for the stretch a sample encoded, reading only that
/// stretch of the file.
fn source_bytes(
    tools: &Tools,
    input: &Path,
    stream: u32,
    offset: f64,
    segment: &Segment,
) -> Result<u64> {
    let from = offset + segment.start;
    let to = from + segment.secs;
    let interval = read_interval(from, to);
    let output = command(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", &stream.to_string()])
        .args(["-read_intervals", &interval])
        .args(["-show_entries", "packet=pts_time,size", "-of", "csv=p=0"])
        .arg(file_url(input))
        .output()
        .map_err(|e| Error::Tool {
            tool: "ffprobe",
            message: e.to_string(),
        })?;
    Ok(sum_window(
        &String::from_utf8_lossy(&output.stdout),
        from,
        to,
    ))
}

/// The ffprobe `-read_intervals` covering `[from, to)`. The end is absolute:
/// ffprobe seeks back to the keyframe before the start, and a `+duration`
/// end counts from that keyframe. With keyframes seconds apart, as in AV1
/// and long-GOP camera footage, that read stopped short of the window and
/// halved the source bytes, which doubled the estimate.
fn read_interval(from: f64, to: f64) -> String {
    format!("{:.3}%{:.3}", (from - 1.0).max(0.0), to + 1.0)
}

/// Bytes of the `pts_time,size` packets that fall in `[from, to)`.
fn sum_window(csv: &str, from: f64, to: f64) -> u64 {
    csv.lines()
        .filter_map(|line| {
            let (pts, size) = line.split_once(',')?;
            let pts: f64 = pts.trim().parse().ok()?;
            let size: u64 = size.trim().split(',').next()?.parse().ok()?;
            (pts >= from && pts < to).then_some(size)
        })
        .sum()
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
            audio_channels: Some(2),
            audio_tracks: u32::from(audio.is_some()),
            interlaced: false,
        }
    }

    #[test]
    fn samples_five_points_or_the_whole_of_a_short_file() {
        let long = segments(100.0, 24.0);
        assert_eq!(long.len(), 5);
        // 48 frames at 24 fps, centred on 10%, 30% … 90%.
        assert!((long[0].secs - 2.0).abs() < 1e-9);
        assert!((long[2].start - (50.0 - 1.0)).abs() < 1e-9);
        assert!((long[4].start - (90.0 - 1.0)).abs() < 1e-9);

        assert_eq!(
            segments(2.0, 24.0),
            vec![Segment {
                start: 0.0,
                secs: 2.0
            }]
        );
    }

    #[test]
    fn scales_by_compression_ratio_not_raw_size() {
        let mut packets = Packets::default();
        // Two samples of a keyframe and three frames, where the source spent
        // 400 kB on each stretch: the output is a tenth of the source.
        for _ in 0..2 {
            packets.add("40000,K__\n10000,___\n10000,___\n10000,___\n");
            packets.source_bytes += 400_000;
        }
        // The whole source is 100 MB of video over 500 frames: two keyframes.
        let bytes = packets.extrapolate_by_ratio(500.0, 100e6).expect("bytes");
        assert_eq!(bytes, 0.1 * 100e6 + 2.0 * 30_000.0);
        // Raw scaling sees only that these frames were small, and says half.
        let raw = packets.extrapolate(500.0).expect("raw");
        assert!((raw / bytes - 0.5).abs() < 0.01, "{raw} vs {bytes}");

        // Without source bytes there is no ratio, and the caller falls back.
        let mut blind = packets;
        blind.source_bytes = 0;
        assert_eq!(blind.extrapolate_by_ratio(500.0, 100e6), None);
    }

    #[test]
    fn reads_to_an_absolute_end_whatever_keyframe_the_seek_lands_on() {
        // Not `9.600%+2.801`, which ends 2.8 s after wherever ffprobe's
        // seek lands, as early as the keyframe at 0.
        assert_eq!(read_interval(10.6, 11.4), "9.600%12.400");
        assert_eq!(read_interval(0.5, 1.3), "0.000%2.300");
    }

    #[test]
    fn sums_only_the_source_packets_in_the_stretch() {
        let csv = "9.9,1000\n10.0,200\n10.5,300\n11.99,400\n12.0,5000\nN/A,7\n";
        assert_eq!(sum_window(csv, 10.0, 12.0), 900);
    }

    #[test]
    fn source_video_is_the_file_less_its_audio() {
        let with_audio = info(Some("aac"), Some(128_000));
        let one_second = with_audio.size_bytes as f64; // 1 byte file: floored at half
        assert_eq!(source_video_bytes(&with_audio, 1.0), one_second / 2.0);
        let big = MediaInfo {
            size_bytes: 10_000_000,
            ..info(Some("aac"), Some(128_000))
        };
        assert_eq!(source_video_bytes(&big, 100.0), 10_000_000.0 - 1_600_000.0);
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
