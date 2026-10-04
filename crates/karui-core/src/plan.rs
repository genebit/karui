//! Where each output goes.
//!
//! Decided for the whole batch up front, so two inputs can never be given
//! the same output and no output can land on an input. The original script
//! wrote `<folder>/../Output/<same name>`: re-running it made ffmpeg stop and
//! wait on an `Overwrite? [y/N]` prompt nobody could see.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Appended to the stem of an output written beside its source.
pub const OUTPUT_SUFFIX: &str = "-compressed";

/// Every output is MP4: it is the one container every player and site takes.
pub const OUTPUT_EXTENSION: &str = "mp4";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub input: PathBuf,
    pub output: PathBuf,
}

/// The working file ffmpeg writes to. Renamed to `output` only on success, so
/// an interrupted encode never leaves a truncated file under the final name.
/// Hidden, and skipped by `discover`.
pub fn partial_path(output: &Path) -> PathBuf {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    output.with_file_name(format!(".{name}.part"))
}

/// How paths are compared for collisions.
///
/// Case-folded on every platform: macOS and Windows file systems are
/// case-insensitive by default, and on Linux the cost of folding is at worst
/// an unnecessary ` (2)`. The parent is canonicalised when it exists so
/// `./a.mp4` and `/abs/a.mp4` collide as they should.
fn identity(path: &Path) -> String {
    let resolved = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => std::fs::canonicalize(parent)
            .map(|p| p.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    };
    resolved.to_string_lossy().to_lowercase()
}

pub fn plan(inputs: &[PathBuf], output_dir: Option<&Path>, overwrite: bool) -> Vec<Job> {
    let mut claimed: HashSet<String> = inputs.iter().map(|p| identity(p)).collect();

    inputs
        .iter()
        .map(|input| {
            let stem = input
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "video".into());
            let (dir, base) = match output_dir {
                Some(dir) => (dir.to_path_buf(), stem),
                None => (
                    input.parent().map(Path::to_path_buf).unwrap_or_default(),
                    format!("{stem}{OUTPUT_SUFFIX}"),
                ),
            };

            let mut n = 1;
            let output = loop {
                let name = if n == 1 {
                    format!("{base}.{OUTPUT_EXTENSION}")
                } else {
                    format!("{base} ({n}).{OUTPUT_EXTENSION}")
                };
                let candidate = dir.join(name);
                let taken =
                    claimed.contains(&identity(&candidate)) || (!overwrite && candidate.exists());
                if !taken {
                    break candidate;
                }
                n += 1;
            };

            claimed.insert(identity(&output));
            Job {
                input: input.clone(),
                output,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("karui-plan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn beside_source_with_suffix() {
        let jobs = plan(&[PathBuf::from("/v/trip.mov")], None, false);
        assert_eq!(jobs[0].output, PathBuf::from("/v/trip-compressed.mp4"));
    }

    #[test]
    fn same_stem_in_one_batch_is_numbered() {
        let out = PathBuf::from("/nonexistent-karui-out");
        let jobs = plan(
            &[
                PathBuf::from("/v/a.mov"),
                PathBuf::from("/v/a.mkv"),
                PathBuf::from("/w/A.mp4"),
            ],
            Some(&out),
            true,
        );
        let names: Vec<_> = jobs.iter().map(|j| j.output.clone()).collect();
        assert_eq!(
            names,
            vec![
                out.join("a.mp4"),
                out.join("a (2).mp4"),
                out.join("A (3).mp4")
            ]
        );
    }

    #[test]
    fn never_writes_over_an_input() {
        let dir = scratch("input");
        let input = dir.join("clip.mp4");
        std::fs::write(&input, b"").expect("touch");
        // Overwrite on, output folder = input folder: the obvious target is
        // the input itself.
        let jobs = plan(std::slice::from_ref(&input), Some(&dir), true);
        assert_eq!(jobs[0].output, dir.join("clip (2).mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_files_are_kept_unless_overwriting() {
        let dir = scratch("existing");
        std::fs::write(dir.join("clip-compressed.mp4"), b"").expect("touch");
        let input = dir.join("clip.mov");

        let kept = plan(std::slice::from_ref(&input), None, false);
        assert_eq!(kept[0].output, dir.join("clip-compressed (2).mp4"));

        let replaced = plan(&[input], None, true);
        assert_eq!(replaced[0].output, dir.join("clip-compressed.mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_is_hidden_sibling() {
        assert_eq!(
            partial_path(Path::new("/o/clip.mp4")),
            PathBuf::from("/o/.clip.mp4.part")
        );
    }
}
