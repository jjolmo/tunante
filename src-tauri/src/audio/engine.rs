use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AudioError {
    #[error("Audio output error: {0}")]
    OutputError(String),
    #[error("Decoder error: {0}")]
    DecoderError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

struct PlaybackTimer {
    started_at: Option<Instant>,
    accumulated: Duration,
}

impl PlaybackTimer {
    fn new() -> Self {
        Self {
            started_at: None,
            accumulated: Duration::ZERO,
        }
    }

    fn start(&mut self) {
        self.started_at = Some(Instant::now());
        self.accumulated = Duration::ZERO;
    }

    fn pause(&mut self) {
        if let Some(started) = self.started_at.take() {
            self.accumulated += started.elapsed();
        }
    }

    fn resume(&mut self) {
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }
    }

    fn stop(&mut self) {
        self.started_at = None;
        self.accumulated = Duration::ZERO;
    }

    fn seek(&mut self, position: Duration) {
        self.accumulated = position;
        if self.started_at.is_some() {
            self.started_at = Some(Instant::now());
        }
    }

    fn position(&self) -> Duration {
        let running = self
            .started_at
            .map(|s| s.elapsed())
            .unwrap_or(Duration::ZERO);
        self.accumulated + running
    }

    fn position_ms(&self) -> u64 {
        self.position().as_millis() as u64
    }
}

pub struct AudioEngine {
    _stream: Box<OutputStream>,
    stream_handle: OutputStreamHandle,
    sink: Option<Sink>,
    volume: f32,
    timer: PlaybackTimer,
    current_duration_ms: u64,
    was_playing: bool,
}

// Safety: OutputStream is stored in a Box and never moved between threads.
// The AudioEngine itself is always accessed through a Mutex, ensuring single-threaded access.
// OutputStreamHandle and Sink are already Send+Sync.
unsafe impl Send for AudioEngine {}
unsafe impl Sync for AudioEngine {}

impl AudioEngine {
    pub fn new() -> Result<Self, AudioError> {
        let (stream, stream_handle) =
            OutputStream::try_default().map_err(|e| AudioError::OutputError(e.to_string()))?;

        Ok(Self {
            _stream: Box::new(stream),
            stream_handle,
            sink: None,
            volume: 0.8,
            timer: PlaybackTimer::new(),
            current_duration_ms: 0,
            was_playing: false,
        })
    }

    pub fn play_file(&mut self, path: &Path) -> Result<(), AudioError> {
        // Stop current playback
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }

        let file = BufReader::new(File::open(path)?);

        // Wrap decoder creation in catch_unwind because rodio/symphonia can panic
        // on certain malformed or unsupported files instead of returning an error
        let source = catch_unwind(AssertUnwindSafe(|| Decoder::new(file)))
            .map_err(|panic| {
                let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown decoder panic".to_string()
                };
                AudioError::DecoderError(format!("Decoder crashed: {}", msg))
            })?
            .map_err(|e| AudioError::DecoderError(e.to_string()))?;

        // Get duration from source if available
        let duration = source.total_duration();

        let sink = Sink::try_new(&self.stream_handle)
            .map_err(|e| AudioError::OutputError(e.to_string()))?;
        sink.set_volume(self.volume);
        sink.append(source);

        self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        self.timer.start();
        self.was_playing = true;
        self.sink = Some(sink);

        Ok(())
    }

    pub fn pause(&mut self) {
        if let Some(ref sink) = self.sink {
            sink.pause();
            self.timer.pause();
            self.was_playing = false;
        }
    }

    pub fn resume(&mut self) {
        if let Some(ref sink) = self.sink {
            sink.play();
            self.timer.resume();
            self.was_playing = true;
        }
    }

    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.timer.stop();
        self.was_playing = false;
        self.current_duration_ms = 0;
    }

    pub fn seek(&mut self, position_ms: u64) {
        if let Some(ref sink) = self.sink {
            let position = Duration::from_millis(position_ms);
            let _ = sink.try_seek(position);
            self.timer.seek(position);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(ref sink) = self.sink {
            sink.set_volume(self.volume);
        }
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn is_playing(&self) -> bool {
        self.sink
            .as_ref()
            .map(|s| !s.is_paused() && !s.empty())
            .unwrap_or(false)
    }

    pub fn track_finished(&self) -> bool {
        self.was_playing
            && self
                .sink
                .as_ref()
                .map(|s| s.empty())
                .unwrap_or(false)
    }

    pub fn position_ms(&self) -> u64 {
        self.timer.position_ms()
    }

    pub fn duration_ms(&self) -> u64 {
        self.current_duration_ms
    }
}
