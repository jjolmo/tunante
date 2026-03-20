pub mod ffi;

use std::ffi::{CStr, CString};
use std::path::Path;

/// Stream info extracted after opening a file
#[derive(Debug, Clone)]
pub struct VgmstreamInfo {
    pub channels: i32,
    pub sample_rate: i32,
    pub stream_samples: i64,
    pub play_samples: i64,
    pub loop_flag: bool,
    pub loop_start: i64,
    pub loop_end: i64,
    pub subsong_index: i32,
    pub subsong_count: i32,
    pub codec_name: String,
    pub stream_name: String,
    pub meta_name: String,
    pub bitrate: i32,
}

/// Safe wrapper around libvgmstream
pub struct Vgmstream {
    lib: *mut ffi::libvgmstream_t,
}

// vgmstream is not thread-safe per instance, but we ensure single-thread access
unsafe impl Send for Vgmstream {}

impl Vgmstream {
    /// Open a file with default config (loop 2x + 10s fade, PCM16 output)
    pub fn open(path: &Path, subsong: i32) -> Result<Self, String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| "Invalid path encoding".to_string())?;
        let c_path = CString::new(path_str).map_err(|e| e.to_string())?;

        unsafe {
            // Silence vgmstream logging
            ffi::libvgmstream_set_log(ffi::LIBVGMSTREAM_LOG_LEVEL_NONE, None);

            let lib = ffi::libvgmstream_init();
            if lib.is_null() {
                return Err("Failed to init vgmstream".to_string());
            }

            // Configure: 2 loops + 10s fade, force PCM16 output
            let mut cfg: ffi::libvgmstream_config_t = std::mem::zeroed();
            cfg.loop_count = 2.0;
            cfg.fade_time = 10.0;
            cfg.fade_delay = 0.0;
            cfg.force_sfmt = ffi::LIBVGMSTREAM_SFMT_PCM16;
            ffi::libvgmstream_setup(lib, &mut cfg);

            // Open via stdio streamfile
            let sf = ffi::libstreamfile_open_from_stdio(c_path.as_ptr());
            if sf.is_null() {
                ffi::libvgmstream_free(lib);
                return Err(format!("Failed to open streamfile: {}", path_str));
            }

            let ret = ffi::libvgmstream_open_stream(lib, sf, subsong);
            ffi::libstreamfile_close(sf);

            if ret < 0 {
                ffi::libvgmstream_free(lib);
                return Err(format!("Unsupported format or invalid file: {}", path_str));
            }

            Ok(Vgmstream { lib })
        }
    }

    /// Get stream format info
    pub fn info(&self) -> VgmstreamInfo {
        unsafe {
            let fmt = &*(*self.lib).format;
            VgmstreamInfo {
                channels: fmt.channels,
                sample_rate: fmt.sample_rate,
                stream_samples: fmt.stream_samples,
                play_samples: fmt.play_samples,
                loop_flag: fmt.loop_flag,
                loop_start: fmt.loop_start,
                loop_end: fmt.loop_end,
                subsong_index: fmt.subsong_index,
                subsong_count: fmt.subsong_count,
                codec_name: cchar_to_string(&fmt.codec_name),
                stream_name: cchar_to_string(&fmt.stream_name),
                meta_name: cchar_to_string(&fmt.meta_name),
                bitrate: fmt.stream_bitrate,
            }
        }
    }

    /// Render next batch of samples. Returns (buffer_of_i16_samples, done_flag).
    /// Buffer is interleaved: [ch0_s0, ch1_s0, ch0_s1, ch1_s1, ...]
    pub fn render(&mut self) -> Result<(&[i16], bool), String> {
        unsafe {
            let ret = ffi::libvgmstream_render(self.lib);
            if ret < 0 {
                return Err("Render error".to_string());
            }
            let dec = &*(*self.lib).decoder;
            let sample_count = dec.buf_bytes as usize / 2; // PCM16 = 2 bytes per sample
            let buf =
                std::slice::from_raw_parts(dec.buf as *const i16, sample_count);
            Ok((buf, dec.done))
        }
    }

    /// Seek to absolute sample position
    pub fn seek(&mut self, sample: i64) {
        unsafe {
            ffi::libvgmstream_seek(self.lib, sample);
        }
    }

    /// Get current play position in samples
    pub fn position(&self) -> i64 {
        unsafe { ffi::libvgmstream_get_play_position(self.lib) }
    }

    /// Get list of all supported extensions
    pub fn extensions() -> Vec<String> {
        unsafe {
            let mut size: std::os::raw::c_int = 0;
            let exts = ffi::libvgmstream_get_extensions(&mut size);
            if exts.is_null() || size <= 0 {
                return Vec::new();
            }
            let mut result = Vec::with_capacity(size as usize);
            for i in 0..size as isize {
                let ext_ptr = *exts.offset(i);
                if !ext_ptr.is_null() {
                    if let Ok(s) = CStr::from_ptr(ext_ptr).to_str() {
                        result.push(s.to_string());
                    }
                }
            }
            result
        }
    }

    /// Check if a filename/extension is supported by vgmstream
    pub fn is_valid(filename: &str) -> bool {
        let c_name = match CString::new(filename) {
            Ok(c) => c,
            Err(_) => return false,
        };
        unsafe { ffi::libvgmstream_is_valid(c_name.as_ptr(), std::ptr::null_mut()) }
    }
}

impl Drop for Vgmstream {
    fn drop(&mut self) {
        unsafe {
            ffi::libvgmstream_free(self.lib);
        }
    }
}

fn cchar_to_string(buf: &[std::os::raw::c_char]) -> String {
    unsafe {
        CStr::from_ptr(buf.as_ptr())
            .to_str()
            .unwrap_or("")
            .to_string()
    }
}
