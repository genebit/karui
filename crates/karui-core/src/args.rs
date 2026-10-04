//! The ffmpeg argument vector for one encode.
//!
//! Pure: everything an encode needs to decide is known from the probe and the
//! options, so every flag here is unit-tested without running ffmpeg.

use crate::hardware::{Backend, Hardware};
use crate::options::{Audio, Codec, CompressOptions, Engine, Preset, AAC_BITS_PER_SEC};
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
    let ten_bit = info
        .pix_fmt
        .as_deref()
        .is_some_and(|f| f.contains("10le") || f.contains("10be"));
    // Keeps an iPhone's HDR footage HDR. 8-bit would band visibly.
    let keep_ten_bit = ten_bit && opts.codec == Codec::H265;
    if ten_bit && !keep_ten_bit {
        notes.push("10-bit source reduced to 8-bit: H.264 players rarely decode 10-bit".into());
    }
    if info.interlaced {
        notes.push("interlaced source deinterlaced, at the same frame rate".into());
    }
    if opts.engine == Engine::Hardware && opts.hardware().is_none() {
        notes.push("no hardware encoder was set up, so this was encoded in software".into());
    }
    let video = video_args(opts, keep_ten_bit);
    args.extend(video.global.iter().map(OsString::from));

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

    args.extend(video.codec.iter().map(OsString::from));

    let mut filters = video_filters(info, opts);
    filters.extend(video.upload);
    if !filters.is_empty() {
        let chain = filters.join(",");
        push(&mut args, &["-vf", &chain]);
    }

    match audio_out(info, opts) {
        AudioOut::Silent => push(&mut args, &["-an"]),
        AudioOut::Copy => push(&mut args, &["-c:a", "copy"]),
        AudioOut::Aac(bits_per_sec) => {
            if let (Audio::Copy, Some(codec)) = (opts.audio, info.audio_codec.as_deref()) {
                notes.push(format!(
                    "{codec} audio cannot go in MP4 as-is; re-encoded to AAC"
                ));
            }
            let bitrate = format!("{}k", bits_per_sec / 1000);
            push(&mut args, &["-c:a", "aac", "-b:a", &bitrate]);
        }
    }

    // `faststart` moves the index to the front so a browser can start playing
    // before the download finishes. `-f mp4` because the working file ends in
    // `.part`, from which ffmpeg cannot guess a container.
    push(&mut args, &["-movflags", "+faststart", "-f", "mp4"]);
    args.push(file_url(output));

    EncodePlan { args, notes }
}

/// What an encode does with the audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AudioOut {
    Silent,
    Copy,
    /// Re-encoded to AAC at this many bits a second.
    Aac(u64),
}

/// Leeway over the AAC bitrate within which a source's AAC is copied. A
/// stream encoded at 128k reads anywhere from 125 to 132 kb/s in ffprobe.
const AAC_COPY_SLACK: f64 = 1.05;

pub(crate) fn audio_out(info: &MediaInfo, opts: &CompressOptions) -> AudioOut {
    let Some(codec) = info.audio_codec.as_deref() else {
        return AudioOut::Silent;
    };
    // `-c:a` and `-b:a` reach every track, but only the first was probed, so
    // a file with several keeps the old blanket rule.
    let one_track = info.audio_tracks <= 1;
    // Half for mono: 128k spread over one channel is twice what stereo gets
    // per channel, and well past where AAC stops sounding different.
    let aac = if one_track && info.audio_channels == Some(1) {
        AAC_BITS_PER_SEC / 2
    } else {
        AAC_BITS_PER_SEC
    };
    match opts.audio {
        Audio::Remove => AudioOut::Silent,
        Audio::Copy if MP4_AUDIO.contains(&codec) => AudioOut::Copy,
        Audio::Copy => AudioOut::Aac(aac),
        // AAC already at or under the target is copied: re-encoding it could
        // only lose quality, and would save nothing.
        Audio::Aac
            if one_track
                && codec == "aac"
                && info
                    .audio_bitrate
                    .is_some_and(|b| b as f64 <= aac as f64 * AAC_COPY_SLACK) =>
        {
            AudioOut::Copy
        }
        Audio::Aac => AudioOut::Aac(aac),
    }
}

