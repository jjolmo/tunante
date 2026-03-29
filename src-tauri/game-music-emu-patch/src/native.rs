use crate::emu_type::EmuType;
use crate::error::{GmeError, GmeOrIoError, GmeResult};
use std::ffi::{CStr, CString};
use std::mem::{transmute, transmute_copy};
use std::os::raw::c_char;
use std::path::Path;
use std::sync::Arc;

/// Holds a pointer to a `MusicEmu` instance in the C++ code. It automatically frees the instance
/// when dropped.
#[derive(Clone)]
pub(crate) struct EmuHandle {
    pub(crate) emu: Arc<MusicEmu>,
}

impl EmuHandle {
    pub(crate) fn new(emu: *const MusicEmu) -> Self {
        unsafe {
            Self {
                emu: Arc::new(transmute(emu)),
            }
        }
    }

    pub(crate) fn to_raw(&self) -> *const MusicEmu {
        unsafe { transmute_copy(&*self.emu) }
    }
}

impl Drop for EmuHandle {
    fn drop(&mut self) {
        if Arc::strong_count(&self.emu) == 1 {
            delete(self);
        }
    }
}

pub(crate) fn delete(handle: &EmuHandle) {
    unsafe {
        gme_delete(handle.to_raw());
    }
}

/// Determine likely `EmuType` based on first four bytes of file.
pub fn identify_header(buffer: &[u8]) -> EmuType {
    unsafe {
        EmuType::from_extension(
            &CStr::from_ptr(gme_identify_header(buffer.as_ptr()))
                .to_str()
                .unwrap()
                .to_string(),
        )
    }
}

/// Load music file from memory into emulator. Makes a copy of data passed.
pub(crate) fn load_data(handle: &EmuHandle, data: &[u8]) -> GmeResult<()> {
    unsafe {
        // let mut emu_ptr: *const MusicEmu = std::ptr::null_mut();
        process_result(gme_load_data(handle.to_raw(), data.as_ptr(), data.len()))
    }
}

/// Load music file into emulator
pub(crate) fn load_file(handle: &EmuHandle, path: impl AsRef<Path>) -> Result<(), GmeOrIoError> {
    let buffer = get_file_data(path)?;
    Ok(load_data(handle, &buffer)?)
}

/// Creates an `EmuHandle` with the specified `EmuType`
pub(crate) fn new_emu(emu_type: EmuType, sample_rate: u32) -> EmuHandle {
    unsafe {
        let cstring = CString::new(emu_type.to_extension()).unwrap();
        let gme_type = gme_identify_extension(cstring.as_ptr());
        let music_emu = gme_new_emu(gme_type, sample_rate as i32);
        EmuHandle::new(music_emu)
    }
}

pub(crate) fn open_data(data: &[u8], sample_rate: u32) -> GmeResult<EmuHandle> {
    let emu_type = identify_header(data);
    let handle = new_emu(emu_type, sample_rate);
    load_data(&handle, data)?;
    Ok(handle)
}

pub(crate) fn open_file(
    path: impl AsRef<Path>,
    sample_rate: u32,
) -> Result<EmuHandle, GmeOrIoError> {
    let path_str = path.as_ref().to_str().ok_or_else(|| {
        GmeOrIoError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid UTF-8 in path",
        ))
    })?;
    let c_path = CString::new(path_str).map_err(|_| {
        GmeOrIoError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Path contains null byte",
        ))
    })?;
    unsafe {
        let mut emu_ptr: *const MusicEmu = std::ptr::null();
        let err = gme_open_file(c_path.as_ptr(), &mut emu_ptr, sample_rate as i32);
        if !err.is_null() {
            let msg = CStr::from_ptr(err).to_string_lossy().to_string();
            return Err(GmeOrIoError::Gme(GmeError::new(msg)));
        }
        if emu_ptr.is_null() {
            return Err(GmeOrIoError::Gme(GmeError::new("gme_open_file returned null".to_string())));
        }

        // Auto-load matching .m3u playlist (same base name, same directory)
        // NOTE: gme_load_m3u (file path) is broken in our GME fork (Std_File_Reader issue).
        // Use gme_load_m3u_data (from memory) instead, which works correctly.
        let m3u_path = path.as_ref().with_extension("m3u");
        if m3u_path.exists() {
            if let Ok(m3u_data) = std::fs::read(&m3u_path) {
                let _ = gme_load_m3u_data(
                    emu_ptr,
                    m3u_data.as_ptr() as *const std::ffi::c_void,
                    m3u_data.len() as std::ffi::c_long,
                );
            }
        }

        Ok(EmuHandle::new(emu_ptr))
    }
}

