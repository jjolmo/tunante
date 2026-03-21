use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use vio2sf_rs::TwoSfDecoder;

/// Default play duration when PSF tags don't specify a length (2.5 minutes)
const DEFAULT_DURATION_MS: u64 = 150_000;
/// Default fade duration when not specified in tags
const DEFAULT_FADE_MS: u64 = 10_000;
/// Sample rate for NDS audio output
const SAMPLE_RATE: u32 = 44100;
/// Decode chunk size in stereo frames
const CHUNK_FRAMES: usize = 1024;
/// Larger chunk size for seek fast-forward (less overhead per call)
const SEEK_CHUNK_FRAMES: usize = 16384;

/// rodio::Source implementation wrapping vio2sf for 2SF/mini2sf playback.
/// Emulates NDS audio hardware via DeSmuME to decode Nintendo DS music.
pub struct TwoSfSource {
    decoder: TwoSfDecoder,
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

// Safety: TwoSfSource is moved into Player::append() and used exclusively on the audio thread.
unsafe impl Send for TwoSfSource {}

impl TwoSfSource {
    /// Create a new TwoSfSource for a 2SF/mini2sf file.
    ///
    /// Loads the PSF chain (mini2sf → 2sflib), initializes the DeSmuME emulator,
    /// and prepares for streaming PCM output.
    pub fn new(path: &Path) -> Result<Self, String> {
        let (decoder, tags) =
            TwoSfDecoder::new(path).map_err(|e| format!("2SF load error: {}", e))?;

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
    fn decode_next_chunk(&mut self) -> bool {
        if self.frame_no >= self.frame_total {
            self.finished = true;
            return false;
        }

        // How many frames to render this chunk
        let remaining = self.frame_total - self.frame_no;
        let frames_to_render = CHUNK_FRAMES.min(remaining as usize);

        // Render i16 stereo samples
        let mut i16_buf = vec![0i16; frames_to_render * 2];
        self.decoder.render(&mut i16_buf, frames_to_render);

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

impl Iterator for TwoSfSource {
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

impl Source for TwoSfSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.buf_pos < self.buffer.len() {
            Some(self.buffer.len() - self.buf_pos)
        } else {
            None
        }
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(2).unwrap() // NDS audio is stereo
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        // 2SF doesn't support arbitrary seek — re-init from scratch and fast-forward
        let target_frame = (pos.as_millis() as u64 * SAMPLE_RATE as u64) / 1000;

        // Re-create decoder from the file
        let (new_decoder, _tags) = TwoSfDecoder::new(&self.path).map_err(|e| {
            SeekError::Other(Arc::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("2SF seek re-init error: {}", e),
            )))
        })?;

        self.decoder = new_decoder;
        self.frame_no = 0;
        self.buffer.clear();
        self.buf_pos = 0;
        self.finished = false;

        // === Seek optimizations ===
        // 1. Mute all 16 SPU channels — skips expensive resampling/fetch
        self.decoder.set_channel_mute(0xFFFF);
        // 2. Slow down ARM9 (game logic CPU) — it mostly idles during music playback.
        //    ARM7 (sound driver CPU) stays at full speed for correct sequencer timing.
        //    +2 means ARM9 runs at 1/4 speed, saving ~40% of total CPU work.
        let saved_arm9_cd = self.decoder.arm9_clockdown();
        self.decoder.set_arm9_clockdown(saved_arm9_cd + 2);

        // Fast-forward by rendering and discarding frames
        let mut throwaway = vec![0i16; SEEK_CHUNK_FRAMES * 2];
        let mut remaining = target_frame;
        while remaining > 0 {
            let skip = SEEK_CHUNK_FRAMES.min(remaining as usize);
            self.decoder.render(&mut throwaway[..skip * 2], skip);
            remaining -= skip as u64;
        }

        // Restore normal operation
        self.decoder.set_arm9_clockdown(saved_arm9_cd);
        self.decoder.set_channel_mute(0);

        self.frame_no = target_frame;
        Ok(())
    }
}
