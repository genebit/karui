//! Camera cards and other removable storage.
//!
//! A card is compressed where it sits: ffmpeg reads straight from it and the
//! output goes to a folder on this computer, so there is no copy step and
//! nothing is ever written to the card. A card is recognised by the folder
//! layouts cameras use, not by its name, which is usually `Untitled` or
//! `NO NAME`.

use crate::discover::is_video;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Folders that mark a camera card, relative to the volume root. `DCIM` is
/// the DCF standard nearly every camera, phone, drone, and action camera
/// follows. Sony's XAVC and the AVCHD format keep video under `PRIVATE`
/// instead and leave `DCIM` for stills.
const CAMERA_DIRS: &[&str] = &["DCIM", "PRIVATE/M4ROOT/CLIP", "PRIVATE/AVCHD/BDMV/STREAM"];

/// Deep enough for `DCIM/100CANON` and `PRIVATE/AVCHD/BDMV/STREAM`, shallow
/// enough that a mis-detected disk is not walked end to end.
const MAX_DEPTH: usize = 4;

/// Mirrored by `Card` in `src/lib/bindings.ts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub root: PathBuf,
    /// The volume's label, as the file manager shows it.
    pub name: String,
}

/// One video on a card, identified well enough to know it again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardVideo {
    pub path: PathBuf,
    pub bytes: u64,
    /// Seconds since the epoch, as the camera stamped it.
    pub modified: u64,
}

impl CardVideo {
    fn read(path: PathBuf) -> Option<CardVideo> {
        let meta = std::fs::metadata(&path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        Some(CardVideo {
            path,
            bytes: meta.len(),
            modified,
        })
    }

    /// Name, size, and time together. Cameras restart numbering on a fresh
    /// card, so `MVI_0001.MP4` alone would mistake a new clip for an old one.
    fn fingerprint(&self) -> String {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{name}|{}|{}", self.bytes, self.modified)
    }
}

/// A card and what is on it. Mirrored by `CardSummary` in
/// `src/lib/bindings.ts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSummary {
    #[serde(flatten)]
    pub card: Card,
    pub videos: usize,
    pub bytes: u64,
    /// Videos not imported before, in recording order.
    pub fresh: Vec<PathBuf>,
    pub fresh_bytes: u64,
}

/// Read a card's videos and sort out which are new to `ledger`.
pub fn summarise(card: Card, ledger: &Ledger) -> CardSummary {
    let all = videos(&card.root);
    let (fresh, _): (Vec<&CardVideo>, Vec<&CardVideo>) =
        all.iter().partition(|video| !ledger.contains(video));
    CardSummary {
        videos: all.len(),
        bytes: all.iter().map(|v| v.bytes).sum(),
        fresh_bytes: fresh.iter().map(|v| v.bytes).sum(),
        fresh: fresh.into_iter().map(|v| v.path.clone()).collect(),
        card,
    }
}

/// Every mounted camera card. Cheap enough to poll: a directory listing and
/// a few `stat`s per volume.
pub fn cards() -> Vec<Card> {
    let mut cards: Vec<Card> = mount_points()
        .into_iter()
        .filter(|root| is_card_root(root))
        .map(|root| Card {
            name: volume_name(&root),
            root,
        })
        .collect();
    cards.sort_by(|a, b| a.root.cmp(&b.root));
    cards
}

/// The card `path` is on, if it is on one.
pub fn card_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|a| is_mount_root(a) && is_card_root(a))
        .map(Path::to_path_buf)
}

/// Every video under the card's camera folders, sorted by path so they
/// compress in recording order. Hidden entries are skipped: macOS litters
/// cards with `._name.mp4` copies and permission-locked `.Trashes`.
pub fn videos(root: &Path) -> Vec<CardVideo> {
    let mut found = Vec::new();
    for dir in CAMERA_DIRS {
        walk(&root.join(dir), 0, &mut found);
    }
    found.sort();
    found.dedup();
    found.into_iter().filter_map(CardVideo::read).collect()
}

/// Every video under `dir`, however deep, within the same limits as a card.
/// For a folder on a card the user picked by hand.
pub fn videos_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(dir, 0, &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if hidden {
            continue;
        }
        if kind.is_dir() && depth < MAX_DEPTH {
            walk(&path, depth + 1, found);
        } else if kind.is_file() && is_video(&path) {
            found.push(path);
        }
    }
}

/// Where card videos go when no output folder is set: a `karui` folder in
/// the user's videos folder. Never beside the source, which would write
/// gigabytes back onto the card.
pub fn default_import_dir() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let videos = if cfg!(target_os = "macos") {
        "Movies"
    } else {
        "Videos"
    };
    Some(PathBuf::from(home).join(videos).join("karui"))
}

/// Which card videos have been imported before.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ledger {
    imported: BTreeSet<String>,
}

impl Ledger {
    /// A missing or unreadable ledger starts empty; the cost is offering old
    /// clips again.
    pub fn load(path: &Path) -> Ledger {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        let partial = path.with_extension("json.part");
        std::fs::write(&partial, json)?;
        if cfg!(windows) && path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(partial, path)
    }

    pub fn contains(&self, video: &CardVideo) -> bool {
        self.imported.contains(&video.fingerprint())
    }

    /// Remember `path` as imported. `false` if it could not be read, for
    /// instance because the card was pulled the moment the encode finished.
    pub fn record(&mut self, path: &Path) -> bool {
        let Some(video) = CardVideo::read(path.to_path_buf()) else {
            return false;
        };
        self.imported.insert(video.fingerprint());
        true
    }
}

