//! Finding ffmpeg and ffprobe, and asking what they can do.
//!
//! An app launched from Finder or a desktop menu does not inherit the login
//! shell's `PATH`, so `/opt/homebrew/bin` — where Homebrew puts ffmpeg on
//! Apple Silicon — is invisible to it. Searching `PATH` alone would make the
//! desktop app report ffmpeg missing on exactly the machines that have it.

use crate::options::Codec;
use crate::{Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Overrides the search when set. Checked first, and an error if it names
/// something that is not a file, rather than silently falling through.
pub const FFMPEG_ENV: &str = "KARUI_FFMPEG";
pub const FFPROBE_ENV: &str = "KARUI_FFPROBE";

/// Searched after `PATH`, in order.
#[cfg(not(windows))]
const FALLBACK_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/local/bin",
    "/usr/bin",
    "/snap/bin",
];

#[cfg(windows)]
const FALLBACK_DIRS: &[&str] = &[r"C:\ffmpeg\bin", r"C:\Program Files\ffmpeg\bin"];

#[derive(Clone, Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub ffmpeg: String,
    pub ffprobe: String,
    pub version: String,
    /// Codecs this build of ffmpeg can encode. Distribution builds sometimes
    /// omit libx265, and the UI disables what is missing rather than letting
    /// every file fail with "Unknown encoder".
    pub encoders: Vec<Codec>,
}

impl Tools {
    pub fn locate() -> Result<Tools> {
        let ffmpeg = find("ffmpeg", FFMPEG_ENV, None)?;
        // A pair from one install, so the two never disagree about formats.
        let ffprobe = find("ffprobe", FFPROBE_ENV, ffmpeg.parent())?;
        Ok(Tools { ffmpeg, ffprobe })
    }

    pub fn status(&self) -> Result<ToolStatus> {
        let version = run_capture("ffmpeg", &self.ffmpeg, &["-hide_banner", "-version"])?;
        let encoders = run_capture("ffmpeg", &self.ffmpeg, &["-hide_banner", "-encoders"])?;
        Ok(ToolStatus {
            ffmpeg: self.ffmpeg.display().to_string(),
            ffprobe: self.ffprobe.display().to_string(),
            version: parse_version(&version),
            encoders: parse_encoders(&encoders),
        })
    }
}

/// How to get ffmpeg on this platform, for a "not found" message.
pub fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Install it with Homebrew: `brew install ffmpeg`. karui also looks in \
         /opt/homebrew/bin and /usr/local/bin, and KARUI_FFMPEG can name the binary directly."
    } else if cfg!(windows) {
        "Install it with `winget install ffmpeg`, or unpack a build to C:\\ffmpeg. \
         KARUI_FFMPEG can name ffmpeg.exe directly."
    } else {
        "Install it from your package manager: `sudo apt install ffmpeg`, \
         `sudo pacman -S ffmpeg`, or `sudo dnf install ffmpeg`. KARUI_FFMPEG can name the binary directly."
    }
}

fn executable(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    }
}

fn find(tool: &'static str, env: &str, beside: Option<&Path>) -> Result<PathBuf> {
    if let Some(value) = std::env::var_os(env) {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        return Err(Error::Tool {
            tool,
            message: format!("{env} is set to {}, which is not a file", path.display()),
        });
    }

    let name = executable(tool);
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();

    beside
        .map(Path::to_path_buf)
        .into_iter()
        .chain(path_dirs)
        .chain(winget_links())
        .chain(FALLBACK_DIRS.iter().map(PathBuf::from))
        .map(|dir| dir.join(&name))
        .find(|candidate| candidate.is_file())
        .ok_or(Error::ToolMissing { tool })
}

/// Where `winget install ffmpeg` links its binaries. Not on `PATH` until the
/// user opens a new session, which a running app never does.
fn winget_links() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var_os("LOCALAPPDATA").map(|base| {
        PathBuf::from(base)
            .join("Microsoft")
            .join("WinGet")
            .join("Links")
    })
}

/// A `Command` for one of the tools that opens no window of its own.
///
/// Without `CREATE_NO_WINDOW`, a GUI-subsystem app on Windows flashes a
/// console for every ffprobe and ffmpeg it starts.
pub fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd
}

fn run_capture(tool: &'static str, program: &Path, args: &[&str]) -> Result<String> {
    let output = command(program)
        .args(args)
        .output()
        .map_err(|e| Error::Tool {
            tool,
            message: format!("could not start {}: {e}", program.display()),
        })?;
    if !output.status.success() {
        return Err(Error::Tool {
            tool,
            message: format!(
                "exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `ffmpeg version 8.0 Copyright …` → `8.0`.
pub fn parse_version(banner: &str) -> String {
    banner
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("ffmpeg version "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("unknown")
        .to_string()
}

/// Which of our codecs appear in `ffmpeg -encoders`.
///
/// Lines look like ` V....D libx264   libx264 H.264 / AVC …`; the second
/// column is the encoder name.
pub fn parse_encoders(listing: &str) -> Vec<Codec> {
    let names: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();
    Codec::ALL
        .into_iter()
        .filter(|codec| names.contains(&codec.encoder()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_version_from_banner() {
        assert_eq!(
            parse_version("ffmpeg version 8.0 Copyright (c) 2000-2025 the FFmpeg developers\n"),
            "8.0"
        );
        assert_eq!(
            parse_version("ffmpeg version n6.1.1-1ubuntu1 Copyright"),
            "n6.1.1-1ubuntu1"
        );
        assert_eq!(parse_version(""), "unknown");
    }

    #[test]
    fn finds_encoders_by_exact_name() {
        let listing = "\
Encoders:
 V..... = Video
 ------
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC (codec h264)
 V....D libx264rgb           libx264 H.264 RGB (codec h264)
 V....D h264_videotoolbox    VideoToolbox H.264 Encoder (codec h264)
";
        assert_eq!(parse_encoders(listing), vec![Codec::H264]);
    }
}
