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
pub(crate) const MP4_AUDIO: &[&str] = &["aac", "mp3", "ac3", "eac3", "alac", "opus", "flac"];

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
    build(input, output, info, opts, None)
}

/// The same encode as [`ffmpeg_args`], limited to `secs` seconds from
/// `start`, for a preview sample. Sharing the builder means the sample can
/// never drift from what the real encode would do.
pub fn sample_args(
    input: &Path,
    output: &Path,
    info: &MediaInfo,
    opts: &CompressOptions,
    start: f64,
    secs: f64,
) -> EncodePlan {
    build(input, output, info, opts, Some((start, secs)))
}

/// One frame at `at` seconds, scaled to `width`×`height`, as a PNG.
///
/// PNG because a lossy still would add artefacts of its own to exactly the
/// picture meant to show the encoder's. RGB rather than the source's 10-bit
/// formats, which the webview cannot show.
pub fn frame_args(
    input: &Path,
    stream: u32,
    at: f64,
    (width, height): (u32, u32),
    output: &Path,
) -> Vec<OsString> {
    let mut args = Vec::new();
    let at = seconds(at);
    push(
        &mut args,
        &[
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-y",
            "-ss",
            &at,
            "-i",
        ],
    );
    args.push(file_url(input));
    let map = format!("0:{stream}");
    // Bicubic is what most players scale with, so a smaller encode looks
    // here the way it will when played full size.
    let scale = format!("scale={width}:{height}:flags=bicubic");
    push(
        &mut args,
        &[
            "-map",
            &map,
            "-frames:v",
            "1",
            "-vf",
            &scale,
            "-pix_fmt",
            "rgb24",
            "-c:v",
            "png",
            // `-update` writes one image to a plain name rather than
            // expecting a `%d` pattern.
            "-f",
            "image2",
            "-update",
            "1",
        ],
    );
    args.push(file_url(output));
    args
}

/// One frame at `at` seconds, scaled to `width`×`height`, as a JPEG on
/// stdout.
///
/// Only keyframes are decoded, so the frame is the keyframe at or before
/// `at`. Decoding forward from it to the exact moment took 4.7 s instead of
/// 0.3 s on a 4K AV1 file, and a thumbnail does not need the exact frame.
///
/// JPEG because a list of hundreds of thumbnails is a lot of PNG to pass to
/// the webview, and a lossy thumbnail is still recognisable. Piped rather than
/// written, so there are no files to clean up.
pub fn thumbnail_args(
    input: &Path,
    stream: u32,
    at: f64,
    (width, height): (u32, u32),
) -> Vec<OsString> {
    let mut args = Vec::new();
    let at = seconds(at);
    push(
        &mut args,
        &[
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-skip_frame",
            "nokey",
            "-ss",
            &at,
            "-i",
        ],
    );
    args.push(file_url(input));
    let map = format!("0:{stream}");
    let scale = format!("scale={width}:{height}");
    push(
        &mut args,
        &[
            "-map",
            &map,
            "-frames:v",
            "1",
            "-vf",
            &scale,
            "-c:v",
            "mjpeg",
            "-q:v",
            "4",
            "-f",
            "image2pipe",
            "pipe:1",
        ],
    );
    args
}

/// SSIM and PSNR of `compressed` against `original`, printed to stderr.
/// `info` is the level those filters report at.
pub fn metric_args(compressed: &Path, original: &Path) -> Vec<OsString> {
    let mut args = Vec::new();
    push(
        &mut args,
        &[
            "-hide_banner",
            "-nostdin",
            "-nostats",
            "-loglevel",
            "info",
            "-i",
        ],
    );
    args.push(file_url(compressed));
    push(&mut args, &["-i"]);
    args.push(file_url(original));
    push(
        &mut args,
        &[
            "-lavfi",
            "[0:v]split[a0][a1];[1:v]split[b0][b1];[a0][b0]ssim;[a1][b1]psnr",
            "-f",
            "null",
            "-",
        ],
    );
    args
}

fn build(
    input: &Path,
    output: &Path,
    info: &MediaInfo,
    opts: &CompressOptions,
    clip: Option<(f64, f64)>,
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
        ],
    );
    if let Some((start, secs)) = clip {
        // Before `-i`, so ffmpeg seeks the input instead of decoding up to
        // `start` and throwing the frames away.
        let (start, secs) = (seconds(start), seconds(secs));
        push(&mut args, &["-ss", &start, "-t", &secs]);
    }
    push(&mut args, &["-i"]);
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

/// Millisecond precision, and never `1e-7` notation, which ffmpeg rejects.
fn seconds(secs: f64) -> String {
    format!("{:.3}", secs.max(0.0))
}

/// The frame rate an encode caps to, when the source is faster than the cap.
pub fn fps_cap(info: &MediaInfo, opts: &CompressOptions) -> Option<u32> {
    let (max, source) = (opts.max_fps?, info.fps?);
    // The tolerance keeps 30000/1001 footage from being "capped" to 30.
    (source > f64::from(max) + 0.05).then_some(max)
}

/// The shorter side an encode scales down to, when the source is larger.
fn resolution_cap(info: &MediaInfo, opts: &CompressOptions) -> Option<u32> {
    let (w, h) = info.display_size();
    opts.max_resolution.filter(|cap| w.min(h) > *cap)
}

