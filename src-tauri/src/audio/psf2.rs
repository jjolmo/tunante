use hepsf_rs::Psf2Decoder;
use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::Duration;

/// Default play duration when PSF tags don't specify a length (2.5 minutes)
const DEFAULT_DURATION_MS: u64 = 150_000;
/// Default fade duration when not specified in tags
const DEFAULT_FADE_MS: u64 = 10_000;
/// Sample rate for PS2 IOP audio output.
/// PS2 IOP clock = 36,864,000 Hz / 768 cycles per sample = 48,000 Hz.
/// (PS1 uses 33,868,800 / 768 = 44,100 Hz — different!)
const SAMPLE_RATE: u32 = 48000;
/// Decode chunk size in stereo frames
const CHUNK_FRAMES: usize = 1024;
/// Larger chunk size for seek fast-forward (less overhead per call).
/// Bigger = fewer render calls = faster seeking.
const SEEK_CHUNK_FRAMES: usize = 16384;

/// rodio::Source implementation wrapping Highly Experimental for PSF2/minipsf2 playback.
/// Emulates PS2 IOP (R3000 CPU + SPU2) to decode PlayStation 2 music.
pub struct Psf2Source {
    decoder: Psf2Decoder,
    buffer: Vec<f32>,
    buf_pos: usize,
    total_duration: Option<Duration>,
    /// Current PCM frame position (for fade calculation)
    frame_no: u64,
    /// Total frames to render (length + fade)
    frame_total: u64,
    /// Frame at which fade begins
    frame_fade: u64,
    finished: bool,
    /// Path for restart-based seeking
    path: std::path::PathBuf,
}

// Safety: Psf2Source is moved into Player::append() and used exclusively on the audio thread.
unsafe impl Send for Psf2Source {}

impl Psf2Source {
    /// Create a new Psf2Source for a PSF2/minipsf2 file.
    ///
    /// Loads the PSF2 chain, initializes the HE IOP emulator with the PS2 BIOS,
    /// populates the psf2fs virtual filesystem, and prepares for streaming PCM output.
    ///
    /// Uses catch_unwind to prevent panics in the C FFI layer from crashing
    /// the entire application.
    pub fn new(path: &Path) -> Result<Self, String> {
        let path_owned = path.to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| {
            Psf2Decoder::new(&path_owned)
        }));

        let (decoder, tags) = match result {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => return Err(format!("PSF2 load error: {}", e)),
            Err(panic) => {
                let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                return Err(format!("PSF2 load crashed: {}", msg));
            }
        };

        // Duration from PSF tags, or defaults
        let length_ms = if tags.length_ms > 0 {
            tags.length_ms
        } else {
            DEFAULT_DURATION_MS
        };
        let fade_ms = if tags.fade_ms > 0 {
            tags.fade_ms
        } else {
            DEFAULT_FADE_MS
        };

        let total_ms = length_ms + fade_ms;

        // Convert to frames
        let frame_fade = length_ms * SAMPLE_RATE as u64 / 1000;
        let frame_total = total_ms * SAMPLE_RATE as u64 / 1000;

        Ok(Self {
            decoder,
            buffer: Vec::new(),
            buf_pos: 0,
            total_duration: Some(Duration::from_millis(total_ms)),
            frame_no: 0,
            frame_total,
            frame_fade,
            finished: false,
            path: path.to_path_buf(),
        })
    }

    /// Decode the next chunk of audio, applying fade-out as needed.
    ///
    /// Wraps the C FFI render call in catch_unwind so that a panic in the
    /// IOP emulator doesn't kill the audio thread — it just ends the stream.
    fn decode_next_chunk(&mut self) -> bool {
        if self.frame_no >= self.frame_total {
            self.finished = true;
            return false;
        }

        // How many frames to render this chunk
        let remaining = self.frame_total - self.frame_no;
        let frames_to_render = CHUNK_FRAMES.min(remaining as usize);

        // Render i16 stereo samples — wrapped in catch_unwind to survive C crashes
        let mut i16_buf = vec![0i16; frames_to_render * 2];
        let render_result = catch_unwind(AssertUnwindSafe(|| {
            self.decoder.render(&mut i16_buf, frames_to_render);
        }));

        if render_result.is_err() {
            log::error!("PSF2 render panic — ending stream gracefully");
            self.finished = true;
            return false;
        }

        // Convert i16 → f32 and apply fade-out
        self.buffer.clear();
        self.buffer.reserve(frames_to_render * 2);

        for i in 0..frames_to_render {
            let global_frame = self.frame_no + i as u64;
            let mut left = i16_buf[i * 2] as f32 / 32768.0;
            let mut right = i16_buf[i * 2 + 1] as f32 / 32768.0;

            if global_frame >= self.frame_total {
                left = 0.0;
                right = 0.0;
            } else if global_frame >= self.frame_fade {
                // In the fade region — apply quadratic fade curve
                let fade_progress = (self.frame_total - global_frame) as f32
                    / (self.frame_total - self.frame_fade) as f32;
                let fade = fade_progress * fade_progress;
                left *= fade;
                right *= fade;
            }

            self.buffer.push(left);
            self.buffer.push(right);
        }

        self.frame_no += frames_to_render as u64;
        self.buf_pos = 0;
        true
    }
}

impl Iterator for Psf2Source {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.finished && self.buf_pos >= self.buffer.len() {
            return None;
        }
        if self.buf_pos >= self.buffer.len() {
            if !self.decode_next_chunk() {
                return None;
            }
        }
        let sample = self.buffer[self.buf_pos];
        self.buf_pos += 1;
        Some(sample)
    }
}

impl Source for Psf2Source {
    fn current_span_len(&self) -> Option<usize> {
        if self.buf_pos < self.buffer.len() {
            Some(self.buffer.len() - self.buf_pos)
        } else {
            None
        }
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(2).unwrap() // PS2 IOP audio is stereo
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        // PSF2 doesn't support arbitrary seek — re-init from scratch and fast-forward.
        // The IOP emulator doesn't support random access, so we re-initialize
        // and render (discard) audio until we reach the target position.
        let target_frame = (pos.as_millis() as u64 * SAMPLE_RATE as u64) / 1000;

        // Re-create decoder from the file
        let result = catch_unwind(AssertUnwindSafe(|| {
            Psf2Decoder::new(&self.path)
        }));

        let (new_decoder, _tags) = match result {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                return Err(SeekError::Other(std::sync::Arc::new(
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("PSF2 seek re-init error: {}", e),
                    ),
                )));
            }
            Err(_panic) => {
                log::error!("PSF2 seek panic — stream may be in bad state");
                return Err(SeekError::NotSupported {
                    underlying_source: "PSF2 seek crashed",
                });
            }
        };

        self.decoder = new_decoder;
        self.frame_no = 0;
        self.buffer.clear();
        self.buf_pos = 0;
        self.finished = false;

        // Fast-forward by rendering and discarding frames.
        // Single catch_unwind for the entire loop to minimize overhead.
        let ff_result = catch_unwind(AssertUnwindSafe(|| {
            let mut throwaway = vec![0i16; SEEK_CHUNK_FRAMES * 2];
            let mut remaining = target_frame;
            while remaining > 0 {
                let skip = SEEK_CHUNK_FRAMES.min(remaining as usize);
                self.decoder.render(&mut throwaway[..skip * 2], skip);
                remaining -= skip as u64;
            }
        }));

        if ff_result.is_err() {
            log::error!("PSF2 seek fast-forward panic");
        }

        self.frame_no = target_frame;
        Ok(())
    }
}
