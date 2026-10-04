//! Hardware encoding against a real ffmpeg and whatever encoder this machine
//! has. Ignored by default like `ffmpeg.rs`. On a machine with no working
//! hardware encoder, such as a CI runner, it reports that and passes.

use karui_core::batch::{self, Event};
use karui_core::hardware::detected;
use karui_core::options::{CompressOptions, Engine};
use karui_core::plan::plan;
use karui_core::probe::probe;
use karui_core::sizing::measure;
use karui_core::tools::{command, Tools};
use std::sync::atomic::AtomicBool;

#[test]
#[ignore = "needs ffmpeg and a hardware encoder"]
fn compresses_sizes_and_times_on_the_hardware_encoder() {
    let tools = Tools::locate().expect("ffmpeg on PATH");
    let Some(hw) = detected(&tools) else {
        eprintln!("no hardware encoder here; nothing to test");
        return;
    };
    eprintln!("testing {}", hw.name);

    let dir = std::env::temp_dir().join(format!("karui-e2e-hw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let clip = dir.join("it's a clip:hw.mov");
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
        .arg("testsrc2=size=1280x720:rate=30:duration=8,noise=alls=8:allf=t")
        .args(["-f", "lavfi", "-i", "sine=duration=8"])
        .args(["-c:v", "mpeg4", "-q:v", "2", "-c:a", "aac", "-shortest"])
        .arg(format!("file:{}", clip.display()))
        .status()
        .expect("run ffmpeg");
    assert!(status.success());

    let opts = CompressOptions {
        engine: Engine::Hardware,
        max_resolution: Some(480),
        ..Default::default()
    }
    .resolved(&tools)
    .expect("hardware resolves");
    let info = probe(&tools, &clip).expect("probe");
    let cancel = AtomicBool::new(false);

    let estimate = measure(&tools, &clip, &info, &opts, &dir, &cancel).expect("size estimate");
    let rate = karui_core::estimate::benchmark(&tools, &opts, &cancel).expect("benchmark");
    assert!(rate > 1e6, "rate {rate}");

    let jobs = plan(std::slice::from_ref(&clip), Some(&dir.join("out")), false);
    let mut events = Vec::new();
    let summary = batch::run(&tools, &jobs, &opts, &cancel, |e| events.push(e));
    assert_eq!(summary.succeeded, 1, "events: {events:?}");
    let out = probe(&tools, &jobs[0].output).expect("probe output");
    assert_eq!(out.video_codec, "hevc");
    assert_eq!(out.display_size(), (854, 480));
    let actual = std::fs::metadata(&jobs[0].output).expect("output").len();
    let ratio = estimate as f64 / actual as f64;
    assert!(
        (0.6..1.66).contains(&ratio),
        "estimated {estimate} for {actual}"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Progress { .. })));
    let _ = std::fs::remove_dir_all(&dir);
}