/// The encoded picture size, as the filters from [`video_filters`] produce
/// it. `-2` in a scale rounds the free side to the nearest even number.
pub fn output_size(info: &MediaInfo, opts: &CompressOptions) -> (u32, u32) {
    let (w, h) = info.display_size();
    let even = |side: f64| ((side / 2.0).round() as u32 * 2).max(2);
    match resolution_cap(info, opts) {
        Some(cap) if w >= h => (even(f64::from(w) * f64::from(cap) / f64::from(h)), cap),
        Some(cap) => (cap, even(f64::from(h) * f64::from(cap) / f64::from(w))),
        None => (w & !1, h & !1),
    }
}

fn video_filters(info: &MediaInfo, opts: &CompressOptions) -> Vec<String> {
    let mut filters = Vec::new();

    if let Some(max) = fps_cap(info, opts) {
        filters.push(format!("fps={max}"));
    }

    let (w, h) = info.display_size();
    match resolution_cap(info, opts) {
        // `-2` keeps the aspect ratio and rounds to an even number.
        Some(cap) if w >= h => filters.push(format!("scale=-2:{cap}")),
        Some(cap) => filters.push(format!("scale={cap}:-2")),
        // 4:2:0 needs even dimensions, and libx264 refuses odd ones outright.
        None if w % 2 != 0 || h % 2 != 0 => {
            filters.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".into());
        }
        None => {}
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
            audio_bitrate: None,
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

    #[test]
    fn sample_seeks_the_input_and_keeps_the_encode() {
        let opts = CompressOptions {
            max_resolution: Some(720),
            ..Default::default()
        };
        let plan = sample_args(
            Path::new("/in/a b.mov"),
            Path::new("/tmp/sample.mp4"),
            &info(),
            &opts,
            12.5,
            2.0,
        );
        let args = joined(&plan);
        assert!(args.contains("-y -ss 12.500 -t 2.000 -i file:/in/a b.mov"));
        let full = joined(&run(&info(), &opts));
        let tail =
            |s: &str| s[s.find("-map").expect("map")..s.rfind("file:").expect("out")].to_string();
        assert_eq!(tail(&args), tail(&full));
    }

    #[test]
    fn full_encode_has_no_seek() {
        let args = joined(&run(&info(), &CompressOptions::default()));
        assert!(!args.contains("-ss") && !args.contains("-t "));
    }

    #[test]
    fn frame_is_one_scaled_rgb_png() {
        let args = frame_args(
            Path::new("/in/a:b.mov"),
            1,
            3.25,
            (1280, 720),
            Path::new("/tmp/original.png"),
        );
        let joined = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            joined,
            "-hide_banner -nostdin -loglevel error -y -ss 3.250 -i file:/in/a:b.mov \
             -map 0:1 -frames:v 1 -vf scale=1280:720:flags=bicubic -pix_fmt rgb24 -c:v png \
             -f image2 -update 1 file:/tmp/original.png"
        );
    }

    #[test]
    fn thumbnail_is_one_scaled_jpeg_on_stdout() {
        let args = thumbnail_args(Path::new("/in/a:b.mov"), 1, 3.25, (256, 144));
        let joined = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            joined,
            "-hide_banner -nostdin -loglevel error -skip_frame nokey -ss 3.250 \
             -i file:/in/a:b.mov -map 0:1 -frames:v 1 -vf scale=256:144 -c:v mjpeg -q:v 4 \
             -f image2pipe pipe:1"
        );
    }

    #[test]
    fn metrics_compare_compressed_against_original() {
        let args = metric_args(Path::new("/tmp/c.png"), Path::new("/tmp/o.png"));
        let joined = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.starts_with("-hide_banner -nostdin -nostats -loglevel info"));
        assert!(joined.contains("-i file:/tmp/c.png -i file:/tmp/o.png"));
        assert!(joined.contains("[a0][b0]ssim;[a1][b1]psnr -f null -"));
    }

    #[test]
    fn output_size_matches_the_filters() {
        let cap = |res| CompressOptions {
            max_resolution: Some(res),
            ..Default::default()
        };
        assert_eq!(
            output_size(&info(), &CompressOptions::default()),
            (1920, 1080)
        );
        assert_eq!(output_size(&info(), &cap(720)), (1280, 720));
        let portrait = MediaInfo {
            rotation: 90,
            ..info()
        };
        assert_eq!(output_size(&portrait, &cap(720)), (720, 1280));
        let odd = MediaInfo {
            width: 1279,
            height: 719,
            ..info()
        };
        assert_eq!(output_size(&odd, &CompressOptions::default()), (1278, 718));
        // 4:3 at 480 is 640 wide; 1440×1080 → 640×480.
        let four_three = MediaInfo {
            width: 1440,
            height: 1080,
            ..info()
        };
        assert_eq!(output_size(&four_three, &cap(480)), (640, 480));
    }

    #[test]
    fn fps_cap_only_when_faster() {
        let opts = CompressOptions {
            max_fps: Some(30),
            ..Default::default()
        };
        assert_eq!(fps_cap(&info(), &opts), Some(30));
        let ntsc = MediaInfo {
            fps: Some(30000.0 / 1001.0),
            ..info()
        };
        assert_eq!(fps_cap(&ntsc, &opts), None);
        assert_eq!(fps_cap(&info(), &CompressOptions::default()), None);
    }

    #[test]
    fn seconds_never_use_exponents() {
        assert_eq!(seconds(1e-7), "0.000");
        assert_eq!(seconds(-3.0), "0.000");
        assert_eq!(seconds(90.0), "90.000");
    }
}