/// The encoder's share of an ffmpeg command. Real encodes, preview and size
/// samples, the speed benchmark, and the hardware probe all build theirs
/// here, so none of them can measure a different encoder from the one used.
pub(crate) struct VideoArgs {
    /// Before the input: a device to open.
    pub global: Vec<String>,
    /// The encoder and its settings, after the filters.
    pub codec: Vec<String>,
    /// A last filter that moves frames into the encoder's own memory.
    pub upload: Option<String>,
}

pub(crate) fn video_args(opts: &CompressOptions, ten_bit: bool) -> VideoArgs {
    let hevc = opts.codec == Codec::H265;
    let Some(hw) = opts.hardware() else {
        let crf = opts.crf().to_string();
        let mut codec = strings(&[
            "-c:v",
            opts.codec.encoder(),
            "-preset",
            opts.preset.name(),
            "-crf",
            &crf,
        ]);
        if let Some(tune) = opts.content.tune() {
            codec.extend(strings(&["-tune", tune]));
        }
        if hevc {
            // Without `hvc1` QuickTime, Safari, and iOS refuse to play H.265
            // in MP4 at all; ffmpeg's default tag is `hev1`.
            codec.extend(strings(&[
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error",
            ]));
        }
        // Screen recorders often produce 4:4:4, which most players cannot
        // decode in H.264.
        let pix_fmt = if ten_bit { "yuv420p10le" } else { "yuv420p" };
        codec.extend(strings(&["-pix_fmt", pix_fmt]));
        return VideoArgs {
            global: Vec::new(),
            codec,
            upload: None,
        };
    };

    let qp = hardware_qp(opts).to_string();
    let mut codec = strings(&["-c:v", hw.backend.encoder(opts.codec)]);
    let mut global = Vec::new();
    let mut upload = None;
    match hw.backend {
        Backend::Videotoolbox => {
            let q = videotoolbox_quality(opts).to_string();
            codec.extend(strings(&["-q:v", &q]));
        }
        Backend::Vaapi => {
            if let Some(device) = &hw.device {
                global = vec!["-vaapi_device".into(), device.display().to_string()];
            }
            // Decoding and scaling stay on the CPU; the frames are handed to
            // the GPU last, in the layout its encoder reads.
            let layout = if ten_bit { "p010" } else { "nv12" };
            upload = Some(format!("format={layout},hwupload"));
            codec.extend(strings(&["-rc_mode", "CQP", "-qp", &qp]));
        }
        Backend::Nvenc => codec.extend(strings(&[
            "-preset",
            nvenc_preset(opts.preset),
            "-rc",
            "vbr",
            "-cq",
            &qp,
            "-b:v",
            "0",
        ])),
        Backend::Amf => {
            codec.extend(strings(&[
                "-quality",
                amf_quality(opts.preset),
                "-rc",
                "cqp",
                "-qp_i",
                &qp,
                "-qp_p",
                &qp,
            ]));
            if !hevc {
                codec.extend(strings(&["-qp_b", &qp]));
            }
        }
        Backend::Qsv => codec.extend(strings(&[
            "-preset",
            qsv_preset(opts.preset),
            "-global_quality",
            &qp,
        ])),
    }
    if ten_bit {
        codec.extend(strings(&["-profile:v", "main10"]));
    }
    if hevc {
        codec.extend(strings(&["-tag:v", "hvc1"]));
    }
    if hw.backend != Backend::Vaapi {
        // The layout every one of these encoders reads natively.
        codec.extend(strings(&[
            "-pix_fmt",
            if ten_bit { "p010le" } else { "nv12" },
        ]));
    }
    VideoArgs {
        global,
        codec,
        upload,
    }
}

/// The CRF on x265's scale: x264 at 23 looks about like x265 at 28.
fn hevc_scale_crf(opts: &CompressOptions) -> i32 {
    let crf = i32::from(opts.crf());
    if opts.codec == Codec::H264 {
        crf + 5
    } else {
        crf
    }
}

