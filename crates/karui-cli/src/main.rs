use clap::{Parser, Subcommand};
use karui_core::batch::{self, Event};
use karui_core::discover::discover;
use karui_core::options::{Audio, Codec, CompressOptions, Content, Engine, Preset};
use karui_core::plan::plan_for;
use karui_core::probe::probe;
use karui_core::tools::{install_hint, Tools};
use karui_core::units;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "karui", version, about = "Make videos lighter with ffmpeg")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Increase log verbosity. Repeat for more.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Command {
    /// Compress videos. Folders are read one level deep; a camera card or a
    /// folder on one is searched all the way down.
    Compress {
        #[arg(required = true)]
        input: Vec<PathBuf>,

        /// Write every output here. Without it, each output goes beside its
        /// source as `<name>-compressed.mp4`, except files on a camera card.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Where files on a camera card go without --output. Defaults to
        /// karui in your Movies (macOS) or Videos folder.
        #[arg(long)]
        import_dir: Option<PathBuf>,

        /// h264 or h265.
        #[arg(long, default_value_t = Codec::default())]
        codec: Codec,

        /// software (x264/x265, smallest files) or hardware (the GPU or media
        /// engine, several times faster). See `karui doctor` for what is here.
        #[arg(long, default_value_t = Engine::default())]
        encoder: Engine,

        /// 0–51, lower is better. Defaults to 23 for h264 and 28 for h265.
        #[arg(long)]
        crf: Option<u8>,

        /// ultrafast … veryslow. Slower is smaller, never better looking.
        #[arg(long, default_value_t = Preset::default())]
        preset: Preset,

        /// general, animation (also screen recordings), or grain. Tunes the
        /// software encoders; hardware ones ignore it.
        #[arg(long, default_value_t = Content::default())]
        content: Content,

        /// Lower the frame rate to this when the source is faster.
        #[arg(long)]
        max_fps: Option<u32>,

        /// Scale down so the shorter side is at most this, e.g. 720.
        #[arg(long)]
        max_res: Option<u32>,

        /// aac (128k, 64k for mono; AAC already that small is copied),
        /// copy, or remove.
        #[arg(long, default_value_t = Audio::default())]
        audio: Audio,

        /// Replace existing outputs rather than numbering new ones.
        #[arg(long)]
        overwrite: bool,

        /// Ring the terminal bell when the batch ends.
        #[arg(long)]
        bell: bool,
    },
    /// Show what ffprobe reads from each file.
    Probe {
        #[arg(required = true)]
        input: Vec<PathBuf>,
    },
    /// Report where ffmpeg is and what it can encode.
    Doctor,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| level.into()),
        )
        .with_writer(std::io::stderr)
        .without_time()
        .init();

    match run(cli.command) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            if matches!(err, karui_core::Error::ToolMissing { .. }) {
                eprintln!("{}", install_hint());
            }
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> karui_core::Result<ExitCode> {
    let tools = Tools::locate()?;
    match command {
        Command::Compress {
            input,
            output,
            codec,
            crf,
            preset,
            content,
            max_fps,
            max_res,
            audio,
            import_dir,
            encoder,
            overwrite,
            bell,
        } => {
            let opts = CompressOptions {
                codec,
                engine: encoder,
                // Filled in by `resolved` below.
                hardware: None,
                crf,
                preset,
                content,
                max_fps,
                max_resolution: max_res,
                audio,
                output_dir: output,
                import_dir,
                overwrite,
            };
            opts.validate()?;
            let opts = opts.resolved(&tools)?;
            if let Some(hw) = opts.hardware() {
                eprintln!("encoding on {}", hw.name);
            }
            compress(&tools, &input, &opts, bell)
        }
        Command::Probe { input } => {
            for path in discover(&input).files {
                match probe(&tools, &path) {
                    Ok(info) => {
                        let (w, h) = info.display_size();
                        println!(
                            "{}\n  {w}×{h} {} {} · {} · {}",
                            path.display(),
                            info.video_codec,
                            info.fps.map(|f| format!("{f:.2} fps")).unwrap_or_default(),
                            info.duration_secs
                                .map(units::duration)
                                .unwrap_or_else(|| "unknown length".into()),
                            units::bytes(info.size_bytes),
                        );
                    }
                    Err(err) => println!("{}\n  {err}", path.display()),
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            let status = tools.status()?;
            println!("ffmpeg   {} ({})", status.ffmpeg, status.version);
            println!("ffprobe  {}", status.ffprobe);
            for codec in Codec::ALL {
                let mark = if status.encoders.contains(&codec) {
                    "yes"
                } else {
                    "missing"
                };
                println!("{:<8} {} {mark}", codec.name(), codec.encoder());
            }
            match &status.hardware {
                Some(hw) => {
                    let codecs: Vec<&str> = hw.codecs.iter().map(|c| c.name()).collect();
                    println!("hardware {} ({})", hw.name, codecs.join(", "));
                }
                None => println!("hardware none that works with this ffmpeg"),
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Return to the start of the line and erase it, so a shorter progress line
/// never leaves the tail of a longer one behind.
const CLEAR: &str = "\r\x1b[2K";

fn compress(
    tools: &Tools,
    input: &[PathBuf],
    opts: &CompressOptions,
    bell: bool,
) -> karui_core::Result<ExitCode> {
    let found = discover(input);
    for (path, reason) in &found.skipped {
        eprintln!("skipped {}: {reason}", path.display());
    }
    if found.files.is_empty() {
        return Err(karui_core::Error::Invalid("no videos to compress".into()));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = cancel.clone();
        // A second Ctrl-C while the first is being honoured does nothing
        // extra; the current encode is already being torn down.
        let _ = ctrlc::set_handler(move || cancel.store(true, Ordering::Relaxed));
    }

    let jobs = plan_for(&found.files, opts);
    let mut stderr = std::io::stderr();
    let summary = batch::run(tools, &jobs, opts, &cancel, |event| match event {
        Event::Started {
            index,
            total,
            input,
            output,
        } => {
            eprintln!("[{}/{total}] {input}\n      → {output}", index + 1);
        }
        Event::Progress {
            fraction,
            speed,
            eta_secs,
            ..
        } => {
            let pct = fraction
                .map(|f| format!("{:>3.0}%", f * 100.0))
                .unwrap_or_else(|| "  …".into());
            let speed = speed.map(|s| format!("{s:.2}x")).unwrap_or_default();
            let eta = eta_secs
                .map(|e| format!("eta {}", units::duration(e)))
                .unwrap_or_default();
            let _ = write!(stderr, "{CLEAR}      {pct}  {speed:>6}  {eta}");
            let _ = stderr.flush();
        }
        Event::Finished {
            input_bytes,
            output_bytes,
            elapsed_secs,
            ..
        } => {
            eprintln!(
                "{CLEAR}      {} → {} ({}) in {}",
                units::bytes(input_bytes),
                units::bytes(output_bytes),
                units::change(input_bytes, output_bytes),
                units::duration(elapsed_secs),
            );
        }
        Event::KeptOriginal {
            input_bytes,
            output_bytes,
            ..
        } => {
            eprintln!(
                "{CLEAR}      kept the original: compressed it came to {} ({})",
                units::bytes(output_bytes),
                units::change(input_bytes, output_bytes),
            );
        }
        Event::Failed { message, .. } => eprintln!("{CLEAR}      failed: {message}"),
        Event::Cancelled { .. } => eprintln!("{CLEAR}      cancelled"),
        Event::Done { .. } => {}
    });

    eprintln!(
        "\n{} done, {} kept as they were, {} failed, {} cancelled · {} → {} ({}) in {}",
        summary.succeeded,
        summary.kept,
        summary.failed,
        summary.cancelled,
        units::bytes(summary.input_bytes),
        units::bytes(summary.output_bytes),
        units::change(summary.input_bytes, summary.output_bytes),
        units::duration(summary.elapsed_secs),
    );
    if bell {
        eprint!("\x07");
    }

    Ok(if summary.cancelled > 0 {
        ExitCode::from(130)
    } else if summary.failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
