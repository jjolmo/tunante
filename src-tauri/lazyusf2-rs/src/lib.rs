//! # lazyusf2-rs
//!
//! Safe Rust wrapper for lazyusf2 (N64 sound format decoder using Mupen64Plus)
//! and psflib (PSF container parser).
//!
//! Provides `UsfDecoder` for decoding USF/miniusf files into PCM audio,
//! and `read_usf_tags` for metadata-only extraction from PSF tags.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::path::Path;

// ============================================================================
// FFI declarations
// ============================================================================

extern "C" {
    fn usf_get_state_size() -> usize;
    fn usf_clear(state: *mut c_void);
    fn usf_set_compare(state: *mut c_void, enable: c_int);
    fn usf_set_fifo_full(state: *mut c_void, enable: c_int);
    fn usf_set_hle_audio(state: *mut c_void, enable: c_int);
    fn usf_upload_section(state: *mut c_void, data: *const u8, size: usize) -> c_int;
    fn usf_render_resampled(
        state: *mut c_void,
        buffer: *mut i16,
        count: usize,
        sample_rate: i32,
    ) -> *const c_char;
    fn usf_restart(state: *mut c_void);
    fn usf_shutdown(state: *mut c_void);
}

// --- psflib ---

#[repr(C)]
struct PsfFileCallbacks {
    path_separators: *const c_char,
    context: *mut c_void,
    fopen: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    fread: Option<unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize>,
    fseek: Option<unsafe extern "C" fn(*mut c_void, i64, c_int) -> c_int>,
    fclose: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    ftell: Option<unsafe extern "C" fn(*mut c_void) -> c_long>,
}

type PsfLoadCallback =
    Option<unsafe extern "C" fn(*mut c_void, *const u8, usize, *const u8, usize) -> c_int>;
type PsfInfoCallback =
    Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int>;
type PsfStatusCallback = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;