/// VideoToolbox's `-q:v` runs 1–100, higher being better. Measured on an
/// M2 against 4K HDR footage, q 44 matched x265's SSIM at CRF 28 (0.985)
/// at about 1.6 times the size, and every 8 points of q roughly doubled the
/// size, as 6 points of CRF do. So the default lands on 44, and each step of
/// the slider moves q by 1.25: the slider means the same look either way.
fn videotoolbox_quality(opts: &CompressOptions) -> u8 {
    (79.0 - 1.25 * f64::from(hevc_scale_crf(opts)))
        .round()
        .clamp(1.0, 100.0) as u8
}

/// A constant QP for the GPU encoders, on the codec's own 0–51 scale. A few
/// steps below the CRF: a hardware encoder spends its bits less cleverly,
/// and needs a finer quantizer to reach the same look.
fn hardware_qp(opts: &CompressOptions) -> u8 {
    (i32::from(opts.crf()) - 3).clamp(1, 51) as u8
}

/// NVENC's presets run `p1` (fastest) to `p7` (best).
fn nvenc_preset(preset: Preset) -> &'static str {
    match preset {
        Preset::Ultrafast | Preset::Superfast => "p1",
        Preset::Veryfast => "p2",
        Preset::Faster => "p3",
        Preset::Fast => "p4",
        Preset::Medium => "p5",
        Preset::Slow => "p6",
        Preset::Slower | Preset::Veryslow => "p7",
    }
}

fn amf_quality(preset: Preset) -> &'static str {
    match preset {
        Preset::Ultrafast | Preset::Superfast | Preset::Veryfast | Preset::Faster => "speed",
        Preset::Fast | Preset::Medium => "balanced",
        Preset::Slow | Preset::Slower | Preset::Veryslow => "quality",
    }
}

/// Quick Sync takes x264's names, from `veryfast` up.
fn qsv_preset(preset: Preset) -> &'static str {
    match preset {
        Preset::Ultrafast | Preset::Superfast => "veryfast",
        other => other.name(),
    }
}

