//! The ffmpeg argument vector for one encode.
//!
//! Pure: everything an encode needs to decide is known from the probe and the
//! options, so every flag here is unit-tested without running ffmpeg.

use crate::options::{Audio, Codec, CompressOptions, AAC_BITRATE};
use crate::probe::{file_url, MediaInfo};
use std::ffi::OsString;
use std::path::Path;

/// Audio codecs the MP4 container can carry as they are. Anything else —
/// PCM from a camera's `.mov`, Vorbis from a `.webm` — is re-encoded even
/// when copying was asked for, since the alternative is a failed file.
const MP4_AUDIO: &[&str] = &["aac", "mp3", "ac3", "eac3", "alac", "opus", "flac"];

#[derive(Debug, PartialEq)]
pub struct EncodePlan {
    pub args: Vec<OsString>,
    /// Decisions the user did not ask for and should hear about.
    pub notes: Vec<String>,
}

pub fn ffmpeg_args(
    input: &Path,
    output: &Path,
    info: &MediaInfo,
    opts: &CompressOptions,
) -> EncodePlan {
    let mut notes = Vec::new();
    let mut args: Vec<OsString> = Vec::new();

    // `-nostdin` because ffmpeg otherwise reads the terminal for `q`, and
    // under a GUI that read can stall. `-y` only ever replaces our own
    // `.part` file; the final path is decided by `plan`.
    push(
        &mut args,
        &[
            "-hide_banner",
            "-nostdin",
            "-nostats",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-y",
            "-i",
        ],
    );
    args.push(file_url(input));

    let video_map = format!("0:{}", info.video_stream);
    push(&mut args, &["-map", &video_map]);
    if opts.audio != Audio::Remove && info.audio_codec.is_some() {
        push(&mut args, &["-map", "0:a?"]);
    }

    let crf = opts.crf().to_string();
    push(
        &mut args,
        &[
            "-c:v",
            opts.codec.encoder(),
            "-preset",
            opts.preset.name(),
            "-crf",
            &crf,
        ],
    );

    if opts.codec == Codec::H265 {
        // Without `hvc1` QuickTime, Safari, and iOS refuse to play H.265 in
        // MP4 at all; ffmpeg's default tag is `hev1`.
        push(
            &mut args,
            &["-tag:v", "hvc1", "-x265-params", "log-level=error"],
        );
    }

    let ten_bit = info
        .pix_fmt
        .as_deref()
        .is_some_and(|f| f.contains("10le") || f.contains("10be"));
    let pix_fmt = if ten_bit && opts.codec == Codec::H265 {
        // Keeps an iPhone's HDR footage HDR. 8-bit would band visibly.
        "yuv420p10le"
    } else {
        if ten_bit {
            notes.push("10-bit source reduced to 8-bit: H.264 players rarely decode 10-bit".into());
        }
        // Screen recorders often produce 4:4:4, which most players cannot
        // decode in H.264.
        "yuv420p"
    };
    push(&mut args, &["-pix_fmt", pix_fmt]);

    let filters = video_filters(info, opts);
    if !filters.is_empty() {
        let chain = filters.join(",");
        push(&mut args, &["-vf", &chain]);
    }

    match (opts.audio, info.audio_codec.as_deref()) {
        (_, None) | (Audio::Remove, _) => push(&mut args, &["-an"]),
        (Audio::Copy, Some(codec)) if MP4_AUDIO.contains(&codec) => {
            push(&mut args, &["-c:a", "copy"])
        }
        (Audio::Copy, Some(codec)) => {
            notes.push(format!(
                "{codec} audio cannot go in MP4 as-is; re-encoded to AAC"
            ));
            push(&mut args, &["-c:a", "aac", "-b:a", AAC_BITRATE]);
        }
        (Audio::Aac, Some(_)) => push(&mut args, &["-c:a", "aac", "-b:a", AAC_BITRATE]),
    }

    // `faststart` moves the index to the front so a browser can start playing
    // before the download finishes. `-f mp4` because the working file ends in
    // `.part`, from which ffmpeg cannot guess a container.
    push(&mut args, &["-movflags", "+faststart", "-f", "mp4"]);
    args.push(file_url(output));

    EncodePlan { args, notes }
}

fn push(args: &mut Vec<OsString>, items: &[&str]) {
    args.extend(items.iter().map(OsString::from));
}

