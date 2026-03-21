//! # vio2sf-rs
//!
//! Safe Rust wrapper for vio2sf (Nintendo DS sound format decoder using DeSmuME)
//! and psflib (PSF container parser).
//!
//! Provides `TwoSfDecoder` for decoding 2SF/mini2sf files into PCM audio,
//! and `read_twosf_tags` for metadata-only extraction from PSF tags.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_uint, c_void};
use std::path::Path;

// ============================================================================
// FFI declarations
// ============================================================================

// --- vio2sf / DeSmuME NDS_state ---
// NDS_state is a large struct but we treat it as opaque, allocating via malloc
// with the correct size. We only need the fields at known offsets for configuration.

// Size of NDS_state — we use a C helper or compute from the struct.
// Since NDS_state has a known layout, we'll allocate it as zeroed memory
// and let state_init fill it in.

/// Opaque NDS_state type. We interact with it only through FFI functions.
#[repr(C)]
pub struct NDS_state {
    // Configuration fields at the start of the struct
    dw_interpolation: libc::c_ulong,
    dw_channel_mute: libc::c_ulong,
    initial_frames: c_int,
    sync_type: c_int,
    arm9_clockdown_level: c_int,
    arm7_clockdown_level: c_int,
    // ... rest of the struct is opaque, handled by state_init
    _opaque: [u8; 8192], // Generous padding — actual struct is smaller
}

extern "C" {
    fn state_init(state: *mut NDS_state) -> c_int;
    fn state_deinit(state: *mut NDS_state);
    fn state_setrom(
        state: *mut NDS_state,
        rom: *mut u8,
        rom_size: u32,
        enable_coverage_checking: c_uint,
    );
    fn state_loadstate(state: *mut NDS_state, ss: *const u8, ss_size: u32);
    fn state_render(state: *mut NDS_state, buffer: *mut i16, sample_count: c_uint);
}

// --- psflib ---

/// File I/O callbacks for psflib
/// NOTE: This version of psflib does NOT have a context field in the struct.
/// The fopen callback takes only the path, not (context, path).
#[repr(C)]
struct PsfFileCallbacks {
    path_separators: *const c_char,
    fopen: Option<unsafe extern "C" fn(*const c_char) -> *mut c_void>,
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

// --- zlib (for decompressing 2SF reserved/save sections) ---
type ULongF = libc::c_ulong;

extern "C" {
    fn uncompress(
        dest: *mut u8,
        dest_len: *mut ULongF,
        source: *const u8,
        source_len: ULongF,
    ) -> c_int;
}

const Z_OK: c_int = 0;
const Z_BUF_ERROR: c_int = -5;
const Z_MEM_ERROR: c_int = -4;

// ============================================================================
// psflib file I/O callbacks (using libc FILE*)
// ============================================================================

unsafe extern "C" fn psf_fopen(path: *const c_char) -> *mut c_void {
    let mode = b"rb\0".as_ptr() as *const c_char;
    libc::fopen(path, mode) as *mut c_void
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
        fopen: Some(psf_fopen),
        fread: Some(psf_fread),
        fseek: Some(psf_fseek),
        fclose: Some(psf_fclose),
        ftell: Some(psf_ftell),
    }
}

// ============================================================================
// 2SF loader state and callbacks
// ============================================================================

/// Tags extracted from a 2SF/mini2sf file's PSF metadata.
#[derive(Debug, Clone, Default)]
pub struct TwoSfTags {
    pub title: String,
    pub artist: String,
    pub game: String,
    pub year: String,
    pub genre: String,
    pub comment: String,
    pub length_ms: u64,
    pub fade_ms: u64,
    /// Rating (0-5 stars, from "rating" tag)
    pub rating: i32,
}

/// Internal state accumulated during psf_load
struct LoaderState {
    rom: Vec<u8>,
    state: Vec<u8>,
    tags: TwoSfTags,
    // NDS-specific config from tags
    initial_frames: i32,
    sync_type: i32,
    clockdown: i32,
    arm9_clockdown_level: i32,
    arm7_clockdown_level: i32,
}

