//! Turning what the user picked — files, folders, or both — into a list of
//! videos.
//!
//! The original script handed every entry of a folder to ffmpeg: subfolders,
//! `.DS_Store`, and its own earlier outputs included. Each then failed or,
//! worse, was compressed a second time.

use crate::plan::OUTPUT_SUFFIX;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Lowercase extensions treated as video.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "avi", "flv", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "mts", "mxf", "ogv",
    "ts", "webm", "wmv",
];

#[derive(Debug, Default, PartialEq)]
pub struct Discovery {
    pub files: Vec<PathBuf>,
    /// Paths the user named explicitly that cannot be compressed, and why.
    /// Unsuitable entries found *inside* a folder are dropped silently: a
    /// folder of footage with a few photos in it should not read as an error.
    pub skipped: Vec<(PathBuf, String)>,
}

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Hidden files, which include karui's own `.name.mp4.part` working files.
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

/// Something karui wrote beside its source on an earlier run.
fn is_previous_output(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with(OUTPUT_SUFFIX))
}

/// Expand `paths` in order. Folders are read one level deep, sorted by name so
/// a batch runs in the order a file manager shows. A file reached twice is
/// listed once.
pub fn discover(paths: &[PathBuf]) -> Discovery {
    let mut found = Discovery::default();
    let mut seen = HashSet::new();
    let mut add = |found: &mut Discovery, path: PathBuf| {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.insert(key) {
            found.files.push(path);
        }
    };

    for path in paths {
        if path.is_dir() {
            let mut entries: Vec<PathBuf> = match std::fs::read_dir(path) {
                Ok(read) => read.filter_map(|e| e.ok().map(|e| e.path())).collect(),
                Err(e) => {
                    found.skipped.push((path.clone(), e.to_string()));
                    continue;
                }
            };
            entries.sort();
            for entry in entries {
                if entry.is_file()
                    && is_video(&entry)
                    && !is_hidden(&entry)
                    && !is_previous_output(&entry)
                {
                    add(&mut found, entry);
                }
            }
        } else if !path.exists() {
            found.skipped.push((path.clone(), "does not exist".into()));
        } else if !is_video(path) {
            found
                .skipped
                .push((path.clone(), "not a recognised video file".into()));
        } else {
            add(&mut found, path.clone());
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("karui-discover-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn touch(path: &Path) {
        std::fs::write(path, b"").expect("touch");
    }

    #[test]
    fn folder_yields_only_videos_in_name_order() {
        let dir = scratch("folder");
        for name in [
            "b.MOV",
            "a.mp4",
            "notes.txt",
            ".DS_Store",
            ".a.mp4.part",
            "a-compressed.mp4",
        ] {
            touch(&dir.join(name));
        }
        std::fs::create_dir(dir.join("sub.mp4")).expect("subdir");

        let found = discover(std::slice::from_ref(&dir));
        assert_eq!(found.files, vec![dir.join("a.mp4"), dir.join("b.MOV")]);
        assert!(found.skipped.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explicit_paths_explain_rejection_and_dedupe() {
        let dir = scratch("explicit");
        let clip = dir.join("clip.mkv");
        let text = dir.join("readme.txt");
        touch(&clip);
        touch(&text);

        let found = discover(&[
            clip.clone(),
            dir.clone(),
            text.clone(),
            dir.join("gone.mp4"),
        ]);
        assert_eq!(found.files, vec![clip]);
        assert_eq!(found.skipped.len(), 2);
        assert_eq!(found.skipped[0].0, text);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
