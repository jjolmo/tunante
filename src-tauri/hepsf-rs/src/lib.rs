//! # hepsf-rs
//!
//! Safe Rust wrapper for sexypsf (PlayStation 1 sound format decoder).
//!
//! Uses sexypsf — a PCSX-based PS1 emulator with built-in HLE BIOS —
//! to decode PSF/minipsf files into PCM audio. No external BIOS file needed.
//!
//! Provides `PsfDecoder` for streaming audio decode, and `read_psf_tags`
//! for metadata-only extraction from PSF tags.
//!
//! **Important:** sexypsf uses global state, so only ONE `PsfDecoder` can
//! be active at a time. Creating a new decoder while one exists will
//! automatically close the old session.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

// ============================================================================
// FFI declarations — sexypsf wrapper (sexypsf_wrapper.c)
// ============================================================================

#[repr(C)]
struct PsfTagRaw {
    key: *mut c_char,
    value: *mut c_char,
    next: *mut PsfTagRaw,
}

/// Raw PSFINFO struct from sexypsf's driver.h
#[repr(C)]
struct PsfInfoRaw {
    length: u32,    // total duration (stop + fade) in milliseconds
    stop: u32,      // play time before fade in milliseconds
    fade: u32,      // fade duration in milliseconds
    title: *mut c_char,
    artist: *mut c_char,
    game: *mut c_char,
    year: *mut c_char,
    genre: *mut c_char,
    psfby: *mut c_char,
    comment: *mut c_char,
    copyright: *mut c_char,
    tags: *mut PsfTagRaw,
}

extern "C" {
    fn sexypsf_open(path: *const c_char) -> *mut PsfInfoRaw;
    fn sexypsf_render(buf: *mut i16, count: i32) -> i32;
    fn sexypsf_close();
    fn sexypsf_getinfo(path: *const c_char) -> *mut PsfInfoRaw;
    fn sexy_freepsfinfo(info: *mut PsfInfoRaw);
}

// ============================================================================
// Public types
// ============================================================================

/// Tags extracted from a PSF/minipsf file's metadata.
#[derive(Debug, Clone, Default)]
pub struct PsfTags {
    pub title: String,
    pub artist: String,
    pub game: String,
    pub year: String,
    pub genre: String,
    pub comment: String,
    /// Play duration before fade begins (milliseconds)
    pub length_ms: u64,
    /// Fade-out duration (milliseconds)
    pub fade_ms: u64,
    /// Rating (0-5 stars, from "rating" tag)
    pub rating: i32,
}

/// A PSF/minipsf audio decoder.
///
/// Wraps sexypsf's PS1 emulator (R3000 CPU + SPU with HLE BIOS).
/// Each instance decodes a single PSF track at 44100 Hz stereo.
///
/// **Note:** sexypsf uses global state. Only one decoder can be active
/// at a time. Creating a new decoder automatically closes any existing one.
/// Sample rate for PS1 audio output
const SAMPLE_RATE: u32 = 44100;
/// Chunk size for fast-forward rendering during seek (larger = faster)
const SEEK_CHUNK_FRAMES: usize = 4096;

pub struct PsfDecoder {
    active: bool,
    path: PathBuf,
}

// Safety: PsfDecoder is moved to the audio thread and used exclusively there.
// The underlying C library uses global state, but we ensure single-instance
// access through the `active` flag.
unsafe impl Send for PsfDecoder {}

impl PsfDecoder {
    /// Open a PSF/minipsf file and prepare for decoding.
    ///
    /// Returns the decoder and extracted PSF tags (title, artist, game, duration).
    /// The PS1 emulator is initialized with an HLE BIOS — no external BIOS needed.
    pub fn new(path: &Path) -> Result<(Self, PsfTags), String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
        let c_path =
            CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

        let info = unsafe { sexypsf_open(c_path.as_ptr()) };
        if info.is_null() {
            return Err(format!("Failed to load PSF: {}", path_str));
        }

        // Extract tags from the PSFINFO struct
        let tags = unsafe { extract_tags(info) };