impl LoaderState {
    fn new() -> Self {
        Self {
            rom: Vec::new(),
            state: Vec::new(),
            tags: TwoSfTags::default(),
            initial_frames: 0,
            sync_type: 0,
            clockdown: 0,
            arm9_clockdown_level: 0,
            arm7_clockdown_level: 0,
        }
    }
}

/// Parse a PSF time string like "2:30.500" or "150" into milliseconds
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

/// Helper: read little-endian u32 from a byte slice
fn get_le32(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Load ROM/state map from an uncompressed data section.
/// Mirrors `load_twosf_map` from common.h.
fn load_twosf_map(buf: &mut Vec<u8>, is_save: bool, udata: &[u8]) -> Result<(), String> {
    if udata.len() < 8 {
        return Err("2SF data section too small".to_string());
    }

    let xofs = get_le32(&udata[0..4]) as usize;
    let xsize = get_le32(&udata[4..8]) as usize;

    if udata.len() < 8 + xsize {
        return Err("2SF data section truncated".to_string());
    }

    let needed = xofs + xsize;
    if buf.len() < needed {
        // For ROM, round up to power of 2
        let new_size = if !is_save {
            needed.next_power_of_two()
        } else {
            needed
        };
        buf.resize(new_size, 0);
    }

    buf[xofs..xofs + xsize].copy_from_slice(&udata[8..8 + xsize]);
    Ok(())
}

/// Decompress and load a save state section.
/// Mirrors `load_twosf_mapz` from common.h.
fn load_twosf_mapz(state_buf: &mut Vec<u8>, zdata: &[u8]) -> Result<(), String> {
    // Start with a guess for decompressed size
    let mut usize_val: ULongF = 8;
    let mut udata = vec![0u8; usize_val as usize];

    loop {
        let mut dest_len = usize_val;
        let zerr = unsafe {
            uncompress(
                udata.as_mut_ptr(),
                &mut dest_len,
                zdata.as_ptr(),
                zdata.len() as ULongF,
            )
        };

        if zerr == Z_OK {
            udata.truncate(dest_len as usize);
            return load_twosf_map(state_buf, true, &udata);
        }

        if zerr != Z_MEM_ERROR && zerr != Z_BUF_ERROR {
            return Err(format!("zlib uncompress failed: {}", zerr));
        }

        // Grow buffer and retry
        if usize_val >= 8 {
            let data_size = get_le32(&udata[4..8]) as ULongF + 8;
            if data_size > usize_val {
                usize_val = data_size;
            } else {
                usize_val *= 2;
            }
        } else {
            usize_val *= 2;
        }
        udata.resize(usize_val as usize, 0);
    }
}

/// PSF load callback — accumulates ROM and save state data.
/// Mirrors `twosf_loader` from common.h.
unsafe extern "C" fn twosf_load_callback(
    context: *mut c_void,
    exe: *const u8,
    exe_size: usize,
    reserved: *const u8,
    reserved_size: usize,
) -> c_int {
    let loader = &mut *(context as *mut LoaderState);

    // Load executable section (ROM data)
    if exe_size >= 8 {
        let exe_data = std::slice::from_raw_parts(exe, exe_size);
        if load_twosf_map(&mut loader.rom, false, exe_data).is_err() {
            return -1;
        }
    }

    // Load reserved section (save state chunks, zlib-compressed)
    if reserved_size > 0 {
        let reserved_data = std::slice::from_raw_parts(reserved, reserved_size);
        if reserved_size < 16 {
            return -1;
        }

        let mut pos = 0usize;
        while pos + 12 < reserved_size {
            let save_size = get_le32(&reserved_data[pos + 4..pos + 8]) as usize;
            let _save_crc = get_le32(&reserved_data[pos + 8..pos + 12]);

            // Check for "SAVE" magic (0x45564153)
            if get_le32(&reserved_data[pos..pos + 4]) == 0x45564153 {
                if pos + 12 + save_size > reserved_size {
                    return -1;
                }
                let zdata = &reserved_data[pos + 12..pos + 12 + save_size];
                if load_twosf_mapz(&mut loader.state, zdata).is_err() {
                    return -1;
                }
            }

            pos += 12 + save_size;
        }
    }

    0
}

/// PSF info callback — collects tags.
unsafe extern "C" fn twosf_info_callback(
    context: *mut c_void,
    name: *const c_char,
    value: *const c_char,
) -> c_int {
    let loader = &mut *(context as *mut LoaderState);
    let name = CStr::from_ptr(name).to_string_lossy().to_lowercase();
    let value = CStr::from_ptr(value).to_string_lossy().to_string();

    match name.as_str() {
        "title" => loader.tags.title = value,
        "artist" => loader.tags.artist = value,
        "game" => loader.tags.game = value,
        "year" => loader.tags.year = value,
        "genre" => loader.tags.genre = value,
        "comment" => loader.tags.comment = value,
        "length" => loader.tags.length_ms = parse_psf_time(&value),
        "fade" => loader.tags.fade_ms = parse_psf_time(&value),
        "rating" => loader.tags.rating = value.trim().parse::<i32>().unwrap_or(0).clamp(0, 5),
        "_frames" => {
            loader.initial_frames = value.parse::<i32>().unwrap_or(0);
        }
        "_clockdown" => {
            loader.clockdown = value.parse::<i32>().unwrap_or(0);
        }
        "_vio2sf_sync_type" => {
            loader.sync_type = value.parse::<i32>().unwrap_or(0);
        }
        "_vio2sf_arm9_clockdown_level" => {
            loader.arm9_clockdown_level = value.parse::<i32>().unwrap_or(0);
        }
        "_vio2sf_arm7_clockdown_level" => {
            loader.arm7_clockdown_level = value.parse::<i32>().unwrap_or(0);
        }
        _ => {} // ignore other tags
    }

    0
}

// ============================================================================
// TwoSfDecoder — main public API
// ============================================================================

/// A 2SF/mini2sf audio decoder.
///
/// Wraps DeSmuME's NDS_state and provides decoded PCM audio.
/// Each instance decodes a single 2SF track.
pub struct TwoSfDecoder {
    /// Heap-allocated NDS_state
    state: *mut NDS_state,
    /// ROM data — must stay alive as long as the decoder (NDS_state references it)
    _rom: Vec<u8>,
}

unsafe impl Send for TwoSfDecoder {}

impl TwoSfDecoder {
    /// Open a 2SF/mini2sf file and prepare for decoding.
    ///
    /// Returns the decoder and extracted PSF tags (title, artist, game, duration).
    pub fn new(path: &Path) -> Result<(Self, TwoSfTags), String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
        let c_path =
            CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

        let callbacks = make_psf_callbacks();
        let mut loader = LoaderState::new();

        // Phase 1: Load PSF chain — collects ROM data, save state, and tags
        let result = unsafe {
            psf_load(
                c_path.as_ptr(),
                &callbacks,
                0x24, // 2SF version
                Some(twosf_load_callback),
                &mut loader as *mut LoaderState as *mut c_void,
                Some(twosf_info_callback),
                &mut loader as *mut LoaderState as *mut c_void,
                1, // want nested tags
                None,
                std::ptr::null_mut(),
            )
        };

        if result <= 0 {
            return Err(format!("psf_load failed for: {}", path_str));
        }

        // Phase 2: Initialize NDS emulator
        let state = unsafe {
            let ptr = libc::calloc(1, std::mem::size_of::<NDS_state>()) as *mut NDS_state;
            if ptr.is_null() {
                return Err("Failed to allocate NDS_state".to_string());
            }
            ptr
        };

        let init_result = unsafe { state_init(state) };
        if init_result != 0 {
            unsafe {
                libc::free(state as *mut c_void);
            }
            return Err("state_init failed".to_string());
        }

        // Configure emulator
        unsafe {
            (*state).dw_interpolation = 0;
            (*state).dw_channel_mute = 0;

            // Apply clockdown settings
            let arm7_cd = if loader.arm7_clockdown_level != 0 {
                loader.arm7_clockdown_level
            } else {
                loader.clockdown
            };
            let arm9_cd = if loader.arm9_clockdown_level != 0 {
                loader.arm9_clockdown_level
            } else {
                loader.clockdown
            };

            (*state).initial_frames = loader.initial_frames;
            (*state).sync_type = loader.sync_type;
            (*state).arm7_clockdown_level = arm7_cd;
            (*state).arm9_clockdown_level = arm9_cd;
        }

        // Phase 3: Load ROM and save state into emulator
        let mut rom = loader.rom;
        if !rom.is_empty() {
            unsafe {
                state_setrom(state, rom.as_mut_ptr(), rom.len() as u32, 0);
            }
        }

        if !loader.state.is_empty() {
            unsafe {
                state_loadstate(state, loader.state.as_ptr(), loader.state.len() as u32);
            }
        }

        Ok((
            TwoSfDecoder {
                state,
                _rom: rom,
            },
            loader.tags,
        ))
    }

    /// Render `count` stereo frames of audio into `buffer`.
    ///
    /// `buffer` must have space for at least `count * 2` i16 samples
    /// (interleaved stereo: L, R, L, R, ...).
    pub fn render(&mut self, buffer: &mut [i16], count: usize) {
        unsafe {
            state_render(self.state, buffer.as_mut_ptr(), count as c_uint);
        }
    }

    /// Mute/unmute SPU channels by bitmask.
    /// Bit N = 1 means channel N is muted. Use 0xFFFF to mute all 16 channels.
    /// When muted, the SPU skips all fetch + resampler work for those channels.
    pub fn set_channel_mute(&mut self, mask: u32) {
        unsafe {
            (*self.state).dw_channel_mute = mask as libc::c_ulong;
        }
    }

    /// Get the current ARM9 clockdown level.
    pub fn arm9_clockdown(&self) -> i32 {
        unsafe { (*self.state).arm9_clockdown_level }
    }

    /// Set the ARM9 clockdown level.
    /// Each +1 doubles the cycle cost of ARM9 instructions, halving its effective speed.
    /// Useful during seek: ARM9 runs game logic (not sound), so slowing it saves CPU.
    pub fn set_arm9_clockdown(&mut self, level: i32) {
        unsafe {
            (*self.state).arm9_clockdown_level = level;
        }
    }
}