/// A few frames of a test pattern to nowhere, with exactly the arguments a
/// real encode would use, to find out whether a hardware encoder works.
pub(crate) fn probe_args(hw: &Hardware, codec: Codec) -> Vec<OsString> {
    let opts = CompressOptions {
        codec,
        engine: Engine::Hardware,
        hardware: Some(hw.clone()),
        ..Default::default()
    };
    let video = video_args(&opts, false);
    let mut args = strings(&["-hide_banner", "-nostdin", "-loglevel", "error"]);
    args.extend(video.global);
    args.extend(strings(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=30",
        "-frames:v",
        "5",
    ]));
    if let Some(upload) = video.upload {
        args.extend(["-vf".to_string(), upload]);
    }
    args.extend(video.codec);
    args.extend(strings(&["-f", "null", "-"]));
    args.into_iter().map(OsString::from).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
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
    // First, while the two fields are still interleaved line by line: a
    // scale before it would blend them together. One frame out per frame in
    // keeps the frame rate every estimate assumes, and the combing it
    // removes is detail an encoder would otherwise spend bits on.
    if info.interlaced {
        filters.push("bwdif=mode=send_frame".into());
    }

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
    use crate::options::{Content, Preset};

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
            audio_channels: Some(2),
            audio_tracks: 1,
            interlaced: false,
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
    fn aac_already_within_the_bitrate_is_copied() {
        let aac = |bitrate| MediaInfo {
            audio_bitrate: Some(bitrate),
            ..info()
        };
        let opts = CompressOptions::default();
        let plan = run(&aac(96_000), &opts);
        assert!(joined(&plan).contains("-map 0:a? -c:v"));
        assert!(joined(&plan).contains("-c:a copy -movflags"));
        assert!(plan.notes.is_empty());
        // ffprobe's reading of a 128k encode wanders a little either side.
        assert!(joined(&run(&aac(131_000), &opts)).contains("-c:a copy"));
        assert!(joined(&run(&aac(192_000), &opts)).contains("-c:a aac -b:a 128k"));

        let unknown = MediaInfo {
            audio_bitrate: None,
            ..info()
        };
        assert!(joined(&run(&unknown, &opts)).contains("-c:a aac -b:a 128k"));
        let mp3 = MediaInfo {
            audio_codec: Some("mp3".into()),
            ..aac(96_000)
        };
        assert!(joined(&run(&mp3, &opts)).contains("-c:a aac -b:a 128k"));
        // `-c:a copy` would reach a second track nobody probed.
        let two_tracks = MediaInfo {
            audio_tracks: 2,
            ..aac(96_000)
        };
        assert!(joined(&run(&two_tracks, &opts)).contains("-c:a aac -b:a 128k"));
    }

    #[test]
    fn mono_gets_half_the_bitrate() {
        let mono = MediaInfo {
            audio_codec: Some("pcm_s16le".into()),
            audio_channels: Some(1),
            audio_bitrate: Some(768_000),
            ..info()
        };
        assert!(joined(&run(&mono, &CompressOptions::default())).contains("-c:a aac -b:a 64k"));
        // Also when copying was asked for and MP4 cannot hold the original.
        let copy = CompressOptions {
            audio: Audio::Copy,
            ..Default::default()
        };
        assert!(joined(&run(&mono, &copy)).contains("-c:a aac -b:a 64k"));

        let small = MediaInfo {
            audio_codec: Some("aac".into()),
            audio_bitrate: Some(64_000),
            ..mono.clone()
        };
        assert!(joined(&run(&small, &CompressOptions::default())).contains("-c:a copy"));
        let large = MediaInfo {
            audio_bitrate: Some(96_000),
            ..small.clone()
        };
        assert!(joined(&run(&large, &CompressOptions::default())).contains("-b:a 64k"));
        // A stereo second track must not be squeezed into 64k.
        let two_tracks = MediaInfo {
            audio_tracks: 2,
            ..mono
        };
        assert!(joined(&run(&two_tracks, &CompressOptions::default())).contains("-b:a 128k"));
    }

    #[test]
    fn deinterlaces_before_any_other_filter() {
        let interlaced = MediaInfo {
            interlaced: true,
            fps: Some(30000.0 / 1001.0),
            ..info()
        };
        let plan = run(&interlaced, &CompressOptions::default());
        assert!(joined(&plan).contains("-vf bwdif=mode=send_frame -c:a"));
        assert!(plan.notes.iter().any(|n| n.contains("deinterlaced")));

        let capped = CompressOptions {
            max_fps: Some(24),
            max_resolution: Some(720),
            ..Default::default()
        };
        assert!(joined(&run(&interlaced, &capped))
            .contains("-vf bwdif=mode=send_frame,fps=24,scale=-2:720"));
        // On the CPU before the frames go up to the GPU.
        let vaapi = joined(&run(&interlaced, &hardware(Backend::Vaapi)));
        assert!(vaapi.contains("-vf bwdif=mode=send_frame,format=nv12,hwupload"));

        assert!(!joined(&run(&info(), &CompressOptions::default())).contains("bwdif"));
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

    fn hardware(backend: Backend) -> CompressOptions {
        CompressOptions {
            engine: Engine::Hardware,
            hardware: Some(Hardware {
                backend,
                name: "test".into(),
                codecs: vec![Codec::H264, Codec::H265],
                device: (backend == Backend::Vaapi).then(|| "/dev/dri/renderD128".into()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn videotoolbox_encodes_on_the_media_engine() {
        let args = joined(&run(&info(), &hardware(Backend::Videotoolbox)));
        assert!(args.contains("-c:v hevc_videotoolbox -q:v 44 -tag:v hvc1 -pix_fmt nv12"));
        assert!(!args.contains("libx265") && !args.contains("-crf"));
        // Better quality on the slider is a higher q.
        let finer = CompressOptions {
            crf: Some(20),
            ..hardware(Backend::Videotoolbox)
        };
        assert!(joined(&run(&info(), &finer)).contains("-q:v 54"));
    }

    #[test]
    fn vaapi_opens_the_device_and_uploads_after_the_filters() {
        let opts = CompressOptions {
            max_resolution: Some(1080),
            ..hardware(Backend::Vaapi)
        };
        let uhd = MediaInfo {
            width: 3840,
            height: 2160,
            ..info()
        };
        let args = joined(&run(&uhd, &opts));
        assert!(args.contains("-y -vaapi_device /dev/dri/renderD128 -i file:"));
        assert!(args.contains("-vf scale=-2:1080,format=nv12,hwupload"));
        assert!(args.contains("-c:v hevc_vaapi -rc_mode CQP -qp 25 -tag:v hvc1"));
        assert!(!args.contains("-pix_fmt"));

        // HDR stays 10-bit on the GPU too.
        let hdr = MediaInfo {
            pix_fmt: Some("yuv420p10le".into()),
            ..info()
        };
        let args = joined(&run(&hdr, &hardware(Backend::Vaapi)));
        assert!(args.contains("-vf format=p010,hwupload"));
        assert!(args.contains("-profile:v main10"));
    }

    #[test]
    fn gpu_encoders_take_their_own_speed_and_quality_settings() {
        let slow = |backend| CompressOptions {
            preset: Preset::Slow,
            ..hardware(backend)
        };
        assert!(joined(&run(&info(), &slow(Backend::Nvenc))).contains(
            "-c:v hevc_nvenc -preset p6 -rc vbr -cq 25 -b:v 0 -tag:v hvc1 -pix_fmt nv12"
        ));
        let amf_h264 = CompressOptions {
            codec: Codec::H264,
            ..slow(Backend::Amf)
        };
        assert!(joined(&run(&info(), &amf_h264)).contains(
            "-c:v h264_amf -quality quality -rc cqp -qp_i 20 -qp_p 20 -qp_b 20 -pix_fmt nv12"
        ));
        assert!(joined(&run(&info(), &slow(Backend::Qsv)))
            .contains("-c:v hevc_qsv -preset slow -global_quality 25"));
    }

    #[test]
    fn content_tunes_the_software_encoders_only() {
        let animation = CompressOptions {
            content: Content::Animation,
            ..Default::default()
        };
        assert!(joined(&run(&info(), &animation))
            .contains("-c:v libx265 -preset medium -crf 28 -tune animation -tag:v hvc1"));
        let grain = CompressOptions {
            codec: Codec::H264,
            content: Content::Grain,
            ..Default::default()
        };
        assert!(joined(&run(&info(), &grain))
            .contains("-c:v libx264 -preset medium -crf 23 -tune grain -pix_fmt"));
        assert!(!joined(&run(&info(), &CompressOptions::default())).contains("-tune"));
        // VideoToolbox has no tunings; the setting is moot there.
        let hardware = CompressOptions {
            content: Content::Animation,
            ..hardware(Backend::Videotoolbox)
        };
        assert!(!joined(&run(&info(), &hardware)).contains("-tune"));
    }

    #[test]
    fn hardware_asked_for_but_missing_falls_back_out_loud() {
        let opts = CompressOptions {
            engine: Engine::Hardware,
            ..Default::default()
        };
        let plan = run(&info(), &opts);
        assert!(joined(&plan).contains("-c:v libx265"));
        assert!(plan.notes.iter().any(|n| n.contains("software")));
    }

    #[test]
    fn probe_encodes_a_test_pattern_to_nowhere() {
        let opts = hardware(Backend::Vaapi);
        let args = probe_args(opts.hardware.as_ref().expect("hw"), Codec::H264)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            args,
            "-hide_banner -nostdin -loglevel error -vaapi_device /dev/dri/renderD128 \
             -f lavfi -i testsrc2=size=320x240:rate=30 -frames:v 5 -vf format=nv12,hwupload \
             -c:v h264_vaapi -rc_mode CQP -qp 20 -f null -"
        );
    }

    #[test]
    fn seconds_never_use_exponents() {
        assert_eq!(seconds(1e-7), "0.000");
        assert_eq!(seconds(-3.0), "0.000");
        assert_eq!(seconds(90.0), "90.000");
    }
}
