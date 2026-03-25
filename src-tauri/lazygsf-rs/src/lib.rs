//! # lazygsf-rs
//!
//! Safe Rust wrapper for lazygsf (GBA sound format decoder using mGBA)
//! and psflib (PSF container parser).
//!
//! Provides `GsfDecoder` for decoding GSF/minigsf files into PCM audio,
//! and `read_gsf_tags` for metadata-only extraction from PSF tags.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::path::Path;
use std::sync::Once;

// ============================================================================
// FFI declarations
// ============================================================================

// --- lazygsf ---
extern "C" {
    fn gsf_init();
    fn gsf_get_state_size() -> usize;
    fn gsf_clear(state: *mut c_void);
    fn gsf_set_sample_rate(state: *mut c_void, sample_rate: u32) -> u32;
    fn gsf_upload_section(state: *mut c_void, data: *const u8, size: usize) -> c_int;
    fn gsf_render(state: *mut c_void, buffer: *mut i16, count: usize) -> c_int;
    fn gsf_restart(state: *mut c_void);
    fn gsf_shutdown(state: *mut c_void);
}

// --- psflib ---

/// File I/O callbacks for psflib (mirrors psf_file_callbacks struct)
#[repr(C)]
struct PsfFileCallbacks {
    path_separators: *const c_char,
    context: *mut c_void,
    fopen: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    fread:
        Option<unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize>,
    fseek: Option<unsafe extern "C" fn(*mut c_void, i64, c_int) -> c_int>,
    fclose: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    ftell: Option<unsafe extern "C" fn(*mut c_void) -> c_long>,
}

/// psf_load_callback type
type PsfLoadCallback =
    Option<unsafe extern "C" fn(*mut c_void, *const u8, usize, *const u8, usize) -> c_int>;

/// psf_info_callback type
type PsfInfoCallback =
    Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int>;

/// psf_status_callback type
type PsfStatusCallback = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;

extern "C" {
    // Renamed via -Dpsf_load=gsf_psf_load in build.rs to avoid
    // symbol collision with hepsf-rs and vio2sf-rs psflib copies
    #[link_name = "gsf_psf_load"]
    fn psf_load(
        uri: *const c_char,
        file_callbacks: *const PsfFileCallbacks,
        allowed_version: u8,
        load_target: PsfLoadCallback,
        load_context: *mut c_void,
        info_target: PsfInfoCallback,
        info_context: *mut c_void,
        info_want_nested_tags: c_int,
        status_target: PsfStatusCallback,
        status_context: *mut c_void,
    ) -> c_int;
}

// ============================================================================
// psflib file I/O callbacks (using libc FILE* through Rust)
// ============================================================================

unsafe extern "C" fn psf_fopen(_context: *mut c_void, path: *const c_char) -> *mut c_void {
    let mode = b"rb\0".as_ptr() as *const c_char;

    // Try the exact path first
    let handle = libc::fopen(path, mode);
    if !handle.is_null() {
        return handle as *mut c_void;
    }

    // If not found and it's a .gsflib/.psflib/etc, search parent directories.
    // This handles the common case where minigsf files are in subfolders
    // but their referenced .lib file is in a parent directory.
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let p = std::path::Path::new(path_str);
    let filename = match p.file_name() {
        Some(f) => f,
        None => return std::ptr::null_mut(),
    };

    // Only search parents for library files (not the minigsf itself)
    let fname_str = filename.to_string_lossy();
    let is_lib = fname_str.ends_with("lib")
        || fname_str.ends_with(".gsflib")
        || fname_str.ends_with(".psflib")
        || fname_str.ends_with(".2sflib");

    if !is_lib {
        return std::ptr::null_mut();
    }

    // Walk up parent directories (max 5 levels)
    if let Some(mut dir) = p.parent() {
        for _ in 0..5 {
            dir = match dir.parent() {
                Some(d) => d,
                None => break,
            };
            let candidate = dir.join(filename);
            if candidate.exists() {
                let c_candidate = match CString::new(candidate.to_string_lossy().as_bytes()) {
                    Ok(c) => c,
                    Err(_) => break,
                };
                let h = libc::fopen(c_candidate.as_ptr(), mode);
                if !h.is_null() {
                    return h as *mut c_void;
                }
            }
        }
    }

    std::ptr::null_mut()
}

unsafe extern "C" fn psf_fread(
    buffer: *mut c_void,
    size: usize,
    count: usize,
    handle: *mut c_void,
) -> usize {
    libc::fread(buffer, size, count, handle as *mut libc::FILE)
}

unsafe extern "C" fn psf_fseek(handle: *mut c_void, offset: i64, whence: c_int) -> c_int {
    libc::fseek(handle as *mut libc::FILE, offset as c_long, whence)
}

unsafe extern "C" fn psf_fclose(handle: *mut c_void) -> c_int {
    libc::fclose(handle as *mut libc::FILE)
}