extern "C" {
    #[link_name = "usf_psf_load"]
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
// psflib file I/O callbacks
// ============================================================================

unsafe extern "C" fn psf_fopen(_context: *mut c_void, path: *const c_char) -> *mut c_void {
    let mode = b"rb\0".as_ptr() as *const c_char;
    let handle = libc::fopen(path, mode);
    if !handle.is_null() {
        return handle as *mut c_void;
    }

    // Search parent directories for .usflib files
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let p = std::path::Path::new(path_str);
    let filename = match p.file_name() {
        Some(f) => f,
        None => return std::ptr::null_mut(),
    };

    let fname_str = filename.to_string_lossy();
    let is_lib = fname_str.ends_with("lib")
        || fname_str.ends_with(".usflib");

    if !is_lib {
        return std::ptr::null_mut();
    }

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

#[derive(Debug, Clone, Default)]
pub struct UsfTags {
    pub title: String,
    pub artist: String,
    pub game: String,
    pub year: String,
    pub genre: String,
    pub comment: String,
    pub length_ms: u64,
    pub fade_ms: u64,
    pub rating: i32,
}

struct TagCollector {
    tags: UsfTags,
}

fn parse_psf_time(ts: &str) -> u64 {
    let ts = ts.trim();
    if ts.is_empty() {
        return 0;
    }

    let (main_part, frac_ms) = if let Some(dot_pos) = ts.find('.') {
        let frac_str = &ts[dot_pos + 1..];
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
        _ => {}
    }

    0
}

// ============================================================================
// psf_load callback — USF stores data in the RESERVED section (not exe)
// ============================================================================

unsafe extern "C" fn usf_upload_callback(
    context: *mut c_void,
    _exe: *const u8,
    _exe_size: usize,
    reserved: *const u8,
    reserved_size: usize,
) -> c_int {
    if reserved_size > 0 {
        usf_upload_section(context, reserved, reserved_size)
    } else {
        0
    }
}

unsafe extern "C" fn usf_status_callback(_context: *mut c_void, _message: *const c_char) {
    // Intentionally silent — eprintln! crashes with "broken pipe" (os error 32)
    // when the app is launched without a terminal (e.g. from .desktop file).
}

// ============================================================================
// UsfDecoder — main public API
// ============================================================================

pub struct UsfDecoder {
    state: *mut c_void,
    sample_rate: i32,
}

unsafe impl Send for UsfDecoder {}

impl UsfDecoder {
    /// Open a USF/miniusf file and prepare for decoding.
    pub fn new(path: &Path, sample_rate: u32) -> Result<(Self, UsfTags), String> {
        let state_size = unsafe { usf_get_state_size() };
        let state = unsafe {
            let ptr = libc::malloc(state_size);
            if ptr.is_null() {
                return Err("Failed to allocate usf_state".to_string());
            }
            usf_clear(ptr);
            ptr
        };

        let mut collector = TagCollector {
            tags: UsfTags::default(),
        };

        let path_str = path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
        let c_path =
            CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

        let callbacks = make_psf_callbacks();

        let result = unsafe {
            psf_load(
                c_path.as_ptr(),
                &callbacks,
                0x21, // USF version
                Some(usf_upload_callback),
                state,
                Some(tag_info_callback),
                &mut collector as *mut TagCollector as *mut c_void,
                0,
                Some(usf_status_callback),
                std::ptr::null_mut(),
            )
        };

        if result <= 0 {
            unsafe {
                usf_shutdown(state);
                libc::free(state);
            }
            return Err(format!("psf_load failed (code={}) for: {}", result, path_str));
        }

        // Configure emulator
        unsafe {
            usf_set_hle_audio(state, 1); // HLE = faster, sufficient for music
            usf_set_compare(state, 0);
            usf_set_fifo_full(state, 0);
        }

        Ok((
            UsfDecoder {
                state,
                sample_rate: sample_rate as i32,
            },
            collector.tags,
        ))
    }

    /// Render `count` stereo frames of audio into `buffer`.
    pub fn render(&mut self, buffer: &mut [i16], count: usize) -> Result<(), String> {
        let err = unsafe {
            usf_render_resampled(self.state, buffer.as_mut_ptr(), count, self.sample_rate)
        };
        if !err.is_null() {
            let msg = unsafe { CStr::from_ptr(err).to_string_lossy().to_string() };
            return Err(format!("usf_render failed: {}", msg));
        }
        Ok(())
    }

    /// Restart playback from the beginning.
    pub fn restart(&mut self) {
        unsafe {
            usf_restart(self.state);
        }
    }
}

impl Drop for UsfDecoder {
    fn drop(&mut self) {
        unsafe {
            usf_shutdown(self.state);
            libc::free(self.state);
        }
    }
}

// ============================================================================
// Metadata-only tag reading
// ============================================================================

/// Read PSF tags from a USF/miniusf file without initializing the emulator.
pub fn read_usf_tags(path: &Path) -> Result<UsfTags, String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
    let c_path = CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

    let mut collector = TagCollector {
        tags: UsfTags::default(),
    };

    let callbacks = make_psf_callbacks();

    let result = unsafe {
        psf_load(
            c_path.as_ptr(),
            &callbacks,
            0x21, // USF version
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
    fn test_usf_decode() {
        let path = std::path::Path::new("/media/cidwel/storage/Seafile/Cidwel/Musica/OST juegos/N64/Star Fox 64/09 Corneria.miniusf");
        if !path.exists() {
            eprintln!("Skipping: test file not found");
            return;
        }
        
        let (mut decoder, tags) = UsfDecoder::new(path, 44100).expect("Failed to load USF");
        eprintln!("Title: {}", tags.title);
        eprintln!("Game: {}", tags.game);
        eprintln!("Length: {}ms", tags.length_ms);
        
        let mut buf = vec![0i16; 4096 * 2];
        decoder.render(&mut buf, 4096).expect("Failed to render");
        let max = buf.iter().map(|s| s.abs() as u32).max().unwrap_or(0);
        eprintln!("Max sample amplitude: {}", max);
        assert!(max > 0, "Output is silence - decoder not working");
    }
}
