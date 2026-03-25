//! Rust bindings for viogsf (VBA-M based GBA/GSF audio decoder).
//!
//! viogsf uses the VBA-M (VisualBoyAdvance-M) GBA emulator core for
//! accurate audio emulation. This avoids the audio crackling issues
//! found in mGBA-based decoders (lazygsf) with certain games like
//! Metroid Fusion.

use std::ffi::CString;
use std::os::raw::c_void;
use std::path::Path;

extern "C" {
    fn viogsf_create(sample_rate: u32) -> *mut c_void;
    fn viogsf_load_rom(state: *mut c_void, data: *const u8, size: u32, entry_point: u32) -> i32;
    fn viogsf_render(state: *mut c_void, buf: *mut i16, count: usize) -> i32;
    fn viogsf_restart(state: *mut c_void);
    fn viogsf_destroy(state: *mut c_void);
}

// Re-use psflib from lazygsf-rs for loading GSF container files
// (psflib handles minigsf → gsflib chain resolution)
// For now, we load the ROM data directly using psflib callbacks

extern "C" {
    // psflib functions (shared with lazygsf-rs, renamed to avoid collision)
    #[link_name = "viogsf_psf_load"]
    fn psf_load(
        uri: *const libc::c_char,
        file_callbacks: *const PsfFileCallbacks,
        allowed_version: u8,
        load_target: PsfLoadCallback,
        load_context: *mut c_void,
        info_target: PsfInfoCallback,
        info_context: *mut c_void,
        info_want_nested_tags: libc::c_int,
        status_target: PsfStatusCallback,
        status_context: *mut c_void,
    ) -> libc::c_int;
}

// psflib file I/O callback types
#[repr(C)]
struct PsfFileCallbacks {
    path_separators: *const libc::c_char,
    context: *mut c_void,
    fopen: Option<unsafe extern "C" fn(*mut c_void, *const libc::c_char) -> *mut c_void>,
    fread: Option<unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize>,
    fseek: Option<unsafe extern "C" fn(*mut c_void, i64, libc::c_int) -> libc::c_int>,
    fclose: Option<unsafe extern "C" fn(*mut c_void) -> libc::c_int>,
    ftell: Option<unsafe extern "C" fn(*mut c_void) -> libc::c_long>,
}

type PsfLoadCallback =
    Option<unsafe extern "C" fn(*mut c_void, *const u8, usize, *const u8, usize) -> libc::c_int>;
type PsfInfoCallback =
    Option<unsafe extern "C" fn(*mut c_void, *const libc::c_char, *const libc::c_char) -> libc::c_int>;
type PsfStatusCallback = Option<unsafe extern "C" fn(*mut c_void, *const libc::c_char)>;

// File I/O callbacks (same as lazygsf-rs but with parent directory search)
unsafe extern "C" fn psf_fopen(_context: *mut c_void, path: *const libc::c_char) -> *mut c_void {
    let mode = b"rb\0".as_ptr() as *const libc::c_char;
    let handle = libc::fopen(path, mode);
    if !handle.is_null() {
        return handle as *mut c_void;
    }

    // Search parent directories for .gsflib files
    let path_str = match std::ffi::CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let p = std::path::Path::new(path_str);
    let filename = match p.file_name() {
        Some(f) => f,
        None => return std::ptr::null_mut(),
    };
    let fname_str = filename.to_string_lossy();
    if !fname_str.ends_with("lib") && !fname_str.ends_with(".gsflib") {
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
                let c = match CString::new(candidate.to_string_lossy().as_bytes()) {
                    Ok(c) => c,
                    Err(_) => break,
                };
                let h = libc::fopen(c.as_ptr(), mode);
                if !h.is_null() {
                    return h as *mut c_void;
                }
            }
        }
    }
    std::ptr::null_mut()
}

unsafe extern "C" fn psf_fread(buf: *mut c_void, size: usize, count: usize, handle: *mut c_void) -> usize {
    libc::fread(buf, size, count, handle as *mut libc::FILE)
}
unsafe extern "C" fn psf_fseek(handle: *mut c_void, offset: i64, whence: libc::c_int) -> libc::c_int {
    libc::fseek(handle as *mut libc::FILE, offset as libc::c_long, whence)
}
unsafe extern "C" fn psf_fclose(handle: *mut c_void) -> libc::c_int {
    libc::fclose(handle as *mut libc::FILE)
}
unsafe extern "C" fn psf_ftell(handle: *mut c_void) -> libc::c_long {
    libc::ftell(handle as *mut libc::FILE)
}

static PSF_PATH_SEPARATORS: &[u8] = b"\\/:\0";

fn make_psf_callbacks() -> PsfFileCallbacks {
    PsfFileCallbacks {
        path_separators: PSF_PATH_SEPARATORS.as_ptr() as *const libc::c_char,
        context: std::ptr::null_mut(),
        fopen: Some(psf_fopen),
        fread: Some(psf_fread),
        fseek: Some(psf_fseek),
        fclose: Some(psf_fclose),
        ftell: Some(psf_ftell),
    }
}

