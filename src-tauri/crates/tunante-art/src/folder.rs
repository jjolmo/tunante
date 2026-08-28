//! Cover art on disk: finding what is already there, and adding to it.
//!
//! [`folder_image`] moved here from `tunante-helper`, which is where it moved
//! to from `tunante-mini`. It has now been needed by four things, so it lives
//! at the bottom.
//!
//! [`store_cover`] is the only function in this crate that writes anywhere the
//! user would notice. Read its docs before changing it.

use crate::image::ImageInfo;
use std::path::{Path, PathBuf};

/// The image that best represents `dir`, if any.
pub fn folder_image(dir: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "art", "thumb"];
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp"];

    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();

    let is_image = |p: &Path| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
    };

    // By name first, case-insensitively: `cover.jpg` and `Cover.jpg` coexist in
    // this library, and ext4 does not consider them the same file.
    for n in NAMES {
        if let Some(p) = entries.iter().find(|p| {
            is_image(p)
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(n))
        }) {
            return Some(p.clone());
        }
    }

    // Otherwise the first image there is. Sorted, so the same folder always
    // gives the same one and a grid does not change appearance between visits.
    let mut loose: Vec<&PathBuf> = entries.iter().filter(|p| is_image(p)).collect();
    loose.sort();
    loose.first().map(|p| (*p).clone())
}

/// Whether an existing folder image may be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Leave any image already in the folder alone.
    Never,
    /// Replace it. Only ever from an explicit "re-download and replace".
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    /// Written. The path is the new file.
    Written(PathBuf),
    /// The folder already had an image and [`Overwrite::Never`] was asked for.
    Kept(PathBuf),
}

