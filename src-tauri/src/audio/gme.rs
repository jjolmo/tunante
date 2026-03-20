use game_music_emu::GameMusicEmu;
use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Default play duration for tracks with unknown length (2.5 minutes)
const DEFAULT_DURATION_MS: i32 = 150_000;
/// Fade duration appended after play_length
const FADE_MS: i32 = 10_000;

/// rodio::Source implementation wrapping Game Music Emu.
/// Supports NSF, NSFE, SPC, GBS, VGM, VGZ, HES, KSS, AY, SAP, GYM.
pub struct GmeSource {
    emu: GameMusicEmu,
    buffer: Vec<f32>,
    buf_pos: usize,
    total_duration: Option<Duration>,
    finished: bool,
}

// Safety: GmeSource is moved into Player::append() and used exclusively on the audio thread.
// The underlying C++ emulator is not thread-safe, but it's only accessed from one thread.
unsafe impl Send for GmeSource {}

impl GmeSource {
    /// Create a new GmeSource for a specific track in a GME file.
    ///
    /// - `path`: Path to the GME file (NSF, SPC, etc.)
    /// - `track_index`: Sub-track index (0-based)
    pub fn new(path: &Path, track_index: usize) -> Result<Self, String> {
        let emu = GameMusicEmu::from_file(path, 44100)
            .map_err(|e| format!("GME load error: {}", e))?;

        let info = emu
            .track_info(track_index)
            .map_err(|e| format!("GME track info error: {}", e))?;

        // Calculate play duration:
        // 1. If play_length is known and positive, use it (GME already computes
        //    length or intro+loop*2 or default)
        // 2. Fall back to our default
        let play_duration_ms = if info.play_length > 0 {
            info.play_length
        } else {
            DEFAULT_DURATION_MS
        };

        // Set fade-out point so GME fades at the end
        emu.set_fade(play_duration_ms);

        emu.start_track(track_index)
            .map_err(|e| format!("GME start track error: {}", e))?;

        let total_ms = play_duration_ms + FADE_MS;

        Ok(Self {
            emu,
            buffer: Vec::new(),
            buf_pos: 0,
            total_duration: Some(Duration::from_millis(total_ms as u64)),
            finished: false,
        })
    }

    fn decode_next_chunk(&mut self) -> bool {
        if self.emu.track_ended() {
            self.finished = true;
            return false;
        }

        // Decode 2048 i16 samples (1024 stereo frames)
        let chunk_size = 2048;
        let mut i16_buf = vec![0i16; chunk_size];

        match self.emu.play(chunk_size, &mut i16_buf) {
            Ok(()) => {
                self.buffer.clear();
                self.buffer
                    .extend(i16_buf.iter().map(|&s| s as f32 / 32768.0));
                self.buf_pos = 0;

                if self.emu.track_ended() {
                    self.finished = true;
                }
                true
            }
            Err(e) => {
                log::warn!("GME decode error: {}", e);
                self.finished = true;
                false
            }
        }
    }
}

impl Iterator for GmeSource {
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

impl Source for GmeSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.buf_pos < self.buffer.len() {
            Some(self.buffer.len() - self.buf_pos)
        } else {
            None
        }
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(2).unwrap() // GME always outputs stereo
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(44100).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let ms = pos.as_millis() as i32;
        self.emu
            .seek(ms)
            .map_err(|e| SeekError::Other(Arc::new(e)))?;
        self.buffer.clear();
        self.buf_pos = 0;
        self.finished = false;
        Ok(())
    }
}
