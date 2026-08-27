//! Finding cover art on disk.
//!
//! The other half — art embedded in the file's own tags — is [`crate::artwork`],
//! which asks the decoder helper. This is the fallback for the very common case
//! of a rip that carries no tag art but has a `cover.jpg` sitting beside it.
//!
//! Moved out of `tunante-mini/src/library.rs` so `tunante-android` gets the same
//! answers rather than a second implementation that drifts.

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
