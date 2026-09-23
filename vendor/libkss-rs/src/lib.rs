//! Bindings for the vendored libkss: KSS (MSX) playback.
//!
//! Why this exists rather than GME's KSS backend, which also compiles and also
//! claims the format: see `../libkss/README.upstream.md`. The short version is
//! that GME emulates six bytes of MSX BIOS, real Konami drivers expect a real
//! MSX, and the result is silence.
//!
//! The surface is deliberately small — open, pick a song, render samples, fade,
//! ask whether it ended — because that is all a decoder needs.

use std::ffi::{c_char, c_int, CString};
use std::sync::Mutex;

#[repr(C)]
struct KssRaw {
    _private: [u8; 0],
}

#[repr(C)]
struct KssPlayRaw {
    _private: [u8; 0],
}

extern "C" {
    fn KSS_bin2kss(data: *mut u8, size: u32, filename: *const c_char) -> *mut KssRaw;
    fn KSS_delete(kss: *mut KssRaw);
    fn KSSPLAY_new(rate: u32, nch: u32, bps: u32) -> *mut KssPlayRaw;
    fn KSSPLAY_set_data(play: *mut KssPlayRaw, kss: *mut KssRaw) -> c_int;
    fn KSSPLAY_reset(play: *mut KssPlayRaw, song: u32, cpu_speed: u32);
    fn KSSPLAY_calc(play: *mut KssPlayRaw, buf: *mut i16, length: u32);
    fn KSSPLAY_fade_start(play: *mut KssPlayRaw, fade_time: u32);
    fn KSSPLAY_get_fade_flag(play: *mut KssPlayRaw) -> c_int;
    fn KSSPLAY_get_stop_flag(play: *mut KssPlayRaw) -> c_int;
    fn KSSPLAY_get_loop_count(play: *mut KssPlayRaw) -> c_int;
    fn KSSPLAY_set_silent_limit(play: *mut KssPlayRaw, time_in_ms: u32);
    fn KSSPLAY_set_master_volume(play: *mut KssPlayRaw, vol: i32);
    fn KSSPLAY_delete(play: *mut KssPlayRaw);
    fn tunante_kss_track_min(kss: *const KssRaw) -> c_int;
    fn tunante_kss_track_max(kss: *const KssRaw) -> c_int;
}

/// The fade is over and the player is producing nothing but zeroes.
const FADE_END: c_int = 2;

/// Serialises construction only.
///
/// The chip emulators build shared lookup tables the first time one of them is
/// created, and nothing upstream guards that. Rendering afterwards touches only
/// per-instance state, so this is held across `open` and released before a
/// single sample is produced.
static CONSTRUCTION: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KssError {
    /// The bytes are not a KSS file libkss will take.
    NotKss,
    /// libkss could not allocate its player.
    OutOfMemory,
    /// The path had an interior NUL, so it could not be handed to C.
    BadName,
}

impl std::fmt::Display for KssError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KssError::NotKss => "not a KSS file",
            KssError::OutOfMemory => "libkss could not allocate a player",
            KssError::BadName => "file name cannot be passed to C",
        })
    }
}

impl std::error::Error for KssError {}

/// One loaded file and the player reading it.
///
/// They are one type because libkss's player borrows the loaded file for its
/// whole life: keeping them apart would be a lifetime that FFI cannot check.
pub struct KssPlayer {
    play: *mut KssPlayRaw,
    kss: *mut KssRaw,
    channels: u16,
}

// Safety: every pointer here is owned solely by this value, and there is no
// `Sync` — two threads cannot touch one player. The one piece of shared state
// upstream has, the chip emulators' lookup tables, is built under
// `CONSTRUCTION` and only read afterwards.
unsafe impl Send for KssPlayer {}

impl KssPlayer {
    /// Load `data` and start `song`.
    ///
    /// `song` is the number the sound driver knows, not a position in any list:
    /// a Metal Gear 2 rip numbers its songs `$99`, `$9A`, `$6A`, and a `.m3u`
    /// beside the file is what maps a track to one of them.
    ///
    /// `name` only reaches libkss's format sniffing, which looks at the
    /// extension; the bytes decide everything else.
    pub fn open(data: &[u8], name: &str, rate: u32, channels: u16, song: u32) -> Result<Self, KssError> {
        let cname = CString::new(name).map_err(|_| KssError::BadName)?;
        // `KSS_bin2kss` takes a mutable pointer and keeps what it needs, so it
        // gets a copy this call owns rather than the caller's buffer.
        let mut owned = data.to_vec();

        let _guard = CONSTRUCTION.lock().unwrap_or_else(|e| e.into_inner());
        let kss = unsafe { KSS_bin2kss(owned.as_mut_ptr(), owned.len() as u32, cname.as_ptr()) };
        if kss.is_null() {
            return Err(KssError::NotKss);
        }
        let play = unsafe { KSSPLAY_new(rate, channels as u32, 16) };
        if play.is_null() {
            unsafe { KSS_delete(kss) };
            return Err(KssError::OutOfMemory);
        }
        unsafe {
            KSSPLAY_set_data(play, kss);
            // 0 means "the speed the file asks for".
            KSSPLAY_reset(play, song, 0);
        }
        Ok(Self { play, kss, channels })
    }

    /// Fill `out` with interleaved samples. Its length must be a whole number
    /// of frames; any remainder is left untouched.
    pub fn render(&mut self, out: &mut [i16]) {
        let frames = out.len() / self.channels as usize;
        if frames == 0 {
            return;
        }
        unsafe { KSSPLAY_calc(self.play, out.as_mut_ptr(), frames as u32) };
    }

    /// Begin a fade of `ms` milliseconds. [`Self::ended`] reports when it is done.
    pub fn fade_start(&mut self, ms: u32) {
        unsafe { KSSPLAY_fade_start(self.play, ms) };
    }

    /// How many times the song has looped so far.
    pub fn loop_count(&self) -> i32 {
        unsafe { KSSPLAY_get_loop_count(self.play) as i32 }
    }

    /// The song has stopped, or its fade has finished. Either way there is
    /// nothing left to render.
    pub fn ended(&self) -> bool {
        unsafe { KSSPLAY_get_stop_flag(self.play) != 0 || KSSPLAY_get_fade_flag(self.play) == FADE_END }
    }

    /// Set the output gain, in decibels.
    ///
    /// libkss counts volume in steps of 6/32 dB and clamps the total to
    /// -48..+48 dB, so the decibels are converted here rather than leaking that
    /// unit into callers.
    pub fn set_gain_db(&mut self, db: f32) {
        const STEPS_PER_DB: f32 = 32.0 / 6.0;
        let steps = (db * STEPS_PER_DB).round().clamp(-256.0, 255.0) as i32;
        unsafe { KSSPLAY_set_master_volume(self.play, steps) };
    }

    /// How much unbroken silence counts as the end of a song. Zero disables it.
    pub fn set_silent_limit(&mut self, ms: u32) {
        unsafe { KSSPLAY_set_silent_limit(self.play, ms) };
    }

    /// The range of song numbers the file declares. A plain `KSCC` rip claims
    /// the whole 0..=255, which is why a `.m3u` is worth more than this.
    pub fn song_range(&self) -> (u8, u8) {
        unsafe {
            (
                tunante_kss_track_min(self.kss) as u8,
                tunante_kss_track_max(self.kss) as u8,
            )
        }
    }
}

impl Drop for KssPlayer {
    fn drop(&mut self) {
        unsafe {
            KSSPLAY_delete(self.play);
            KSS_delete(self.kss);
        }
    }
}