impl Drop for TwoSfDecoder {
    fn drop(&mut self) {
        unsafe {
            state_deinit(self.state);
            libc::free(self.state as *mut c_void);
        }
    }
}

// ============================================================================
// Metadata-only tag reading
// ============================================================================

/// Read PSF tags from a 2SF/mini2sf file without initializing the emulator.
pub fn read_twosf_tags(path: &Path) -> Result<TwoSfTags, String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
    let c_path = CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

    let mut loader = LoaderState::new();
    let callbacks = make_psf_callbacks();

    let result = unsafe {
        psf_load(
            c_path.as_ptr(),
            &callbacks,
            0x24, // 2SF version
            None, // no load callback — metadata only
            std::ptr::null_mut(),
            Some(twosf_info_callback),
            &mut loader as *mut LoaderState as *mut c_void,
            1,
            None,
            std::ptr::null_mut(),
        )
    };

    if result <= 0 {
        return Err(format!("psf_load failed for: {}", path_str));
    }

    Ok(loader.tags)
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

    #[test]
    fn test_get_le32() {
        assert_eq!(get_le32(&[0x01, 0x02, 0x03, 0x04]), 0x04030201);
        assert_eq!(get_le32(&[0x00, 0x00, 0x00, 0x00]), 0);
        assert_eq!(get_le32(&[0xFF, 0xFF, 0xFF, 0xFF]), 0xFFFFFFFF);
    }
}
