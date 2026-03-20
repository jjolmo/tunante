use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::time::Duration;
use vgmstream_rs::Vgmstream;

/// rodio::Source implementation wrapping vgmstream.
/// Supports 700+ game audio formats including BCSTM, BFSTM, ADX, HCA, and many more.
pub struct VgmstreamSource {
    vgm: Vgmstream,
    channels: u16,
    sample_rate: u32,
    buffer: Vec<f32>,
    buf_pos: usize,
    total_duration: Option<Duration>,
    finished: bool,
}

unsafe impl Send for VgmstreamSource {}

impl VgmstreamSource {
    /// Create a new VgmstreamSource for a file.
    ///
    /// - `path`: Path to the audio file
    /// - `subsong`: Subsong index (0 = default/first, 1..N for specific subsong)
    pub fn new(path: &Path, subsong: i32) -> Result<Self, String> {
        let vgm = Vgmstream::open(path, subsong)?;
        let info = vgm.info();

        let channels = info.channels.max(1) as u16;
        let sample_rate = info.sample_rate.max(1) as u32;

        // Calculate duration from play_samples (includes loop/fade config)
        let total_duration = if info.play_samples > 0 {
            let secs = info.play_samples as f64 / info.sample_rate as f64;
            Some(Duration::from_secs_f64(secs))
        } else if info.stream_samples > 0 {
            let secs = info.stream_samples as f64 / info.sample_rate as f64;
            Some(Duration::from_secs_f64(secs))
        } else {
            None
        };

        Ok(Self {
            vgm,
            channels,
            sample_rate,
            buffer: Vec::new(),
            buf_pos: 0,
            total_duration,
            finished: false,
        })
    }

    fn decode_next_chunk(&mut self) -> bool {
        match self.vgm.render() {
            Ok((samples, done)) => {
                // Convert i16 interleaved PCM to f32
                self.buffer.clear();
                self.buffer.extend(samples.iter().map(|&s| s as f32 / 32768.0));
                self.buf_pos = 0;
                if done {
                    self.finished = true;
                }
                !samples.is_empty()
            }
            Err(e) => {
                log::warn!("vgmstream decode error: {}", e);
                self.finished = true;
                false
            }
        }
    }
}

impl Iterator for VgmstreamSource {
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

impl Source for VgmstreamSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.buf_pos < self.buffer.len() {
            Some(self.buffer.len() - self.buf_pos)
        } else {
            None
        }
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(self.channels).unwrap_or(NonZeroU16::new(2).unwrap())
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(self.sample_rate).unwrap_or(NonZeroU32::new(44100).unwrap())
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let target_sample = (pos.as_secs_f64() * self.sample_rate as f64) as i64;
        self.vgm.seek(target_sample);
        self.buffer.clear();
        self.buf_pos = 0;
        self.finished = false;
        Ok(())
    }
}
