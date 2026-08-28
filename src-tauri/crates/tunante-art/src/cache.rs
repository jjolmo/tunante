//! Where downloaded covers and parsed archive indexes are kept.
//!
//! # Why this is not `AppHandle::app_data_dir()`
//!
//! That is how the desktop app used to find `covers/`, and it is why neither
//! `tunante-mini` nor the Android app could cache anything: there is no Tauri
//! handle on a phone. The resolver here is the same shape as
//! `tunante_helper::decoder_path` — an explicit setter, then an environment
//! variable, then the platform's own convention — so there is only one idiom in
//! the repository for "where does this thing live".
//!
//! **Android must call [`set_cache_dir`]**, with `context.cacheDir`. There is no
//! other way to learn it, and the last fallback resolves to a `/tmp` that does
//! not exist there.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// A miss is remembered for this long, and no longer.
///
/// It used to be remembered forever, which is a real bug rather than a
/// conservative choice: the Libretro archive gains covers continuously, so a
/// game looked up the week before its box art was added could never be found
/// again on that machine. Users experience that as "this feature doesn't work
/// for this game", permanently.
pub const MISS_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Point the cache somewhere explicit. First call wins; later ones are ignored.
pub fn set_cache_dir(path: impl Into<PathBuf>) -> bool {
    CACHE_DIR.set(path.into()).is_ok()
}

/// The cache root, created if it is missing.
pub fn cache_dir() -> PathBuf {
    let dir = CACHE_DIR.get().cloned().unwrap_or_else(resolve);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        // Loud, because the most likely cause is Android never calling
        // `set_cache_dir`, and the symptom of that is silence.
        log::error!("cover cache unusable at {}: {e}", dir.display());
    }
    dir
}

fn resolve() -> PathBuf {
    if let Some(v) = std::env::var_os("TUNANTE_CACHE_DIR") {
        return PathBuf::from(v);
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Library/Caches/tunante");
    }
    #[cfg(target_os = "windows")]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("tunante").join("cache");
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("tunante");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("tunante");
    }
    std::env::temp_dir().join("tunante-cache")
}

/// What a cached cover is a cover *of*. Keeps game lookups and plain album
/// lookups in separate namespaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Game,
    Album,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Game => "game",
            Kind::Album => "album",
        }
    }
}

/// The cache key for a lookup.
///
/// One scheme, where there used to be two incompatible ones sharing a directory
/// — `sha256(album\0artist)` with a `.jpg` suffix, and `sha256("vgm3\0"…)` with
/// `.img`. The `art1` prefix invalidates both, which is wanted: the matching
/// algorithm changed underneath them.
///
/// `name` must already be normalised. Keying on the raw string, as the old
/// scheme did, filed `Lufia II` and `lufia 2` separately and made each pay its
/// own full lookup.
pub fn key(kind: Kind, normalized_name: &str, system: &str) -> String {
    let input = format!("art1\0{}\0{}\0{}", kind.as_str(), normalized_name, system.to_lowercase());
    format!("{:x}", Sha256::digest(input.as_bytes()))[..16].to_string()
}

fn covers_dir() -> PathBuf {
    let d = cache_dir().join("covers");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The bytes cached under this key, if any.
pub fn get(key: &str) -> Option<Vec<u8>> {
    std::fs::read(covers_dir().join(format!("{key}.img"))).ok()
}

pub fn put(key: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(covers_dir().join(format!("{key}.img")), bytes)
}

/// Has this lookup already failed recently enough to skip?
pub fn is_fresh_miss(key: &str) -> bool {
    let path = covers_dir().join(format!("{key}.miss"));
    let Ok(text) = std::fs::read_to_string(&path) else { return false };
    let Ok(when) = text.trim().parse::<u64>() else {
        // Written by a build that stored an empty file. Treat as expired rather
        // than as "never again".
        let _ = std::fs::remove_file(&path);
        return false;
    };
    match now_secs().checked_sub(when) {
        Some(age) => age < MISS_TTL.as_secs(),
        // A clock that went backwards is not a reason to keep a miss.
        None => false,
    }
}

pub fn record_miss(key: &str) {
    let _ = std::fs::write(
        covers_dir().join(format!("{key}.miss")),
        now_secs().to_string(),
    );
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Empty the whole cover cache. Returns how many files went.
pub fn clear() -> std::io::Result<u32> {
    let mut n = 0;
    let dir = covers_dir();
    if !dir.exists() {
        return Ok(0);
    }
    for entry in std::fs::read_dir(&dir)?.flatten() {
        if entry.path().is_file() && std::fs::remove_file(entry.path()).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// Also remove a directory the desktop app used before the cache moved here.
///
/// `<app_data>/covers` was always the wrong home for something disposable; this
/// exists so one release cleans up after the move instead of leaving a few
/// megabytes behind forever.
pub fn clear_legacy(dir: &Path) -> std::io::Result<u32> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut n = 0;
    for entry in std::fs::read_dir(dir)?.flatten() {
        if entry.path().is_file() && std::fs::remove_file(entry.path()).is_ok() {
            n += 1;
        }
    }
    let _ = std::fs::remove_dir(dir);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_namespaces_do_not_collide() {
        assert_ne!(key(Kind::Game, "celeste", ""), key(Kind::Album, "celeste", ""));
    }

    #[test]
    fn the_console_is_part_of_the_key() {
        assert_ne!(key(Kind::Game, "castlevania", "nes"), key(Kind::Game, "castlevania", "snes"));
    }

    #[test]
    fn a_key_is_short_and_stable() {
        let k = key(Kind::Game, "chrono trigger", "snes");
        assert_eq!(k.len(), 16);
        assert_eq!(k, key(Kind::Game, "chrono trigger", "snes"));
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// The system is lowercased on the way in, so a caller passing a display
    /// name and a caller passing an id do not create two entries.
    #[test]
    fn the_system_is_case_folded() {
        assert_eq!(key(Kind::Game, "x", "SNES"), key(Kind::Game, "x", "snes"));
    }

    #[test]
    fn an_empty_miss_file_from_an_older_build_is_not_permanent() {
        let tmp = std::env::temp_dir().join(format!("tunante-art-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("TUNANTE_CACHE_DIR", &tmp);
        // Only meaningful if this process has not already resolved the dir.
        if CACHE_DIR.get().is_some() {
            return;
        }
        let d = covers_dir();
        std::fs::write(d.join("deadbeef.miss"), b"").unwrap();
        assert!(!is_fresh_miss("deadbeef"), "an unparseable miss must expire");
        assert!(!d.join("deadbeef.miss").exists(), "and be cleaned up");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