fn video_filters(info: &MediaInfo, opts: &CompressOptions) -> Vec<String> {
    let mut filters = Vec::new();

    if let (Some(max), Some(source)) = (opts.max_fps, info.fps) {
        // The tolerance keeps 30000/1001 footage from being "capped" to 30.
        if source > f64::from(max) + 0.05 {
            filters.push(format!("fps={max}"));
        }
    }

    let (w, h) = info.display_size();
    match opts.max_resolution {
        Some(cap) if w.min(h) > cap => {
            // `-2` keeps the aspect ratio and rounds to an even number.
            if w >= h {
                filters.push(format!("scale=-2:{cap}"));
            } else {
                filters.push(format!("scale={cap}:-2"));
            }
        }
        // 4:2:0 needs even dimensions, and libx264 refuses odd ones outright.
        _ if w % 2 != 0 || h % 2 != 0 => {
            filters.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".into());
        }
        _ => {}
    }

    filters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Preset;

    fn info() -> MediaInfo {
        MediaInfo {
            duration_secs: Some(10.0),
            size_bytes: 1,
            width: 1920,
            height: 1080,
            rotation: 0,
            fps: Some(60.0),
            video_codec: "h264".into(),
            pix_fmt: Some("yuv420p".into()),
            video_stream: 0,
            audio_codec: Some("aac".into()),
        }
    }

    fn joined(plan: &EncodePlan) -> String {
        plan.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn run(info: &MediaInfo, opts: &CompressOptions) -> EncodePlan {
        ffmpeg_args(
            Path::new("/in/a b.mov"),
            Path::new("/out/.a b.mp4.part"),
            info,
            opts,
        )
    }

    #[test]
    fn default_h265_encode() {
        let plan = run(&info(), &CompressOptions::default());
        assert_eq!(
            joined(&plan),
            "-hide_banner -nostdin -nostats -loglevel error -progress pipe:1 -y \
             -i file:/in/a b.mov -map 0:0 -map 0:a? -c:v libx265 -preset medium -crf 28 \
             -tag:v hvc1 -x265-params log-level=error -pix_fmt yuv420p \
             -c:a aac -b:a 128k -movflags +faststart -f mp4 file:/out/.a b.mp4.part"
        );
        assert!(plan.notes.is_empty());
    }

    #[test]
    fn paths_are_single_arguments() {
        let plan = run(&info(), &CompressOptions::default());
        assert!(plan.args.contains(&OsString::from("file:/in/a b.mov")));
    }

    #[test]
    fn caps_frame_rate_only_when_faster() {
        let opts = CompressOptions {
            max_fps: Some(30),
            ..Default::default()
        };
        assert!(joined(&run(&info(), &opts)).contains("-vf fps=30"));

        let ntsc = MediaInfo {
            fps: Some(30000.0 / 1001.0),
            ..info()
        };
        assert!(!joined(&run(&ntsc, &opts)).contains("fps="));

        let film = MediaInfo {
            fps: Some(24.0),
            ..info()
        };
        assert!(!joined(&run(&film, &opts)).contains("fps="));
    }

    #[test]
    fn caps_the_short_side_of_portrait_footage() {
        let opts = CompressOptions {
            max_resolution: Some(720),
            ..Default::default()
        };
        assert!(joined(&run(&info(), &opts)).contains("-vf scale=-2:720"));

        let portrait = MediaInfo {
            rotation: 90,
            ..info()
        };
        assert!(joined(&run(&portrait, &opts)).contains("-vf scale=720:-2"));

        let small = MediaInfo {
            width: 640,
            height: 360,
            ..info()
        };
        assert!(!joined(&run(&small, &opts)).contains("scale"));
    }

    #[test]
    fn evens_odd_dimensions() {
        let odd = MediaInfo {
            width: 1279,
            height: 719,
            ..info()
        };
        assert!(joined(&run(&odd, &CompressOptions::default()))
            .contains("-vf scale=trunc(iw/2)*2:trunc(ih/2)*2"));
    }

    #[test]
    fn combines_filters_in_order() {
        let opts = CompressOptions {
            max_fps: Some(30),
            max_resolution: Some(720),
            ..Default::default()
        };
        assert!(joined(&run(&info(), &opts)).contains("-vf fps=30,scale=-2:720"));
    }

    #[test]
    fn h264_has_no_hevc_tag() {
        let opts = CompressOptions {
            codec: Codec::H264,
            preset: Preset::Slow,
            ..Default::default()
        };
        let args = joined(&run(&info(), &opts));
        assert!(args.contains("-c:v libx264 -preset slow -crf 23"));
        assert!(!args.contains("hvc1"));
    }

    #[test]
    fn audio_modes() {
        let copy = CompressOptions {
            audio: Audio::Copy,
            ..Default::default()
        };
        assert!(joined(&run(&info(), &copy)).contains("-c:a copy"));

        let pcm = MediaInfo {
            audio_codec: Some("pcm_s16le".into()),
            ..info()
        };
        let plan = run(&pcm, &copy);
        assert!(joined(&plan).contains("-c:a aac"));
        assert_eq!(plan.notes.len(), 1);

        let remove = CompressOptions {
            audio: Audio::Remove,
            ..Default::default()
        };
        let args = joined(&run(&info(), &remove));
        assert!(args.contains("-an") && !args.contains("0:a?"));

        let silent = MediaInfo {
            audio_codec: None,
            ..info()
        };
        assert!(joined(&run(&silent, &CompressOptions::default())).contains("-an"));
    }

    #[test]
    fn ten_bit_survives_h265_only() {
        let hdr = MediaInfo {
            pix_fmt: Some("yuv420p10le".into()),
            ..info()
        };
        assert!(joined(&run(&hdr, &CompressOptions::default())).contains("-pix_fmt yuv420p10le"));

        let h264 = CompressOptions {
            codec: Codec::H264,
            ..Default::default()
        };
        let plan = run(&hdr, &h264);
        assert!(joined(&plan).contains("-pix_fmt yuv420p "));
        assert_eq!(plan.notes.len(), 1);
    }
}
