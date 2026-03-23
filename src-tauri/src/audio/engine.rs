use super::gme::GmeSource;
use super::gsf::GsfSource;
use super::opus::OggOpusSource;
use super::psf::PsfSource;
use super::psf2::Psf2Source;
use super::twosf::TwoSfSource;
use super::vgm_path::{is_gme_format, is_gsf_format, is_psf_format, is_psf2_format, is_twosf_format, parse_vgm_path};
use super::vgmstream::VgmstreamSource;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::fs::File;
use std::io::BufReader;
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

/// Standard audio formats that symphonia handles well — route these directly,
/// never through vgmstream (which may mishandle them).
fn is_standard_format(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "flac" | "ogg" | "wav" | "aac" | "aiff" | "wma" | "m4a" | "ape" | "wv"
    )
}

pub struct AudioEngine {
    _device: MixerDeviceSink,
    player: Player,
    volume: f32,
    timer: PlaybackTimer,
    current_duration_ms: u64,
    was_playing: bool,
    has_source: bool,
}

// Safety: AudioEngine is always accessed through a Mutex, ensuring single-threaded access.
unsafe impl Send for AudioEngine {}
unsafe impl Sync for AudioEngine {}

impl AudioEngine {
    pub fn new() -> Result<Self, AudioError> {
        let device = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| AudioError::OutputError(e.to_string()))?;
        let player = Player::connect_new(&device.mixer());
        player.set_volume(0.8);

        Ok(Self {
            _device: device,
            player,
            volume: 0.8,
            timer: PlaybackTimer::new(),
            current_duration_ms: 0,
            was_playing: false,
            has_source: false,
        })
    }

    pub fn play_file(&mut self, path: &Path) -> Result<(), AudioError> {
        // Recreate the Player to fully reset rodio's internal resampler state.
        // Without this, switching between tracks with different sample rates
        // (e.g. 48kHz PSF2/Opus → 44.1kHz GSF) can corrupt the resampler,
        // causing audio to play at the wrong speed until app restart.
        self.player.stop();
        self.player = Player::connect_new(&self._device.mixer());
        self.player.set_volume(self.volume);

        let path_str = path.to_string_lossy();
        let (actual_path_str, sub_track) = parse_vgm_path(&path_str);
        let actual_path = Path::new(actual_path_str);

        let ext = actual_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        log::info!("[play_file] path={}, ext='{}', is_gsf={}", actual_path.display(), ext, is_gsf_format(ext));

        if is_gme_format(ext) {
            // GME chiptune format (NSF, SPC, GBS, VGM, etc.)
            let track_index = sub_track.unwrap_or(0);
            let source = GmeSource::new(actual_path, track_index)
                .map_err(|e| AudioError::DecoderError(e))?;
            let duration = source.total_duration();
            self.player.append(source);
            self.player.play();
            self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        } else if is_gsf_format(ext) {
            // GSF/minigsf format (GBA Sound Format via mGBA)
            let source = GsfSource::new(actual_path)
                .map_err(|e| AudioError::DecoderError(e))?;
            let duration = source.total_duration();
            self.player.append(source);
            self.player.play();
            self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        } else if is_twosf_format(ext) {
            // 2SF/mini2sf format (NDS Sound Format via DeSmuME)
            let source = TwoSfSource::new(actual_path)
                .map_err(|e| AudioError::DecoderError(e))?;
            let duration = source.total_duration();
            self.player.append(source);
            self.player.play();
            self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        } else if is_psf2_format(ext) {
            // PSF2/minipsf2 format (PlayStation 2 Sound Format via Highly Experimental)
            let source = Psf2Source::new(actual_path)
                .map_err(|e| AudioError::DecoderError(e))?;
            let duration = source.total_duration();
            self.player.append(source);
            self.player.play();
            self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        } else if is_psf_format(ext) {
            // PSF/minipsf format (PlayStation 1 Sound Format via sexypsf)
            let source = PsfSource::new(actual_path)
                .map_err(|e| AudioError::DecoderError(e))?;
            let duration = source.total_duration();
            self.player.append(source);
            self.player.play();
            self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        } else if ext.eq_ignore_ascii_case("opus") {
            // Use our custom Opus decoder (symphonia doesn't support Opus)
            let file = BufReader::new(File::open(actual_path)?);
            let source = OggOpusSource::new(file)
                .map_err(|e| AudioError::DecoderError(e))?;
            let duration = source.total_duration();
            self.player.append(source);
            self.player.play();
            self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        } else if is_standard_format(ext) {
            // Standard symphonia decoder (MP3, FLAC, AAC, WAV, OGG, etc.)
            let file = File::open(actual_path)?;
            let source = Decoder::try_from(file)
                .map_err(|e| AudioError::DecoderError(e.to_string()))?;
            let duration = source.total_duration();
            self.player.append(source);
            self.player.play();
            self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
        } else {
            // Try vgmstream for game audio formats (BCSTM, ADX, HCA, etc.)
            let subsong = sub_track.map(|s| s as i32).unwrap_or(0);
            match VgmstreamSource::new(actual_path, subsong) {
                Ok(source) => {
                    let duration = source.total_duration();
                    self.player.append(source);
                    self.player.play();
                    self.current_duration_ms =
                        duration.map(|d| d.as_millis() as u64).unwrap_or(0);
                }
                Err(_) => {
                    // Last resort: try symphonia decoder for unknown formats
                    let file = File::open(actual_path)?;
                    let source = Decoder::try_from(file)
                        .map_err(|e| AudioError::DecoderError(e.to_string()))?;
                    let duration = source.total_duration();
                    self.player.append(source);
                    self.player.play();
                    self.current_duration_ms =
                        duration.map(|d| d.as_millis() as u64).unwrap_or(0);
                }
            }
        }

        self.timer.start();
        self.was_playing = true;
        self.has_source = true;

        Ok(())
    }

    pub fn pause(&mut self) {
        self.player.pause();
        self.timer.pause();
        self.was_playing = false;
    }

    pub fn resume(&mut self) {
        self.player.play();
        self.timer.resume();
        self.was_playing = true;
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.timer.stop();
        self.was_playing = false;
        self.has_source = false;
        self.current_duration_ms = 0;
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        let position = Duration::from_millis(position_ms);
        self.player
            .try_seek(position)
            .map_err(|e| format!("Seek failed: {}", e))?;
        self.timer.seek(position);
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.player.set_volume(self.volume);
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn is_playing(&self) -> bool {
        self.has_source && !self.player.is_paused() && !self.player.empty()
    }

    pub fn track_finished(&self) -> bool {
        self.was_playing && self.has_source && self.player.empty()
    }

    pub fn position_ms(&self) -> u64 {
        self.timer.position_ms()
    }

    pub fn duration_ms(&self) -> u64 {
        self.current_duration_ms
    }
}
