///! Raw FFI bindings for libvgmstream
use std::os::raw::{c_char, c_int, c_void};

// Sample format enum
pub const LIBVGMSTREAM_SFMT_PCM16: c_int = 1;
pub const LIBVGMSTREAM_SFMT_FLOAT: c_int = 4;

// Log levels
pub const LIBVGMSTREAM_LOG_LEVEL_NONE: c_int = 100;

#[repr(C)]
pub struct libvgmstream_format_t {
    pub channels: c_int,
    pub sample_rate: c_int,
    pub sample_format: c_int,
    pub sample_size: c_int,
    pub channel_layout: u32,

    pub subsong_index: c_int,
    pub subsong_count: c_int,

    pub input_channels: c_int,

    pub stream_samples: i64,
    pub loop_start: i64,
    pub loop_end: i64,
    pub loop_flag: bool,

    pub play_forever: bool,

    pub play_samples: i64,

    pub stream_bitrate: c_int,

    pub codec_name: [c_char; 128],
    pub layout_name: [c_char; 128],
    pub meta_name: [c_char; 128],
    pub stream_name: [c_char; 256],

    pub format_id: c_int,
}

#[repr(C)]
pub struct libvgmstream_decoder_t {
    pub buf: *mut c_void,
    pub buf_samples: c_int,
    pub buf_bytes: c_int,
    pub done: bool,
}

#[repr(C)]
pub struct libvgmstream_t {
    pub priv_: *mut c_void,
    pub format: *const libvgmstream_format_t,
    pub decoder: *mut libvgmstream_decoder_t,
}

#[repr(C)]
pub struct libvgmstream_config_t {
    pub disable_config_override: bool,
    pub allow_play_forever: bool,
    pub play_forever: bool,
    pub ignore_loop: bool,
    pub force_loop: bool,
    pub really_force_loop: bool,
    pub ignore_fade: bool,
    pub loop_count: f64,
    pub fade_time: f64,
    pub fade_delay: f64,
    pub stereo_track: c_int,
    pub auto_downmix_channels: c_int,
    pub force_sfmt: c_int,
}

#[repr(C)]
pub struct libstreamfile_t {
    pub user_data: *mut c_void,
    pub read: Option<unsafe extern "C" fn(*mut c_void, *mut u8, i64, c_int) -> c_int>,
    pub get_size: Option<unsafe extern "C" fn(*mut c_void) -> i64>,
    pub get_name: Option<unsafe extern "C" fn(*mut c_void) -> *const c_char>,
    pub open: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut libstreamfile_t,
    >,
    pub close: Option<unsafe extern "C" fn(*mut libstreamfile_t)>,
}

unsafe extern "C" {
    // Lifecycle
    pub fn libvgmstream_init() -> *mut libvgmstream_t;
    pub fn libvgmstream_free(lib: *mut libvgmstream_t);

    // Config
    pub fn libvgmstream_setup(lib: *mut libvgmstream_t, cfg: *mut libvgmstream_config_t);

    // Stream
    pub fn libvgmstream_open_stream(
        lib: *mut libvgmstream_t,
        libsf: *mut libstreamfile_t,
        subsong: c_int,
    ) -> c_int;
    pub fn libvgmstream_close_stream(lib: *mut libvgmstream_t);

    // Decode
    pub fn libvgmstream_render(lib: *mut libvgmstream_t) -> c_int;

    // Seek
    pub fn libvgmstream_get_play_position(lib: *mut libvgmstream_t) -> i64;
    pub fn libvgmstream_seek(lib: *mut libvgmstream_t, sample: i64);
    pub fn libvgmstream_reset(lib: *mut libvgmstream_t);

    // Helpers
    pub fn libvgmstream_get_extensions(size: *mut c_int) -> *const *const c_char;
    pub fn libvgmstream_is_valid(
        filename: *const c_char,
        cfg: *mut c_void,
    ) -> bool;
    pub fn libvgmstream_set_log(
        level: c_int,
        callback: Option<unsafe extern "C" fn(c_int, *const c_char)>,
    );

    // Streamfile
    pub fn libstreamfile_open_from_stdio(filename: *const c_char) -> *mut libstreamfile_t;
    pub fn libstreamfile_close(libsf: *mut libstreamfile_t);
}
