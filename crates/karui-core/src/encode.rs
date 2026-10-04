//! Running one ffmpeg encode.

use crate::args::ffmpeg_args;
use crate::options::CompressOptions;
use crate::plan::{partial_path, Job};
use crate::probe::MediaInfo;
use crate::progress::{Parser, Snapshot};
use crate::tools::{command, Tools};
use crate::{Error, Result};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How often a quiet encode checks for cancellation. ffmpeg reports progress
/// twice a second, so this is only reached while it is still opening a file.
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// How much of ffmpeg's error output to keep in a failure message.
const ERROR_LINES: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub input_bytes: u64,
    /// What the encode came to, kept or not.
    pub output_bytes: u64,
    pub elapsed: Duration,
    /// The encode came out no smaller than its source beside it, so it was
    /// discarded and the original is the copy to keep.
    pub kept_original: bool,
}

pub fn encode(
    tools: &Tools,
    job: &Job,
    info: &MediaInfo,
    opts: &CompressOptions,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Snapshot),
) -> Result<Outcome> {
    let started = Instant::now();
    if let Some(dir) = job.output.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }

    let partial = partial_path(&job.output);
    let plan = ffmpeg_args(&job.input, &partial, info, opts);
    for note in &plan.notes {
        tracing::warn!("{}: {note}", file_name(&job.input));
    }
    tracing::debug!(args = ?plan.args, "starting ffmpeg");

    if let Err(error) = run(tools, &plan.args, &job.input, cancel, on_progress) {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }

    let output_bytes = std::fs::metadata(&partial)?.len();
    let kept_original = output_bytes >= info.size_bytes && beside_source(job);
    if kept_original {
        // Decided on the working file, so a file already at the output path,
        // which the user asked to overwrite, survives a discarded encode.
        let _ = std::fs::remove_file(&partial);
    } else {
        replace(&partial, &job.output)?;
        keep_modified_time(&job.input, &job.output);
    }
    Ok(Outcome {
        input_bytes: info.size_bytes,
        output_bytes,
        elapsed: started.elapsed(),
        kept_original,
    })
}

/// An output in its source's own folder is a second copy of the same video,
/// which is only worth keeping if it is smaller. One sent elsewhere, to an
/// output folder or off a camera card, is a file the user asked to have
/// there, so it is written however it came out.
fn beside_source(job: &Job) -> bool {
    job.output.parent() == job.input.parent()
}

/// Run one ffmpeg encode to completion, reporting progress and stopping when
/// `cancel` is set. Leaves whatever it wrote for the caller to keep or delete.
pub(crate) fn run(
    tools: &Tools,
    args: &[OsString],
    input: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Snapshot),
) -> Result<()> {
    let fail = |message: String| Error::Encode {
        path: input.to_path_buf(),
        message,
    };

    let mut child = command(&tools.ffmpeg)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(format!("could not start ffmpeg: {e}")))?;

    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Err(fail("ffmpeg's output pipes were not opened".into()));
    };

    // Both pipes are drained on their own threads. Reading only stdout would
    // deadlock the moment ffmpeg filled the stderr pipe buffer.
    let (lines, received) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
        {
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let errors = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = BufReader::new(stderr).read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut parser = Parser::default();
    let cancelled = loop {
        if cancel.load(Ordering::Relaxed) {
            break true;
        }
        match received.recv_timeout(CANCEL_POLL) {
            Ok(line) => {
                if let Some(snapshot) = parser.feed(&line) {
                    on_progress(snapshot);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break false,
        }
    };

    if cancelled {
        let _ = child.kill();
    }
    let status = child.wait()?;
    // A terminal's Ctrl-C reaches ffmpeg too, which may exit before the flag
    // is seen above. That is still a cancellation, not a failure.
    let cancelled = cancelled || cancel.load(Ordering::Relaxed);
    let _ = reader.join();
    let stderr = errors.join().unwrap_or_default();

    if cancelled {
        return Err(Error::Cancelled);
    }
    if !status.success() {
        return Err(fail(summarise(&stderr, &status.to_string())));
    }
    Ok(())
}

/// Rename over `to`. Windows refuses to rename onto an existing file, so the
/// old one goes first there; `plan` only targets an existing file when the
/// user asked to overwrite.
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    if cfg!(windows) && to.exists() {
        std::fs::remove_file(to)?;
    }
    std::fs::rename(from, to)
}

/// Give the output its source's modified time, so a folder of compressed
/// clips sorts by when they were recorded rather than when they were
/// compressed, which for a whole card import is all the same afternoon. Best
/// effort: a file system that refuses costs only the date.
fn keep_modified_time(input: &Path, output: &Path) {
    let Ok(modified) = std::fs::metadata(input).and_then(|m| m.modified()) else {
        return;
    };
    let result = std::fs::File::options()
        .write(true)
        .open(output)
        .and_then(|file| file.set_modified(modified));
    if let Err(e) = result {
        tracing::debug!(
            "could not keep the modified time of {}: {e}",
            output.display()
        );
    }
}

/// The last few lines ffmpeg wrote, which is where it says what went wrong.
pub(crate) fn summarise(stderr: &str, status: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return format!("ffmpeg {status}");
    }
    lines[lines.len().saturating_sub(ERROR_LINES)..].join("; ")
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_outputs_beside_their_source_are_discarded() {
        let job = |input: &str, output: &str| Job {
            input: input.into(),
            output: output.into(),
        };
        assert!(beside_source(&job("/v/a.mov", "/v/a-compressed.mp4")));
        assert!(!beside_source(&job("/v/a.mov", "/out/a.mp4")));
        assert!(!beside_source(&job(
            "/Volumes/CARD/DCIM/100/a.mp4",
            "/Users/me/Movies/karui/a.mp4"
        )));
    }

    #[test]
    fn summary_keeps_the_last_lines() {
        let stderr = "a\n\nb\nc\nUnknown encoder 'libx265'\n";
        assert_eq!(
            summarise(stderr, "exit status: 1"),
            "b; c; Unknown encoder 'libx265'"
        );
        assert_eq!(summarise("", "exit status: 1"), "ffmpeg exit status: 1");
    }
}
