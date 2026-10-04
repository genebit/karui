//! Size estimates against real encodes.
//!
//! Ignored by default, like `ffmpeg.rs`: CI runs them with `--include-ignored`.

use karui_core::encode::encode;
use karui_core::options::{Codec, CompressOptions, Preset};
use karui_core::plan::Job;
use karui_core::probe::probe;
use karui_core::sizing::measure;
use karui_core::tools::{command, Tools};
use std::sync::atomic::AtomicBool;

#[test]
#[ignore = "needs ffmpeg and ffprobe"]
fn estimate_lands_near_the_real_size() {
    let tools = Tools::locate().expect("ffmpeg on PATH");
    let dir = std::env::temp_dir().join(format!("karui-e2e-sizing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");

    // Twelve seconds, long enough to be sampled rather than encoded whole,
    // with noise so the encoder has real work to size.
    let clip = dir.join("it's a clip:noisy.mov");
    let status = command(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("testsrc2=size=640x360:rate=30:duration=12,noise=alls=10:allf=t")
        .args(["-f", "lavfi", "-i", "sine=duration=12"])
        .args(["-c:v", "mpeg4", "-q:v", "2", "-c:a", "aac", "-shortest"])
        .arg(format!("file:{}", clip.display()))
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "could not generate the clip");

    let info = probe(&tools, &clip).expect("probe");
    let cancel = AtomicBool::new(false);
    for opts in [
        CompressOptions {
            codec: Codec::H264,
            preset: Preset::Veryfast,
            ..Default::default()
        },
        CompressOptions {
            preset: Preset::Fast,
            ..Default::default()
        },
    ] {
        let estimate = measure(&tools, &clip, &info, &opts, &dir, &cancel).expect("estimate");
        let job = Job {
            input: clip.clone(),
            output: dir.join("full.mp4"),
        };
        let actual = encode(&tools, &job, &info, &opts, &cancel, &mut |_| {})
            .expect("encode")
            .output_bytes;
        let ratio = estimate as f64 / actual as f64;
        assert!(
            (0.75..1.33).contains(&ratio),
            "{:?}: estimated {estimate} for {actual}",
            opts.codec
        );
    }
    // Its samples are cleaned up.
    assert!(std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().starts_with("size-")));
    let _ = std::fs::remove_dir_all(&dir);
}
