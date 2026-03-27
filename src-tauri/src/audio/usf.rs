use lazyusf2_rs::UsfDecoder;
use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_DURATION_MS: u64 = 150_000;
const DEFAULT_FADE_MS: u64 = 10_000;
const SAMPLE_RATE: u32 = 44100;
/// Frames per decode chunk in the background thread
const CHUNK_FRAMES: usize = 2048;

/// Messages from UsfSource to the decode thread
enum DecodeCmd {
    /// Request more audio
    Continue,
    /// Seek to a position (frame number)
    Seek(u64),
    /// Stop the decode thread
    Stop,
}

/// Messages from the decode thread back to UsfSource
enum DecodeResult {
    /// A chunk of f32 stereo samples (already with fade applied)
    Samples(Vec<f32>),
    /// Decoding finished (end of track)
    Finished,
    /// Seek completed
    SeekDone,
}

/// rodio::Source that decodes USF in a background thread.
/// If the N64 emulator crashes or blocks, rodio's audio thread
/// continues running (gets silence when buffer is empty).
pub struct UsfSource {
    rx: mpsc::Receiver<DecodeResult>,
    tx: mpsc::Sender<DecodeCmd>,
    buffer: Vec<f32>,
    buf_pos: usize,
    total_duration: Option<Duration>,
    finished: bool,
}

unsafe impl Send for UsfSource {}

impl UsfSource {
    pub fn new(path: &Path) -> Result<Self, String> {
        let (decoder, tags) =
            UsfDecoder::new(path, SAMPLE_RATE).map_err(|e| format!("USF load error: {}", e))?;

        let length_ms = if tags.length_ms > 0 { tags.length_ms } else { DEFAULT_DURATION_MS };
        let fade_ms = if tags.fade_ms > 0 {
            tags.fade_ms
        } else {
            DEFAULT_FADE_MS
        };
        let total_ms = length_ms + fade_ms;
        let frame_fade = length_ms * SAMPLE_RATE as u64 / 1000;
        let frame_total = total_ms * SAMPLE_RATE as u64 / 1000;

        let (cmd_tx, cmd_rx) = mpsc::channel::<DecodeCmd>();
        let (result_tx, result_rx) = mpsc::channel::<DecodeResult>();

        // Spawn decode thread
        std::thread::Builder::new()
            .name("usf-decode".into())
            .spawn(move || {
                decode_thread(decoder, cmd_rx, result_tx, frame_total, frame_fade);
            })
            .map_err(|e| format!("Failed to spawn USF decode thread: {}", e))?;

        // Request first chunk immediately
        let _ = cmd_tx.send(DecodeCmd::Continue);

        Ok(Self {
            rx: result_rx,
            tx: cmd_tx,
            buffer: Vec::new(),
            buf_pos: 0,
            total_duration: Some(Duration::from_millis(total_ms)),
            finished: false,
        })
    }

    fn try_fill_buffer(&mut self) -> bool {
        // Try to receive a decoded chunk (non-blocking first, then short block)
        match self.rx.try_recv() {
            Ok(DecodeResult::Samples(samples)) => {
                self.buffer = samples;
                self.buf_pos = 0;
                // Request next chunk
                let _ = self.tx.send(DecodeCmd::Continue);
                true
            }
            Ok(DecodeResult::Finished) => {
                self.finished = true;
                false
            }
            Ok(DecodeResult::SeekDone) => {
                // After seek, request next chunk
                let _ = self.tx.send(DecodeCmd::Continue);
                // Block briefly waiting for first post-seek chunk
                match self.rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(DecodeResult::Samples(samples)) => {
                        self.buffer = samples;
                        self.buf_pos = 0;
                        let _ = self.tx.send(DecodeCmd::Continue);
                        true
                    }
                    Ok(DecodeResult::Finished) => {
                        self.finished = true;
                        false
                    }
                    _ => false,
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Buffer empty, block briefly for data
                match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(DecodeResult::Samples(samples)) => {
                        self.buffer = samples;
                        self.buf_pos = 0;
                        let _ = self.tx.send(DecodeCmd::Continue);
                        true
                    }
                    Ok(DecodeResult::Finished) => {
                        self.finished = true;
                        false
                    }
                    _ => false,
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                // Decode thread crashed or exited
                self.finished = true;
                false
            }
        }
    }
}

impl Drop for UsfSource {
    fn drop(&mut self) {
        let _ = self.tx.send(DecodeCmd::Stop);
    }
}

impl Iterator for UsfSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.finished && self.buf_pos >= self.buffer.len() {
            return None;
        }
        if self.buf_pos >= self.buffer.len() {
            if !self.try_fill_buffer() {
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
        self.buffer.clear();
        self.buf_pos = 0;
        self.finished = false;
        let _ = self.tx.send(DecodeCmd::Seek(target_frame));
        Ok(())
    }
}

// ============================================================================
// Background decode thread
// ============================================================================

fn decode_thread(
    mut decoder: UsfDecoder,
    cmd_rx: mpsc::Receiver<DecodeCmd>,
    result_tx: mpsc::Sender<DecodeResult>,
    frame_total: u64,
    frame_fade: u64,
) {
    let mut frame_no: u64 = 0;

    loop {
        match cmd_rx.recv() {
            Ok(DecodeCmd::Continue) => {
                if frame_no >= frame_total {
                    let _ = result_tx.send(DecodeResult::Finished);
                    continue;
                }

                let remaining = (frame_total - frame_no) as usize;
                let frames = CHUNK_FRAMES.min(remaining);

                let mut i16_buf = vec![0i16; frames * 2];
                if decoder.render(&mut i16_buf, frames).is_err() {
                    let _ = result_tx.send(DecodeResult::Finished);
                    return; // Fatal error — exit thread
                }

                // Convert to f32 with fade
                let mut samples = Vec::with_capacity(frames * 2);
                for i in 0..frames {
                    let gf = frame_no + i as u64;
                    let mut l = i16_buf[i * 2] as f32 / 32768.0;
                    let mut r = i16_buf[i * 2 + 1] as f32 / 32768.0;

                    if gf >= frame_total {
                        l = 0.0;
                        r = 0.0;
                    } else if gf >= frame_fade && frame_total > frame_fade {
                        let progress = (frame_total - gf) as f32 / (frame_total - frame_fade) as f32;
                        let fade = progress * progress;
                        l *= fade;
                        r *= fade;
                    }
                    samples.push(l);
                    samples.push(r);
                }

                frame_no += frames as u64;
                let _ = result_tx.send(DecodeResult::Samples(samples));
            }
            Ok(DecodeCmd::Seek(target_frame)) => {
                decoder.restart();
                frame_no = 0;

                // Fast-forward in large chunks
                let mut throwaway = vec![0i16; 8192 * 2];
                let mut rem = target_frame;
                while rem > 0 {
                    let skip = 8192usize.min(rem as usize);
                    if decoder.render(&mut throwaway[..skip * 2], skip).is_err() {
                        let _ = result_tx.send(DecodeResult::Finished);
                        return;
                    }
                    rem -= skip as u64;
                }
                frame_no = target_frame;
                let _ = result_tx.send(DecodeResult::SeekDone);
            }
            Ok(DecodeCmd::Stop) | Err(_) => {
                return; // Clean exit
            }
        }
    }
}