pub(crate) fn play(handle: &EmuHandle, count: usize, buffer: &mut [i16]) -> Result<(), GmeError> {
    unsafe { process_result(gme_play(handle.to_raw(), count as i32, buffer.as_mut_ptr())) }
}

pub(crate) fn start_track(handle: &EmuHandle, index: u32) -> GmeResult<()> {
    unsafe { process_result(gme_start_track(handle.to_raw(), index as i32)) }
}

pub(crate) fn tell(handle: &EmuHandle) -> u32 {
    unsafe { gme_tell(handle.to_raw()) as u32 }
}

pub(crate) fn track_count(handle: &EmuHandle) -> usize {
    unsafe { gme_track_count(handle.to_raw()) as usize }
}

pub(crate) fn track_ended(handle: &EmuHandle) -> bool {
    unsafe { gme_track_ended(handle.to_raw()) }
}

/// Rust-friendly track info struct
pub struct TrackInfo {
    pub length: i32,
    pub intro_length: i32,
    pub loop_length: i32,
    pub play_length: i32,
    pub system: String,
    pub game: String,
    pub song: String,
    pub author: String,
    pub copyright: String,
    pub comment: String,
    pub dumper: String,
}

unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    }
}

pub(crate) fn track_info(handle: &EmuHandle, track: usize) -> GmeResult<TrackInfo> {
    unsafe {
        let mut info_ptr: *mut gme_info_t = std::ptr::null_mut();
        let result = gme_track_info(handle.to_raw(), &mut info_ptr, track as i32);
        process_result(result)?;

        let info = &*info_ptr;
        let track_info = TrackInfo {
            length: info.length,
            intro_length: info.intro_length,
            loop_length: info.loop_length,
            play_length: info.play_length,
            system: cstr_to_string(info.system),
            game: cstr_to_string(info.game),
            song: cstr_to_string(info.song),
            author: cstr_to_string(info.author),
            copyright: cstr_to_string(info.copyright),
            comment: cstr_to_string(info.comment),
            dumper: cstr_to_string(info.dumper),
        };

        gme_free_info(info_ptr);
        Ok(track_info)
    }
}

pub(crate) fn seek(handle: &EmuHandle, msec: i32) -> GmeResult<()> {
    unsafe { process_result(gme_seek(handle.to_raw(), msec)) }
}

pub(crate) fn set_fade(handle: &EmuHandle, start_msec: i32) {
    unsafe { gme_set_fade(handle.to_raw(), start_msec) }
}

pub(crate) fn set_tempo(handle: &EmuHandle, tempo: f64) {
    unsafe { gme_set_tempo(handle.to_raw(), tempo) }
}

pub(crate) fn ignore_silence(handle: &EmuHandle, ignore: bool) {
    unsafe { gme_ignore_silence(handle.to_raw(), if ignore { 1 } else { 0 }) }
}

/// Returns all of the supported `EmuTypes`. This is based on the features the crate is compiled
/// with.
pub fn type_list() -> Vec<EmuType> {
    let mut types = Vec::new();
    unsafe {
        let mut p = gme_type_list();
        while *p != std::ptr::null() {
            let gme_type = p.clone().read();
            let extension = CStr::from_ptr((*gme_type).extension).to_str().unwrap();
            types.push(EmuType::from_extension(extension));
            p = p.offset(1);
        }
    }
    types
}

fn process_result(result: *const c_char) -> GmeResult<()> {
    if result.is_null() {
        Ok(())
    } else {
        unsafe {
            Err(GmeError::new(
                CStr::from_ptr(result).to_str().unwrap().to_string(),
            ))
        }
    }
}

pub(crate) fn get_file_data(path: impl AsRef<Path>) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

#[repr(C)]
#[derive(Clone)]
pub(crate) struct MusicEmu {
    _private: isize,
}

/// C struct for track information returned by gme_track_info
#[repr(C)]
pub(crate) struct gme_info_t {
    pub length: i32,
    pub intro_length: i32,
    pub loop_length: i32,
    pub play_length: i32,
    _i4: i32, _i5: i32, _i6: i32, _i7: i32,
    _i8: i32, _i9: i32, _i10: i32, _i11: i32,
    _i12: i32, _i13: i32, _i14: i32, _i15: i32,
    pub system: *const c_char,
    pub game: *const c_char,
    pub song: *const c_char,
    pub author: *const c_char,
    pub copyright: *const c_char,
    pub comment: *const c_char,
    pub dumper: *const c_char,
    _s7: *const c_char, _s8: *const c_char, _s9: *const c_char,
    _s10: *const c_char, _s11: *const c_char, _s12: *const c_char,
    _s13: *const c_char, _s14: *const c_char, _s15: *const c_char,
}