// Tag collection (same structure as lazygsf)
#[derive(Debug, Clone, Default)]
pub struct GsfTags {
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

struct LoadCollector {
    rom_data: Vec<u8>,
    rom_offset: u32,
    entry_point: u32,
    tags: GsfTags,
}

// PSF load callback - receives the GSF exe data (GBA ROM + header)
unsafe extern "C" fn gsf_upload_callback(
    context: *mut c_void,
    exe: *const u8,
    exe_size: usize,
    _reserved: *const u8,
    _reserved_size: usize,
) -> libc::c_int {
    if exe_size < 12 || context.is_null() {
        return -1;
    }
    let collector = &mut *(context as *mut LoadCollector);

    // GSF exe header: 4 bytes entry, 4 bytes offset, 4 bytes size, then ROM data
    let entry = u32::from_le_bytes([*exe, *exe.add(1), *exe.add(2), *exe.add(3)]);
    // Mask offset to 25 bits (0x1FFFFFF) — without this, the ROM buffer
    // is allocated at the full GBA memory address (e.g. 0x08000000 = 128MB)
    // creating a huge mostly-empty buffer that the emulator can't use properly.
    let offset = u32::from_le_bytes([*exe.add(4), *exe.add(5), *exe.add(6), *exe.add(7)]) & 0x1FFFFFF;
    let size = u32::from_le_bytes([*exe.add(8), *exe.add(9), *exe.add(10), *exe.add(11)]);

    let data_start = 12;
    let data_end = std::cmp::min(exe_size, data_start + size as usize);

    collector.entry_point = entry;
    collector.rom_offset = offset;

    // Expand ROM buffer if needed
    let needed = offset as usize + (data_end - data_start);
    if collector.rom_data.len() < needed {
        collector.rom_data.resize(needed, 0);
    }

    // Copy ROM data at the specified offset
    let src = std::slice::from_raw_parts(exe.add(data_start), data_end - data_start);
    collector.rom_data[offset as usize..offset as usize + src.len()].copy_from_slice(src);

    0
}

fn parse_psf_time(s: &str) -> u64 {
    let s = s.trim();
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => (parts[0].parse::<f64>().unwrap_or(0.0) * 1000.0) as u64,
        2 => {
            let min = parts[0].parse::<u64>().unwrap_or(0);
            let sec = parts[1].parse::<f64>().unwrap_or(0.0);
            min * 60000 + (sec * 1000.0) as u64
        }
        _ => 0,
    }
}

unsafe extern "C" fn tag_info_callback(
    context: *mut c_void,
    name: *const libc::c_char,
    value: *const libc::c_char,
) -> libc::c_int {
    if context.is_null() || name.is_null() || value.is_null() {
        return 0;
    }
    let collector = &mut *(context as *mut LoadCollector);
    let name = std::ffi::CStr::from_ptr(name).to_string_lossy().to_lowercase();
    let value = std::ffi::CStr::from_ptr(value).to_string_lossy().to_string();

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

/// VBA-M based GSF decoder. More accurate audio than mGBA (lazygsf).
pub struct VioGsfDecoder {
    state: *mut c_void,
}

unsafe impl Send for VioGsfDecoder {}

impl VioGsfDecoder {
    /// Open a GSF/minigsf file and prepare for decoding.
    pub fn new(path: &Path, sample_rate: u32) -> Result<(Self, GsfTags), String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
        let c_path = CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

        let mut collector = LoadCollector {
            rom_data: Vec::new(),
            rom_offset: 0,
            entry_point: 0,
            tags: GsfTags::default(),
        };

        let callbacks = make_psf_callbacks();

        // Load the GSF chain (minigsf → gsflib)
        let result = unsafe {
            psf_load(
                c_path.as_ptr(),
                &callbacks,
                0x22, // GSF version
                Some(gsf_upload_callback),
                &mut collector as *mut LoadCollector as *mut c_void,
                Some(tag_info_callback),
                &mut collector as *mut LoadCollector as *mut c_void,
                0,
                None,
                std::ptr::null_mut(),
            )
        };

        if result <= 0 {
            return Err(format!("psf_load failed (code={}) for: {}", result, path_str));
        }

        if collector.rom_data.is_empty() {
            return Err("GSF contained no ROM data".to_string());
        }

        // Create the VBA-M emulator and load the ROM
        let state = unsafe { viogsf_create(sample_rate) };
        if state.is_null() {
            return Err("Failed to create viogsf state".to_string());
        }

        let load_result = unsafe {
            viogsf_load_rom(
                state,
                collector.rom_data.as_ptr(),
                collector.rom_data.len() as u32,
                collector.entry_point,
            )
        };

        if load_result != 0 {
            unsafe { viogsf_destroy(state); }
            return Err(format!("Failed to load GSF ROM: {}", path_str));
        }

        Ok((VioGsfDecoder { state }, collector.tags))
    }

    /// Render `count` stereo frames of audio.
    pub fn render(&self, buffer: &mut [i16], count: usize) -> Result<(), String> {
        if buffer.len() < count * 2 {
            return Err("Buffer too small".to_string());
        }
        let result = unsafe { viogsf_render(self.state, buffer.as_mut_ptr(), count) };
        if result != 0 {
            return Err("viogsf_render failed".to_string());
        }
        Ok(())
    }

    /// Reset playback to the beginning (for seeking via reopen+fast-forward).
    pub fn restart(&self) {
        unsafe { viogsf_restart(self.state); }
    }
}

impl Drop for VioGsfDecoder {
    fn drop(&mut self) {
        if !self.state.is_null() {
            unsafe { viogsf_destroy(self.state); }
            self.state = std::ptr::null_mut();
        }
    }
}

/// Read PSF tags from a GSF/minigsf file without initializing the emulator.
pub fn read_gsf_tags(path: &Path) -> Result<GsfTags, String> {
    let path_str = path.to_str().ok_or("Invalid UTF-8")?;
    let c_path = CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

    let mut collector = LoadCollector {
        rom_data: Vec::new(),
        rom_offset: 0,
        entry_point: 0,
        tags: GsfTags::default(),
    };

    let callbacks = make_psf_callbacks();
    let result = unsafe {
        psf_load(
            c_path.as_ptr(),
            &callbacks,
            0x22,
            None, // no ROM upload — tags only
            std::ptr::null_mut(),
            Some(tag_info_callback),
            &mut collector as *mut LoadCollector as *mut c_void,
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
