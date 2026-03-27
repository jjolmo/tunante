use lazyusf2_rs::UsfDecoder;
use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Default play duration when PSF tags don't specify a length (2.5 minutes)
const DEFAULT_DURATION_MS: u64 = 150_000;
/// Default fade duration when not specified in tags
const DEFAULT_FADE_MS: u64 = 10_000;
/// Sample rate for output audio.
const SAMPLE_RATE: u32 = 44100;
/// Decode chunk size in stereo frames.
const CHUNK_FRAMES: usize = 4096;
/// Larger chunk size for seek fast-forward
const SEEK_CHUNK_FRAMES: usize = 8192;

/// rodio::Source implementation wrapping lazyusf2 for USF/miniusf playback.
pub struct UsfSource {
    decoder: UsfDecoder,
    buffer: Vec<f32>,
    buf_pos: usize,
    total_duration: Option<Duration>,
    frame_no: u64,
    frame_total: u64,
    frame_fade: u64,
    finished: bool,
}

unsafe impl Send for UsfSource {}

impl UsfSource {
    pub fn new(path: &Path) -> Result<Self, String> {
        let (decoder, tags) =
            UsfDecoder::new(path, SAMPLE_RATE).map_err(|e| format!("USF load error: {}", e))?;

        let length_ms = if tags.length_ms > 0 {
            tags.length_ms
        } else {
            DEFAULT_DURATION_MS
        };
        let fade_ms = if tags.fade_ms > 0 {
            tags.fade_ms
        } else if tags.length_ms > 0 {
            DEFAULT_FADE_MS
        } else {
            DEFAULT_FADE_MS
        };

        let total_ms = length_ms + fade_ms;
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
        })
    }

    fn decode_next_chunk(&mut self) -> bool {
        if self.frame_no >= self.frame_total {
            self.finished = true;
            return false;
        }

        let remaining = self.frame_total - self.frame_no;
        let frames_to_render = CHUNK_FRAMES.min(remaining as usize);

        let mut i16_buf = vec![0i16; frames_to_render * 2];
        if let Err(e) = self.decoder.render(&mut i16_buf, frames_to_render) {
            log::warn!("USF decode error: {}", e);
            self.finished = true;
            return false;
        }

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

impl Iterator for UsfSource {
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

impl Source for UsfSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.buf_pos < self.buffer.len() {
            Some(self.buffer.len() - self.buf_pos)
        } else {
            None
        }
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(2).unwrap()
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let target_frame = (pos.as_millis() as u64 * SAMPLE_RATE as u64) / 1000;

        self.decoder.restart();
        self.frame_no = 0;
        self.buffer.clear();
        self.buf_pos = 0;
        self.finished = false;

        let mut throwaway = vec![0i16; SEEK_CHUNK_FRAMES * 2];
        let mut remaining = target_frame;
        while remaining > 0 {
            let skip = SEEK_CHUNK_FRAMES.min(remaining as usize);
            if let Err(e) = self.decoder.render(&mut throwaway[..skip * 2], skip) {
                return Err(SeekError::Other(Arc::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("USF seek error: {}", e),
                ))));
            }
            remaining -= skip as u64;
        }

        self.frame_no = target_frame;
        Ok(())
    }
}
