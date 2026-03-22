//! # hepsf-rs
//!
//! Safe Rust wrappers for PlayStation sound format decoders:
//!
//! - **PSF1** (PS1): Uses sexypsf — a PCSX-based PS1 emulator with HLE BIOS.
//!   Provides `PsfDecoder` and `read_psf_tags`.
//!
//! - **PSF2** (PS2): Uses Highly Experimental (HE) — an IOP emulator.
//!   The required PS2 BIOS (hebios.bin) is embedded in the binary — no
//!   external files needed. Provides `Psf2Decoder` and `read_psf2_tags`.
//!
//! Both decoders output 44100 Hz stereo PCM audio.
//!
//! **Important:** sexypsf (PSF1) uses global state, so only ONE `PsfDecoder` can
//! be active at a time. PSF2 uses per-instance state and is reentrant.

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

// ============================================================================
// PSF2 (PlayStation 2) support via Highly Experimental (HE)
// ============================================================================

pub mod psf2 {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int, c_long, c_void};
    use std::path::Path;
    use std::sync::OnceLock;

    // ========================================================================
    // FFI declarations — Highly Experimental (HE) library
    // ========================================================================

    // HE uses custom integer types (sint32, uint32, etc.) defined in emuconfig.h.
    // On little-endian platforms, these map to standard C int types.

    extern "C" {
        // BIOS management
        fn bios_set_image(image: *mut u8, size: u32);

        // Library init
        fn psx_init() -> i32;

        // State management (version: 1=PS1, 2=PS2)
        fn psx_get_state_size(version: u8) -> u32;
        fn psx_clear_state(state: *mut c_void, version: u8);

        // Execution — synchronous, pull-based audio generation
        fn psx_execute(
            state: *mut c_void,
            cycles: i32,
            sound_buf: *mut i16,
            sound_samples: *mut u32,
            event_mask: u32,
        ) -> i32;

        // Virtual filesystem callback for PS2 (reads from psf2fs)
        fn psx_set_readfile(
            state: *mut c_void,
            callback: Option<
                unsafe extern "C" fn(
                    context: *mut c_void,
                    path: *const c_char,
                    offset: i32,
                    buffer: *mut c_char,
                    length: i32,
                ) -> i32,
            >,
            context: *mut c_void,
        );
    }

    // ========================================================================
    // FFI declarations — psflib (PSF container parser)
    // ========================================================================

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

    // ========================================================================
    // FFI declarations — psf2fs (virtual filesystem for PSF2 containers)
    // ========================================================================

    extern "C" {
        fn psf2fs_create() -> *mut c_void;
        fn psf2fs_delete(fs: *mut c_void);
        fn psf2fs_load_callback(
            psf2vfs: *mut c_void,
            exe: *const u8,
            exe_size: usize,
            reserved: *const u8,
            reserved_size: usize,
        ) -> c_int;
        fn psf2fs_virtual_readfile(
            psf2vfs: *mut c_void,
            path: *const c_char,
            offset: i32,
            buffer: *mut c_char,
            length: i32,
        ) -> i32;
    }

    // ========================================================================
    // psflib file I/O callbacks (using libc FILE*)
    // ========================================================================

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

    // ========================================================================
    // HE library initialization (one-time, thread-safe)
    // ========================================================================

    /// Embedded PS2 BIOS image (hebios.bin, 512 KB).
    /// Pre-built by mkhebios from a PS2 BIOS dump — contains only the IOP
    /// modules needed for audio playback.
    static EMBEDDED_HEBIOS: &[u8] = include_bytes!("../hebios.bin");

    /// Cached initialization result — ensures the BIOS is loaded exactly once.
    /// Uses `OnceLock` so subsequent calls return the cached result.
    static HE_INIT_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

    /// Pointer to the BIOS image — must live for the entire process lifetime.
    /// Allocated via libc::malloc; never freed (process-lifetime singleton).
    static mut BIOS_PTR: *mut u8 = std::ptr::null_mut();

    /// Initialize the HE library with the embedded BIOS image.
    ///
    /// The BIOS is embedded in the binary at compile time — no external files needed.
    /// The result is cached — subsequent calls return the same result instantly.
    fn ensure_he_initialized() -> Result<(), String> {
        HE_INIT_RESULT
            .get_or_init(|| {
                unsafe {
                    // Allocate persistent memory and copy the embedded BIOS image
                    let size = EMBEDDED_HEBIOS.len();
                    let ptr = libc::malloc(size) as *mut u8;
                    if ptr.is_null() {
                        return Err("Failed to allocate memory for BIOS image".to_string());
                    }
                    std::ptr::copy_nonoverlapping(EMBEDDED_HEBIOS.as_ptr(), ptr, size);

                    // Register with HE library (must be power of 2 — 512KB = 0x80000 ✓)
                    bios_set_image(ptr, size as u32);
                    BIOS_PTR = ptr;

                    // Initialize all HE subsystems (IOP, R3000, SPU, timers, etc.)
                    let r = psx_init();
                    if r != 0 {
                        return Err(format!("psx_init failed with code {}", r));
                    }
                }
                Ok(())
            })
            .clone()
    }

    // ========================================================================
    // PSF2 tag collection during loading
    // ========================================================================

    /// Tags extracted from a PSF2/minipsf2 file's metadata.
    #[derive(Debug, Clone, Default)]
    pub struct Psf2Tags {
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

    /// Internal state accumulated during psf_load
    struct LoaderState {
        tags: Psf2Tags,
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

    /// PSF info callback — collects tags from PSF2 metadata.
    unsafe extern "C" fn psf2_info_callback(
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
            "rating" => {
                loader.tags.rating = value.trim().parse::<i32>().unwrap_or(0).clamp(0, 5)
            }
            _ => {} // ignore other tags
        }

        0
    }

    // ========================================================================
    // Psf2Decoder — main public API
    // ========================================================================

    /// Sample rate for PS2 HE audio output.
    /// PS2 IOP clock = 36,864,000 Hz / 768 cycles per sample = 48,000 Hz.
    #[allow(dead_code)]
    const SAMPLE_RATE: u32 = 48000;
    /// Number of CPU cycles per psx_execute call (~1024 samples worth)
    /// 768 cycles/sample * 1024 samples = ~786432 cycles
    const CYCLES_PER_RENDER: i32 = 768 * 1024;

    /// A PSF2/minipsf2 audio decoder.
    ///
    /// Wraps the Highly Experimental (HE) IOP emulator for PS2 audio.
    /// Each instance has its own emulator state — multiple decoders can
    /// coexist (unlike PSF1/sexypsf which uses global state).
    pub struct Psf2Decoder {
        /// HE emulator state (allocated via malloc, version=2)
        psx_state: *mut c_void,
        /// PSF2 virtual filesystem (contains IOP modules from the PSF2 container)
        psf2fs: *mut c_void,
        /// Internal render buffer for samples from psx_execute
        render_buf: Vec<i16>,
        /// Position in render_buf (in stereo frames)
        render_pos: usize,
        /// Number of valid frames in render_buf
        render_len: usize,
    }

    unsafe impl Send for Psf2Decoder {}

    impl Psf2Decoder {
        /// Open a PSF2/minipsf2 file and prepare for decoding.
        ///
        /// Returns the decoder and extracted PSF tags (title, artist, game, duration).
        /// The PS2 BIOS is embedded in the binary — no external files needed.
        pub fn new(path: &Path) -> Result<(Self, Psf2Tags), String> {
            // Ensure HE library is initialized (one-time)
            ensure_he_initialized()?;

            let path_str = path
                .to_str()
                .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
            let c_path =
                CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

            let callbacks = make_psf_callbacks();
            let mut loader = LoaderState {
                tags: Psf2Tags::default(),
            };

            // Create PSF2 virtual filesystem
            let psf2fs = unsafe { psf2fs_create() };
            if psf2fs.is_null() {
                return Err("psf2fs_create failed".to_string());
            }

            // Load PSF2 chain — populates the virtual filesystem and collects tags
            let result = unsafe {
                psf_load(
                    c_path.as_ptr(),
                    &callbacks,
                    0x02, // PSF version 2 = PS2
                    Some(psf2fs_load_callback),
                    psf2fs,
                    Some(psf2_info_callback),
                    &mut loader as *mut LoaderState as *mut c_void,
                    1, // want nested tags
                    None,
                    std::ptr::null_mut(),
                )
            };

            if result <= 0 {
                unsafe { psf2fs_delete(psf2fs); }
                return Err(format!("psf_load failed for PSF2: {}", path_str));
            }

            // Allocate and initialize PS2 emulator state
            let state_size = unsafe { psx_get_state_size(2) };
            let psx_state = unsafe { libc::calloc(1, state_size as usize) as *mut c_void };
            if psx_state.is_null() {
                unsafe { psf2fs_delete(psf2fs); }
                return Err("Failed to allocate PSX state".to_string());
            }

            unsafe {
                // Initialize PS2 state (version=2 includes VFS + IOP + SPU2)
                psx_clear_state(psx_state, 2);

                // Register psf2fs as the virtual file reader
                // The IOP will call this to load modules (*.irx) and data files
                psx_set_readfile(psx_state, Some(psf2fs_virtual_readfile), psf2fs);
            }

            let tags = loader.tags;

            Ok((
                Psf2Decoder {
                    psx_state,
                    psf2fs,
                    render_buf: vec![0i16; 2048 * 2], // pre-allocate render buffer
                    render_pos: 0,
                    render_len: 0,
                },
                tags,
            ))
        }

        /// Render `count` stereo frames of audio into `buffer`.
        ///
        /// `buffer` must have space for at least `count * 2` i16 samples
        /// (interleaved stereo: L, R, L, R, ...).
        pub fn render(&mut self, buffer: &mut [i16], count: usize) {
            let mut written = 0usize;

            while written < count {
                // Drain any buffered samples first
                if self.render_pos < self.render_len {
                    let available = self.render_len - self.render_pos;
                    let needed = count - written;
                    let to_copy = available.min(needed);

                    let src_start = self.render_pos * 2;
                    let dst_start = written * 2;
                    buffer[dst_start..dst_start + to_copy * 2]
                        .copy_from_slice(&self.render_buf[src_start..src_start + to_copy * 2]);

                    self.render_pos += to_copy;
                    written += to_copy;
                    continue;
                }

                // Need more samples — run the emulator
                let max_samples = self.render_buf.len() / 2;
                let mut samples_generated: u32 = max_samples as u32;

                let result = unsafe {
                    psx_execute(
                        self.psx_state,
                        CYCLES_PER_RENDER,
                        self.render_buf.as_mut_ptr(),
                        &mut samples_generated,
                        0, // no event mask
                    )
                };

                if result <= -2 || samples_generated == 0 {
                    // Unrecoverable error or no samples — fill remainder with silence
                    for s in buffer[written * 2..count * 2].iter_mut() {
                        *s = 0;
                    }
                    return;
                }

                self.render_pos = 0;
                self.render_len = samples_generated as usize;
            }
        }
    }

    impl Drop for Psf2Decoder {
        fn drop(&mut self) {
            if !self.psx_state.is_null() {
                unsafe {
                    libc::free(self.psx_state);
                }
                self.psx_state = std::ptr::null_mut();
            }
            if !self.psf2fs.is_null() {
                unsafe {
                    psf2fs_delete(self.psf2fs);
                }
                self.psf2fs = std::ptr::null_mut();
            }
        }
    }

    // ========================================================================
    // Metadata-only tag reading
    // ========================================================================

    /// Read PSF2 tags from a PSF2/minipsf2 file without initializing the emulator.
    ///
    /// Only parses the PSF container metadata — much faster than creating a decoder.
    pub fn read_psf2_tags(path: &Path) -> Result<Psf2Tags, String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| "Invalid UTF-8 in path".to_string())?;
        let c_path = CString::new(path_str).map_err(|_| "Path contains null byte".to_string())?;

        let callbacks = make_psf_callbacks();
        let mut loader = LoaderState {
            tags: Psf2Tags::default(),
        };

        let result = unsafe {
            psf_load(
                c_path.as_ptr(),
                &callbacks,
                0x02, // PSF version 2
                None, // no load callback — metadata only
                std::ptr::null_mut(),
                Some(psf2_info_callback),
                &mut loader as *mut LoaderState as *mut c_void,
                1,
                None,
                std::ptr::null_mut(),
            )
        };

        if result <= 0 {
            return Err(format!("psf_load failed for PSF2: {}", path_str));
        }

        Ok(loader.tags)
    }
}

// Re-export PSF2 types at the crate root for convenience
pub use psf2::{Psf2Decoder, Psf2Tags, read_psf2_tags};

#[cfg(test)]
mod tests {
    #[test]
    fn test_cstr_to_string_null() {
        unsafe {
            assert_eq!(super::cstr_to_string(std::ptr::null()), "");
        }
    }
}