unsafe extern "C" fn psf_ftell(handle: *mut c_void) -> c_long {
    libc::ftell(handle as *mut libc::FILE)
}

static PSF_PATH_SEPARATORS: &[u8] = b"\\/:\0";

fn make_psf_callbacks() -> PsfFileCallbacks {
    PsfFileCallbacks {
        path_separators: PSF_PATH_SEPARATORS.as_ptr() as *const c_char,
        context: std::ptr::null_mut(),
        fopen: Some(psf_fopen),
        fread: Some(psf_fread),
        fseek: Some(psf_fseek),
        fclose: Some(psf_fclose),
        ftell: Some(psf_ftell),
    }
}

// ============================================================================
// PSF tag collection
// ============================================================================

/// Tags extracted from a GSF/minigsf file's PSF metadata.
#[derive(Debug, Clone, Default)]
pub struct GsfTags {
    pub title: String,
    pub artist: String,
    pub game: String,
    pub year: String,
    pub genre: String,
    pub comment: String,
    /// Play length in milliseconds (from "length" tag, format "M:SS.mmm")
    pub length_ms: u64,
    /// Fade duration in milliseconds (from "fade" tag)
    pub fade_ms: u64,
    /// Rating (0-5 stars, from "rating" tag)
    pub rating: i32,
}

/// Context passed to the PSF info callback for collecting tags
struct TagCollector {
    tags: GsfTags,
}

/// Parse a PSF time string like "2:30.500" or "150" into milliseconds
fn parse_psf_time(ts: &str) -> u64 {
    let ts = ts.trim();
    if ts.is_empty() {
        return 0;
    }

    // Check if there's a decimal point
    let (main_part, frac_ms) = if let Some(dot_pos) = ts.find('.') {
        let frac_str = &ts[dot_pos + 1..];
        // Pad or truncate to 3 digits for milliseconds
        let frac_ms = match frac_str.len() {
            0 => 0u64,
            1 => frac_str.parse::<u64>().unwrap_or(0) * 100,
            2 => frac_str.parse::<u64>().unwrap_or(0) * 10,
            3 => frac_str.parse::<u64>().unwrap_or(0),
            _ => frac_str[..3].parse::<u64>().unwrap_or(0),
        };
        (&ts[..dot_pos], frac_ms)
    } else {
        (ts, 0u64)
    };

    // Parse the M:SS or H:MM:SS or just seconds part
    let parts: Vec<&str> = main_part.split(':').collect();
    let seconds: u64 = match parts.len() {
        1 => parts[0].parse::<u64>().unwrap_or(0),
        2 => {
            let min = parts[0].parse::<u64>().unwrap_or(0);
            let sec = parts[1].parse::<u64>().unwrap_or(0);
            min * 60 + sec
        }
        3 => {
            let hr = parts[0].parse::<u64>().unwrap_or(0);
            let min = parts[1].parse::<u64>().unwrap_or(0);
            let sec = parts[2].parse::<u64>().unwrap_or(0);
            hr * 3600 + min * 60 + sec
        }
        _ => 0,
    };

    seconds * 1000 + frac_ms
}

unsafe extern "C" fn tag_info_callback(
    context: *mut c_void,
    name: *const c_char,
    value: *const c_char,
) -> c_int {
    let collector = &mut *(context as *mut TagCollector);
    let name = CStr::from_ptr(name).to_string_lossy().to_lowercase();
    let value = CStr::from_ptr(value).to_string_lossy().to_string();

    match name.as_str() {
        "title" => collector.tags.title = value,
        "artist" => collector.tags.artist = value,
        "game" => collector.tags.game = value,
        "year" => collector.tags.year = value,
        "genre" => collector.tags.genre = value,
        "comment" => collector.tags.comment = value,
        "length" => collector.tags.length_ms = parse_psf_time(&value),
        "fade" => collector.tags.fade_ms = parse_psf_time(&value),
        "rating" => collector.tags.rating = value.trim().parse::<i32>().unwrap_or(0).clamp(0, 5),
        _ => {} // ignore other tags
    }

    0
}

// ============================================================================
// psf_load callback for uploading ROM data into gsf_state
// ============================================================================

unsafe extern "C" fn gsf_upload_callback(
    context: *mut c_void,
    exe: *const u8,
    exe_size: usize,
    _reserved: *const u8,
    _reserved_size: usize,
) -> c_int {
    gsf_upload_section(context, exe, exe_size)
}

/// Status callback — logs psflib error/status messages for debugging
unsafe extern "C" fn gsf_status_callback(
    _context: *mut c_void,
    message: *const c_char,
) {
    if !message.is_null() {
        if let Ok(msg) = CStr::from_ptr(message).to_str() {
            eprintln!("[lazygsf] psflib status: {}", msg);
        }
    }
}