// gme_type_t_ is struct
// gme_type_t holds pointer to other
#[repr(C)]
pub(crate) struct gme_type_t_struct {
    /// name of system this music file type is generally for
    pub system: *const c_char,
    /// non-zero for formats with a fixed number of tracks
    track_count: i32,
    /// Create new emulator for this type (useful in C++ only)
    new_emu: *const isize,
    /// Create new info reader for this type
    new_info: *const isize,

    pub extension: *const c_char,
    /// internal
    flags: i32,
}

//#[derive(Debug, Copy, Clone)]
#[allow(non_camel_case_types)]
type gme_type_t = *const gme_type_t_struct;

unsafe extern "C" {
    /// Finish using emulator and free memory
    fn gme_delete(emu: *const MusicEmu);

    /// Open a music file from a filesystem path. Automatically loads matching .m3u
    /// playlist (for track names, durations, and ordering).
    fn gme_open_file(path: *const c_char, out: *mut *const MusicEmu, sample_rate: i32) -> *const c_char;

    /// Determine likely game music type based on first four bytes of file. Returns string
    /// containing proper file suffix (i.e. "NSF", "SPC", etc.) or "" if file header is not
    /// recognized.
    fn gme_identify_header(header: *const u8) -> *const c_char;

    /// Get corresponding music type for file path or extension passed in.
    fn gme_identify_extension(extension: *const c_char) -> *const gme_type_t;

    /// Load music file from memory into emulator. Makes a copy of data passed.
    fn gme_load_data(emu: *const MusicEmu, data: *const u8, size: usize) -> *const c_char;

    /// Generate `count` 16-bit signed samples into `buffer`. Output is in stereo.
    fn gme_play(emu: *const MusicEmu, count: i32, out: *mut i16) -> *const c_char;

    /// Create new emulator and set sample rate.
    fn gme_new_emu(gme_type: *const gme_type_t, sample_rate: i32) -> *const MusicEmu;

    /// Start a track, where 0 is the first track
    fn gme_start_track(emu: *const MusicEmu, index: i32) -> *const c_char;

    /// Number of milliseconds played since beginning of track
    fn gme_tell(emu: *const MusicEmu) -> i32;

    /// Number of tracks available
    fn gme_track_count(emu: *const MusicEmu) -> i32;

    /// True if a track has reached its end
    fn gme_track_ended(emu: *const MusicEmu) -> bool;

    /// Pointer to array of all music types, with NULL entry at end.
    fn gme_type_list() -> *const gme_type_t;

    /// Get track information (length, name, author, etc.). Must be freed with gme_free_info.
    fn gme_track_info(emu: *const MusicEmu, out: *mut *mut gme_info_t, track: i32) -> *const c_char;

    /// Free track information
    fn gme_free_info(info: *mut gme_info_t);

    /// Seek to new time in track (milliseconds)
    fn gme_seek(emu: *const MusicEmu, msec: i32) -> *const c_char;

    /// Set time to start fading track out (milliseconds)
    fn gme_set_fade(emu: *const MusicEmu, start_msec: i32);

    /// Adjust song tempo (1.0 = normal, 0.5 = half, 2.0 = double)
    fn gme_set_tempo(emu: *const MusicEmu, tempo: f64);

    /// Disable automatic end-of-track detection and skipping of silence
    fn gme_ignore_silence(emu: *const MusicEmu, ignore: i32);

    /// Load m3u playlist file (must be done after loading music)
    fn gme_load_m3u(emu: *const MusicEmu, path: *const c_char) -> *const c_char;

    /// Load m3u playlist from memory (must be done after loading music)
    fn gme_load_m3u_data(emu: *const MusicEmu, data: *const std::ffi::c_void, size: std::ffi::c_long) -> *const c_char;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_get_types() {
        let types = type_list();
        assert!(types.len() > 1);
    }

    #[test]
    fn test_open_data() {
        let handle = open_data(&get_test_nsf_data(), 44100).unwrap();
        assert_eq!(track_count(&handle), 1);
        start_track(&handle, 0).unwrap();
    }

    #[test]
    fn test_open_file() {
        let handle = open_file(TEST_NSF_PATH, 44100).unwrap();
        assert_eq!(track_count(&handle), 1);
        start_track(&handle, 0).unwrap();
    }
}