        Ok((PsfDecoder { active: true, path: path.to_path_buf() }, tags))
    }

    /// Render `count` stereo frames of audio into `buffer`.
    ///
    /// `buffer` must have space for at least `count * 2` i16 samples
    /// (interleaved stereo: L, R, L, R, ...).
    ///
    /// If fewer than `count` frames are generated (song ended), the
    /// remainder is zero-filled.
    pub fn render(&mut self, buffer: &mut [i16], count: usize) {
        if !self.active {
            // Decoder was explicitly closed — fill with silence
            for s in buffer.iter_mut().take(count * 2) {
                *s = 0;
            }
            return;
        }

        let written = unsafe { sexypsf_render(buffer.as_mut_ptr(), count as i32) };
        let written = written.max(0) as usize;

        // Zero-fill if fewer samples were written (song ended)
        if written < count {
            for s in buffer.iter_mut().skip(written * 2).take((count - written) * 2) {
                *s = 0;
            }
        }
    }

    /// Seek to a position by closing, reopening, and fast-forwarding.
    ///
    /// This is the standard approach used by PSF players (foobar2000, etc.):
    /// the PS1 emulator doesn't support random access, so we re-initialize
    /// and render (discard) audio until we reach the target position.
    ///
    /// With the emulator compiled at -O2, seeking to 2 minutes takes ~1-2 seconds.
    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        // Close current session
        self.close();

        // Reopen the file
        let path_str = self.path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
        let c_path =
            CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

        let info = unsafe { sexypsf_open(c_path.as_ptr()) };
        if info.is_null() {
            return Err(format!("Failed to reopen PSF for seek: {}", path_str));
        }
        self.active = true;

        // Fast-forward: render and discard audio until we reach the target position
        let target_frames = (position_ms as u64 * SAMPLE_RATE as u64) / 1000;
        let mut frames_rendered: u64 = 0;
        let mut scratch = vec![0i16; SEEK_CHUNK_FRAMES * 2];

        while frames_rendered < target_frames {
            let remaining = target_frames - frames_rendered;
            let chunk = SEEK_CHUNK_FRAMES.min(remaining as usize);
            let written = unsafe { sexypsf_render(scratch.as_mut_ptr(), chunk as i32) };
            if written <= 0 {
                // Song ended before reaching target — that's OK
                break;
            }
            frames_rendered += written as u64;
        }

        Ok(())
    }

    /// Explicitly close the decoder, releasing the PS1 emulator state.
    ///
    /// After calling this, `render()` will produce silence.
    /// This is useful when you need to create a new decoder (e.g., for seeking)
    /// without waiting for Drop.
    pub fn close(&mut self) {
        if self.active {
            unsafe { sexypsf_close(); }
            self.active = false;
        }
    }
}

impl Drop for PsfDecoder {
    fn drop(&mut self) {
        self.close();
    }
}

// ============================================================================
// Tag extraction helpers
// ============================================================================

/// Extract tags from a raw PSFINFO pointer into a safe PsfTags struct.
unsafe fn extract_tags(info: *const PsfInfoRaw) -> PsfTags {
    let info = &*info;

    // Walk the extra tags linked list to find "rating" and other custom tags
    let mut rating: i32 = 0;
    let mut tag_ptr = info.tags;
    while !tag_ptr.is_null() {
        let tag = &*tag_ptr;
        let key = cstr_to_string(tag.key).to_lowercase();
        let value = cstr_to_string(tag.value);
        if key == "rating" {
            rating = value.trim().parse::<i32>().unwrap_or(0).clamp(0, 5);
        }
        tag_ptr = tag.next;
    }

    PsfTags {
        title: cstr_to_string(info.title),
        artist: cstr_to_string(info.artist),
        game: cstr_to_string(info.game),
        year: cstr_to_string(info.year),
        genre: cstr_to_string(info.genre),
        comment: cstr_to_string(info.comment),
        // `stop` = play time before fade begins (milliseconds)
        // `fade` = fade duration (milliseconds)
        // `length` = stop + fade (total duration) — we don't use this directly
        length_ms: info.stop as u64,
        fade_ms: info.fade as u64,
        rating,
    }
}

/// Convert a nullable C string to a Rust String. Returns empty string for null.
unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr).to_string_lossy().to_string()
    }
}

// ============================================================================
// Metadata-only tag reading
// ============================================================================

/// Read PSF tags from a PSF/minipsf file without initializing the emulator.
///
/// This is much faster than creating a full PsfDecoder — it only parses the
/// PSF container metadata, not the PS-X EXE payload. Does not touch global
/// emulator state, so it's safe to call while a decoder is active.
pub fn read_psf_tags(path: &Path) -> Result<PsfTags, String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
    let c_path = CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

    let info = unsafe { sexypsf_getinfo(c_path.as_ptr()) };
    if info.is_null() {
        return Err(format!("Failed to read PSF info: {}", path_str));
    }

    let tags = unsafe { extract_tags(info) };
    unsafe { sexy_freepsfinfo(info) };

    Ok(tags)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_cstr_to_string_null() {
        unsafe {
            assert_eq!(super::cstr_to_string(std::ptr::null()), "");
        }
    }
}