// ============================================================================
// Global init
// ============================================================================

static GSF_INIT: Once = Once::new();

fn ensure_gsf_init() {
    GSF_INIT.call_once(|| {
        unsafe {
            gsf_init();
        }
    });
}

// ============================================================================
// GsfDecoder — main public API
// ============================================================================

/// A GSF/minigsf audio decoder.
///
/// Wraps lazygsf's gsf_state_t and provides decoded PCM audio.
/// Each instance decodes a single GSF track.
pub struct GsfDecoder {
    /// Pointer to heap-allocated gsf_state_t
    state: *mut c_void,
}

// GsfDecoder is only used on the audio thread
unsafe impl Send for GsfDecoder {}

impl GsfDecoder {
    /// Open a GSF/minigsf file and prepare for decoding.
    ///
    /// Returns the decoder and extracted PSF tags (title, artist, game, duration).
    /// The `sample_rate` parameter sets the output sample rate (typically 44100).
    pub fn new(path: &Path, sample_rate: u32) -> Result<(Self, GsfTags), String> {
        ensure_gsf_init();

        // Allocate gsf_state_t
        let state_size = unsafe { gsf_get_state_size() };
        let state = unsafe {
            let ptr = libc::malloc(state_size);
            if ptr.is_null() {
                return Err("Failed to allocate gsf_state".to_string());
            }
            gsf_clear(ptr);
            ptr
        };

        // Prepare tag collector
        let mut collector = TagCollector {
            tags: GsfTags::default(),
        };

        // Convert path to C string
        let path_str = path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
        let c_path =
            CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

        let callbacks = make_psf_callbacks();

        // Load the PSF chain: resolves minigsf → gsflib, uploads ROM sections,
        // and collects tags
        let result = unsafe {
            psf_load(
                c_path.as_ptr(),
                &callbacks,
                0x22, // GSF version
                Some(gsf_upload_callback),
                state,
                Some(tag_info_callback),
                &mut collector as *mut TagCollector as *mut c_void,
                0, // don't want nested tags
                Some(gsf_status_callback),
                std::ptr::null_mut(),
            )
        };

        if result <= 0 {
            unsafe {
                gsf_shutdown(state);
                libc::free(state);
            }
            return Err(format!(
                "psf_load failed (code={}) for: {}",
                result, path_str
            ));
        }

        // Set sample rate
        unsafe {
            gsf_set_sample_rate(state, sample_rate);
        }

        Ok((GsfDecoder { state }, collector.tags))
    }

    /// Render `count` stereo frames of audio into `buffer`.
    ///
    /// `buffer` must have space for at least `count * 2` i16 samples
    /// (interleaved stereo: L, R, L, R, ...).
    pub fn render(&mut self, buffer: &mut [i16], count: usize) -> Result<(), String> {
        let result = unsafe { gsf_render(self.state, buffer.as_mut_ptr(), count) };
        if result != 0 {
            return Err("gsf_render failed".to_string());
        }
        Ok(())
    }

    /// Restart playback from the beginning.
    pub fn restart(&mut self) {
        unsafe {
            gsf_restart(self.state);
        }
    }
}

impl Drop for GsfDecoder {
    fn drop(&mut self) {
        unsafe {
            gsf_shutdown(self.state);
            libc::free(self.state);
        }
    }
}

// ============================================================================
// Metadata-only tag reading
// ============================================================================

/// Read PSF tags from a GSF/minigsf file without initializing the full decoder.
///
/// This is much lighter than `GsfDecoder::new()` since it doesn't load the ROM
/// into the emulator state, just parses the PSF container tags.
pub fn read_gsf_tags(path: &Path) -> Result<GsfTags, String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
    let c_path = CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

    let mut collector = TagCollector {
        tags: GsfTags::default(),
    };

    let callbacks = make_psf_callbacks();

    let result = unsafe {
        psf_load(
            c_path.as_ptr(),
            &callbacks,
            0x22, // GSF version
            None, // no load callback — metadata only
            std::ptr::null_mut(),
            Some(tag_info_callback),
            &mut collector as *mut TagCollector as *mut c_void,
            0,
            None,
            std::ptr::null_mut(),
        )
    };

    if result <= 0 {
        return Err(format!("psf_load failed for: {}", path_str));
    }

    Ok(collector.tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_psf_time() {
        assert_eq!(parse_psf_time("2:30"), 150_000);
        assert_eq!(parse_psf_time("2:30.500"), 150_500);
        assert_eq!(parse_psf_time("1:05"), 65_000);
        assert_eq!(parse_psf_time("0:30.100"), 30_100);
        assert_eq!(parse_psf_time("10"), 10_000);
        assert_eq!(parse_psf_time(""), 0);
        assert_eq!(parse_psf_time("1:00:00"), 3_600_000);
    }
}