/// Write a cover into a game's folder.
///
/// This is the only irreversible thing in the crate: the folder belongs to the
/// user, is very likely inside a sync client, and a wrong cover has to be
/// deleted by hand on every device it reached. Four rules, each of which exists
/// because the obvious version is wrong:
///
/// 1. **Existing art is found with [`folder_image`], not by testing for
///    `cover.jpg`.** The old code only checked that one name, so a folder with
///    a `front.png` the user had chosen got a `cover.jpg` written beside it —
///    and since `cover` sorts first in `NAMES`, the download then *won* over the
///    user's own file. It looked like nothing was overwritten. Something was.
/// 2. **The extension follows the bytes.** Libretro serves PNG; writing it to
///    `cover.jpg` made the name a lie for no gain, since `folder_image` reads
///    both and every consumer sniffs.
/// 3. **Write to a temporary in the same directory, then rename.** Sync clients
///    watch with inotify and will happily upload a half-written file; and on
///    Android's `/sdcard` a rename across directories is not atomic.
/// 4. **The caller records what was written**, so a bulk run can be undone.
///    See [`Manifest`].
pub fn store_cover(
    dir: &Path,
    bytes: &[u8],
    info: &ImageInfo,
    overwrite: Overwrite,
) -> std::io::Result<Stored> {
    if let Some(existing) = folder_image(dir) {
        if overwrite == Overwrite::Never {
            return Ok(Stored::Kept(existing));
        }
    }

    let target = dir.join(format!("cover.{}", info.format.extension()));
    let tmp = dir.join(format!(".tunante-cover-{}.part", std::process::id()));

    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, &target) {
        Ok(()) => Ok(Stored::Written(target)),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// A record of what one bulk run created, so it can be taken back.
///
/// Thirty lines, and it is the difference between a button people press and a
/// button they do not: without it, "download covers for my whole library" is an
/// irreversible action over someone's file collection.
pub struct Manifest {
    path: PathBuf,
}

impl Manifest {
    /// `stamp` is a caller-supplied unix timestamp — this crate takes no clock
    /// of its own so its behaviour stays reproducible in tests.
    pub fn new(cache_root: &Path, stamp: u64) -> std::io::Result<Self> {
        let dir = cache_root.join("cover-runs");
        std::fs::create_dir_all(&dir)?;
        Ok(Self { path: dir.join(format!("{stamp}.txt")) })
    }

    pub fn record(&self, written: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(f, "{}", written.display())
    }

    /// Delete exactly the files this run created, and nothing else.
    pub fn undo(cache_root: &Path, stamp: u64) -> std::io::Result<usize> {
        let path = cache_root.join("cover-runs").join(format!("{stamp}.txt"));
        let text = std::fs::read_to_string(&path)?;
        let mut n = 0;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if std::fs::remove_file(line).is_ok() {
                n += 1;
            }
        }
        let _ = std::fs::remove_file(&path);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Format, ImageInfo};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmpdir(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "tunante-art-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn info(format: Format) -> ImageInfo {
        ImageInfo { format, width: 600, height: 600, bytes: 4 }
    }

    #[test]
    fn a_cover_lands_in_the_folder() {
        let d = tmpdir("store");
        let got = store_cover(&d, b"data", &info(Format::Jpeg), Overwrite::Never).unwrap();
        assert_eq!(got, Stored::Written(d.join("cover.jpg")));
        assert_eq!(std::fs::read(d.join("cover.jpg")).unwrap(), b"data");
    }

    /// PNG bytes get a PNG name. `folder_image` reads both, so nothing downstream
    /// has to care — but the folder no longer lies about what is in it.
    #[test]
    fn png_bytes_are_not_written_as_a_jpg() {
        let d = tmpdir("store-png");
        let got = store_cover(&d, b"data", &info(Format::Png), Overwrite::Never).unwrap();
        assert_eq!(got, Stored::Written(d.join("cover.png")));
        assert_eq!(folder_image(&d), Some(d.join("cover.png")));
    }

    /// The regression this function was rewritten for. A user's `front.png` was
    /// not overwritten — a `cover.jpg` was written next to it, and then won,
    /// because `cover` comes first in the name list.
    #[test]
    fn an_image_the_user_chose_is_not_quietly_outranked() {
        let d = tmpdir("store-existing");
        std::fs::write(d.join("front.png"), b"the user's choice").unwrap();

        let got = store_cover(&d, b"downloaded", &info(Format::Jpeg), Overwrite::Never).unwrap();
        assert_eq!(got, Stored::Kept(d.join("front.png")));
        assert!(!d.join("cover.jpg").exists(), "wrote alongside the user's image");
        assert_eq!(folder_image(&d), Some(d.join("front.png")));
    }

    #[test]
    fn replacing_is_possible_when_asked_for_explicitly() {
        let d = tmpdir("store-replace");
        std::fs::write(d.join("cover.jpg"), b"old").unwrap();
        let got = store_cover(&d, b"new", &info(Format::Jpeg), Overwrite::Replace).unwrap();
        assert_eq!(got, Stored::Written(d.join("cover.jpg")));
        assert_eq!(std::fs::read(d.join("cover.jpg")).unwrap(), b"new");
    }

    /// Nothing partially-written may be left behind for a sync client to upload.
    #[test]
    fn no_temporary_file_survives() {
        let d = tmpdir("store-tmp");
        store_cover(&d, b"data", &info(Format::Jpeg), Overwrite::Never).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("part") || n.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?} behind");
    }

    #[test]
    fn a_run_can_be_taken_back() {
        let cache = tmpdir("manifest");
        let lib = tmpdir("manifest-lib");
        let a = lib.join("cover.jpg");
        let b = lib.join("other.png");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"y").unwrap();
        let untouched = lib.join("not-ours.jpg");
        std::fs::write(&untouched, b"z").unwrap();

        let m = Manifest::new(&cache, 1234).unwrap();
        m.record(&a).unwrap();
        m.record(&b).unwrap();

        assert_eq!(Manifest::undo(&cache, 1234).unwrap(), 2);
        assert!(!a.exists() && !b.exists());
        assert!(untouched.exists(), "undo deleted something it did not write");
    }

    // --- folder_image ---

    #[test]
    fn a_named_cover_beats_a_loose_image() {
        let d = tmpdir("find-named");
        std::fs::write(d.join("aaa-screenshot.png"), b"x").unwrap();
        std::fs::write(d.join("cover.jpg"), b"x").unwrap();
        assert_eq!(folder_image(&d), Some(d.join("cover.jpg")));
    }

    /// `cover.jpg` and `Cover.jpg` are different files on ext4, and this library
    /// contains both spellings.
    #[test]
    fn the_name_match_ignores_case() {
        let d = tmpdir("find-case");
        std::fs::write(d.join("Cover.JPG"), b"x").unwrap();
        assert_eq!(folder_image(&d), Some(d.join("Cover.JPG")));
    }

    /// Sorted, so a grid does not shuffle between visits.
    #[test]
    fn the_loose_fallback_is_deterministic() {
        let d = tmpdir("find-loose");
        for n in ["zzz.png", "aaa.png", "mmm.png"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        assert_eq!(folder_image(&d), Some(d.join("aaa.png")));
    }

    #[test]
    fn a_folder_with_no_images_has_none() {
        let d = tmpdir("find-empty");
        std::fs::write(d.join("track.mp3"), b"x").unwrap();
        assert_eq!(folder_image(&d), None);
        assert_eq!(folder_image(Path::new("/nonexistent-jkhsdf")), None);
    }
}