/// The folders removable volumes are mounted in.
fn mount_dirs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Volumes")]
    } else if cfg!(windows) {
        Vec::new()
    } else {
        // udisks mounts under `/run/media/$USER` (Fedora, Arch) or
        // `/media/$USER` (Debian, Ubuntu); older systems use `/media` itself.
        let user = std::env::var_os("USER").unwrap_or_default();
        vec![
            Path::new("/run/media").join(&user),
            Path::new("/media").join(&user),
            PathBuf::from("/media"),
        ]
    }
}

fn mount_points() -> Vec<PathBuf> {
    if cfg!(windows) {
        // A and B are floppy letters and C is the system drive.
        return ('D'..='Z')
            .map(|letter| PathBuf::from(format!("{letter}:\\")))
            .filter(|root| root.is_dir())
            .collect();
    }
    mount_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        // `/Volumes/Macintosh HD` is a symlink to `/`.
        .filter(|path| !path.is_symlink() && path.is_dir())
        .collect()
}

fn is_mount_root(path: &Path) -> bool {
    if cfg!(windows) {
        return path.parent().is_none() && path.has_root();
    }
    path.parent()
        .is_some_and(|parent| mount_dirs().iter().any(|dir| dir == parent))
}

fn is_card_root(root: &Path) -> bool {
    CAMERA_DIRS.iter().any(|dir| root.join(dir).is_dir())
}

fn volume_name(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("karui-dev-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn touch(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, vec![0u8; bytes]).expect("write");
    }

    #[test]
    fn finds_videos_in_every_camera_layout_and_skips_the_rest() {
        let card = scratch("layouts");
        touch(&card.join("DCIM/Camera01/VID_2.mp4"), 2);
        touch(&card.join("DCIM/Camera01/VID_1.mp4"), 1);
        touch(&card.join("DCIM/100CANON/MVI_0001.MOV"), 3);
        touch(&card.join("DCIM/100CANON/IMG_0001.JPG"), 1);
        touch(&card.join("DCIM/Camera01/._VID_1.mp4"), 1);
        touch(&card.join("PRIVATE/M4ROOT/CLIP/C0001.MP4"), 4);
        touch(&card.join("PRIVATE/AVCHD/BDMV/STREAM/00000.MTS"), 5);
        touch(&card.join("MISC/elsewhere.mp4"), 1);

        let names: Vec<String> = videos(&card)
            .iter()
            .map(|v| {
                v.path
                    .strip_prefix(&card)
                    .expect("on card")
                    .display()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            [
                "DCIM/100CANON/MVI_0001.MOV",
                "DCIM/Camera01/VID_1.mp4",
                "DCIM/Camera01/VID_2.mp4",
                "PRIVATE/AVCHD/BDMV/STREAM/00000.MTS",
                "PRIVATE/M4ROOT/CLIP/C0001.MP4",
            ]
            .map(|n| n.replace('/', std::path::MAIN_SEPARATOR_STR))
        );
        assert!(is_card_root(&card));
        let _ = std::fs::remove_dir_all(&card);
    }

    #[test]
    fn a_folder_with_dcim_off_a_mount_is_not_a_card() {
        let copy = scratch("copied");
        touch(&copy.join("DCIM/Camera01/VID_1.mp4"), 1);
        // A card's folders copied to the home folder are ordinary files:
        // their outputs go beside them as usual.
        assert_eq!(card_of(&copy.join("DCIM/Camera01/VID_1.mp4")), None);
        let _ = std::fs::remove_dir_all(&copy);
    }

    #[test]
    fn ledger_knows_a_clip_by_name_size_and_time() {
        let card = scratch("ledger");
        let clip = card.join("DCIM/100CANON/MVI_0001.MOV");
        touch(&clip, 10);
        let mut ledger = Ledger::default();
        let video = videos(&card).remove(0);
        assert!(!ledger.contains(&video));
        assert!(ledger.record(&clip));
        assert!(ledger.contains(&video));

        // The next card restarts at MVI_0001, with different footage.
        touch(&clip, 11);
        assert!(!ledger.contains(&videos(&card).remove(0)));

        let path = card.join("ledger.json");
        ledger.save(&path).expect("save");
        assert_eq!(Ledger::load(&path), ledger);
        assert!(!ledger.record(&card.join("gone.mp4")));
        let _ = std::fs::remove_dir_all(&card);
    }

    #[test]
    fn summary_counts_only_new_clips_as_fresh() {
        let root = scratch("summary");
        touch(&root.join("DCIM/100GOPRO/GX010001.MP4"), 100);
        touch(&root.join("DCIM/100GOPRO/GX010002.MP4"), 50);
        let mut ledger = Ledger::default();
        ledger.record(&root.join("DCIM/100GOPRO/GX010001.MP4"));
        let card = Card {
            root: root.clone(),
            name: "GOPRO".into(),
        };

        let summary = summarise(card, &ledger);
        assert_eq!((summary.videos, summary.bytes), (2, 150));
        assert_eq!(summary.fresh, vec![root.join("DCIM/100GOPRO/GX010002.MP4")]);
        assert_eq!(summary.fresh_bytes, 50);
        let json = serde_json::to_value(&summary).expect("json");
        assert_eq!(json["name"], "GOPRO");
        assert_eq!(json["freshBytes"], 50);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_dir_is_in_the_videos_folder() {
        let dir = default_import_dir().expect("home");
        assert!(dir.ends_with(if cfg!(target_os = "macos") {
            "Movies/karui"
        } else {
            "Videos/karui"
        }));
    }
}
