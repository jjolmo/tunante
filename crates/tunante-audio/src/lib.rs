//! The playback engine, shared by every app.
//!
//! Born in `apps/desktop/src-tauri/src/audio/engine.rs` and moved here when
//! the Tauri app was retired, with one structural change on the way: decoding
//! now happens **out of process**. The desktop engine used to
//! call `tunante_codec::open_source` in-process; this one spawns
//! `tunante-decoder` per track and reads PCM through
//! [`tunante_helper::PipeSource`], the model `tunante` proved on the
//! phone. What the desktop engine knew and mini's player did not — output
//! device selection, recovery from a stream that dies under us, the DSP chain
//! installed over every source — survives unchanged.
//!
//! The marriage is one line: `PipeSource` implements `rodio::Source`, and
//! [`DspSource`] wraps any `rodio::Source`, so the chain simply goes over the
//! pipe. Nobody had composed the two before this crate.
//!
//! What out-of-process buys the engine, compared to its previous life:
//!
//! - No 50 ms teardown sleep on track change. The old decoder's C globals die
//!   with the old decoder's process; there is nothing to wait for.
//! - A crash in thirty-year-old emulator C kills the helper, not the app.
//! - The console RAM a core allocates (~43 MB for NDS) comes back to the
//!   kernel the moment the track changes.
//! - `tunante-codec`, and with it every vendored core, is not linked here.

// rodio 0.21 deprecated `DeviceTrait::name` in favour of description()/id().
// Deliberately kept: the `audio_output_device` setting stores this exact
// string, and swapping the source of truth would orphan every saved device
// choice. Migrating to id() is its own change, done on purpose or not at all.
#![allow(deprecated)]

use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use tunante_core::dsp::{DspSettings, DspSource};
use tunante_helper::PipeSource;

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

/// User's chosen audio output: follow the OS default, or a specific device by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputSelection {
    /// Follow whatever the operating system reports as the default output, and
    /// re-follow it automatically when it changes (e.g. Bluetooth headphones).
    System,
    /// A specific device, selected by its name.
    Device(String),
}

impl OutputSelection {
    /// Parse the persisted setting value. Empty / "system" → follow the system default.
    pub fn from_setting(value: &str) -> Self {
        if value.is_empty() || value == "system" {
            OutputSelection::System
        } else {
            OutputSelection::Device(value.to_string())
        }
    }

    /// Serialize for persistence in the settings table.
    pub fn to_setting(&self) -> String {
        match self {
            OutputSelection::System => "system".to_string(),
            OutputSelection::Device(name) => name.clone(),
        }
    }
}

/// List the names of all available output devices.
pub fn list_output_devices() -> Vec<String> {
    let host = rodio::cpal::default_host();
    match host.output_devices() {
        Ok(devices) => {
            let mut names: Vec<String> = devices.filter_map(|d| d.name().ok()).collect();
            names.dedup();
            names
        }
        Err(e) => {
            log::warn!("[audio] could not enumerate output devices: {e}");
            Vec::new()
        }
    }
}

/// Name of the current system default output device, if any.
pub fn default_output_device_name() -> Option<String> {
    rodio::cpal::default_host()
        .default_output_device()
        .and_then(|d| d.name().ok())
}

/// A run of errors broken by this much quiet means the stream recovered: the
/// next error starts a new run rather than continuing the old one.
const QUIET: Duration = Duration::from_secs(2);
/// How often a failure that keeps repeating is allowed to say so again.
const REPEAT_EVERY: Duration = Duration::from_secs(5);
/// How long a stream may fail without a single quiet gap before the engine
/// stops it instead of letting it spin.
const GIVE_UP: Duration = Duration::from_secs(10);

/// How the output stream is behaving, shared with the cpal error callback.
///
/// The callback runs on the audio thread and knows nothing about the engine, so
/// everything it has to say arrives through here: whether the device needs
/// re-opening, whether the same error is simply repeating, and whether it has
/// been failing long enough to be worth giving up on.
///
/// It exists because the obvious version — log the error and carry on — is a
/// way to fill a disk. On a headless machine ALSA opens the default device
/// happily and then returns POLLERR on every poll: one error per buffer period,
/// forever, with nothing ever coming out. Two runs wrote 15.4 GiB each of the
/// same single line and filled a 31 GB tmpfs between them.
struct StreamHealth {
    /// Set when an error means the device itself is gone; polled by
    /// [`AudioEngine::reconcile_output`], which re-opens the stream.
    rebuild: AtomicBool,
    /// Set when the stream has failed without pause for [`GIVE_UP`]: there is
    /// nothing behind it and re-opening only starts the same spin again.
    lost: AtomicBool,
    log: Mutex<ErrorLog>,
}

/// Collapses identical consecutive errors into one line plus a periodic tally.
struct ErrorLog {
    /// The message last written verbatim. Anything equal to it is a repeat.
    last: String,
    /// Repeats swallowed since the last line was written.
    swallowed: u64,
    /// When a line was last written, and when an error last arrived. `None`
    /// until the first of each — a stream that has never failed has no history
    /// to compare against.
    last_written: Option<Instant>,
    last_error: Option<Instant>,
    /// Start of the current unbroken run of errors.
    streak_started: Instant,
}

impl StreamHealth {
    fn new() -> Self {
        Self {
            rebuild: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            log: Mutex::new(ErrorLog {
                last: String::new(),
                swallowed: 0,
                last_written: None,
                last_error: None,
                streak_started: Instant::now(),
            }),
        }
    }

    /// Record one error from the stream.
    ///
    /// `now` is a parameter rather than an `Instant::now()` inside so the whole
    /// policy — collapse, tally, give up — can be driven through a made-up
    /// timeline in a test instead of one that takes ten real seconds.
    fn note(&self, err: &rodio::cpal::StreamError, now: Instant) {
        use rodio::cpal::StreamError as StreamErr;

        // A transient glitch, NOT a device problem, so it must not trigger a
        // rebuild. `rebuild_output` re-opens the file and seeks back, and on
        // emulated formats (2SF, PSF, USF...) that seek re-runs the emulator
        // from the start -- expensive enough to cause the next underrun, which
        // rebuilds again. That feedback loop made NDS tracks restart every few
        // seconds.
        let underrun = matches!(err, StreamErr::BufferUnderrun);
        if !underrun {
            self.rebuild.store(true, Ordering::SeqCst);
        }

        // try_lock, not lock: this runs on the audio thread and all that is
        // behind the mutex is bookkeeping for a log line. Losing one repeat out
        // of thousands under contention costs nothing; blocking the thread that
        // feeds the speaker would not be free.
        let Ok(mut log) = self.log.try_lock() else {
            return;
        };

        // Two quiet seconds mean the stream worked in between, so this is a new
        // problem starting rather than the old one still going.
        let fresh = match log.last_error {
            Some(t) => now.duration_since(t) >= QUIET,
            None => true,
        };
        if fresh {
            log.streak_started = now;
            log.last.clear();
            log.swallowed = 0;
        }
        log.last_error = Some(now);

        let text = err.to_string();
        if text != log.last {
            if underrun {
                log::warn!("[audio] buffer underrun (audio glitch, no rebuild)");
            } else {
                log::warn!("[audio] output stream error ({text}); scheduling rebuild");
            }
            log.last = text;
            log.swallowed = 0;
            log.last_written = Some(now);
        } else {
            log.swallowed += 1;
            let due = match log.last_written {
                Some(t) => now.duration_since(t) >= REPEAT_EVERY,
                None => true,
            };
            if due {
                log::warn!(
                    "[audio] still failing after {:.0}s: {} more of the same ({})",
                    now.duration_since(log.streak_started).as_secs_f32(),
                    log.swallowed,
                    log.last
                );
                log.swallowed = 0;
                log.last_written = Some(now);
            }
        }

        // Underruns are excluded on purpose: they are what a machine too slow to
        // keep up produces, and tearing the output down over them would silence
        // a player that was merely stuttering.
        if !underrun && now.duration_since(log.streak_started) >= GIVE_UP {
            self.lost.store(true, Ordering::SeqCst);
        }
    }

    /// Take the pending rebuild request, if there is one.
    fn take_rebuild(&self) -> bool {
        self.rebuild.swap(false, Ordering::SeqCst)
    }

    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    /// Drop a rebuild request that is about to be honoured.
    ///
    /// Deliberately does not touch the streak: a rebuild is an attempt at a
    /// cure, not evidence of one. Were the streak cleared here, the five-second
    /// rebuild cooldown would keep resetting a ten-second countdown and the
    /// engine would never reach [`GIVE_UP`]. What clears it is [`QUIET`] — a
    /// stream that ran two seconds without complaining is working.
    fn clear_pending(&self) {
        self.rebuild.store(false, Ordering::SeqCst);
    }

    /// Forget everything. For a deliberate fresh start — engine startup, or the
    /// user picking a different output device — where the new stream deserves
    /// the full countdown rather than the tail of the old one's.
    fn reset(&self) {
        self.rebuild.store(false, Ordering::SeqCst);
        self.lost.store(false, Ordering::SeqCst);
        if let Ok(mut log) = self.log.lock() {
            log.last.clear();
            log.swallowed = 0;
            log.last_written = None;
            log.last_error = None;
            log.streak_started = Instant::now();
        }
    }
}

/// The error callback every stream gets, whichever path opened it.
///
/// `Clone` because [`DeviceSinkBuilder::open_sink_or_fallback`] retries the
/// device's other configurations and needs a callback per attempt; the clone is
/// an `Arc` bump, and every copy reports into the same health.
fn error_callback(
    health: Arc<StreamHealth>,
) -> impl Fn(rodio::cpal::StreamError) + Clone + Send + 'static {
    move |err: rodio::cpal::StreamError| health.note(&err, Instant::now())
}

/// Open an OS audio sink for the given selection, attaching the error callback
/// that flags the engine for a rebuild when the underlying stream fails (e.g.
/// the device is unplugged). Returns the opened sink and the actual device name.
fn open_device_sink(
    selection: &OutputSelection,
    health: Arc<StreamHealth>,
) -> Result<(MixerDeviceSink, String), AudioError> {
    let host = rodio::cpal::default_host();

    let device = match selection {
        OutputSelection::System => host.default_output_device(),
        OutputSelection::Device(name) => host
            .output_devices()
            .ok()
            .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(name.as_str())))
            // Selected device is gone → fall back to the system default so the
            // user still hears audio instead of silence.
            .or_else(|| host.default_output_device()),
    }
    .ok_or_else(|| AudioError::OutputError("no output device available".to_string()))?;

    let name = device.name().unwrap_or_else(|_| "unknown".to_string());

    let sink = DeviceSinkBuilder::from_device(device)
        .map_err(|e| AudioError::OutputError(e.to_string()))?
        .with_error_callback(error_callback(health))
        .open_stream()
        .map_err(|e| AudioError::OutputError(e.to_string()))?;

    Ok((sink, name))
}

/// Rodio's resilient chain — the default device, then any other that will open,
/// each retried across its supported configurations — wearing our callback.
///
/// `DeviceSinkBuilder::open_default_sink()` is that chain in one call and is
/// what this used to fall back to. It keeps rodio's *default* callback, though:
/// an unconditional `eprintln!` per error, no rate limit, no way to give up.
/// That was the one path in the engine that logged without a bound, and it is
/// the path that filled the tmpfs. The convenience is not worth it — the
/// fallback has to be exactly as careful as the direct open.
fn open_fallback_sink(
    health: Arc<StreamHealth>,
) -> Result<(MixerDeviceSink, String), AudioError> {
    let host = rodio::cpal::default_host();

    // The default first, then the rest. It usually appears in both lists; a
    // second attempt at a device that already worked never happens, because the
    // first one returned.
    let mut devices: Vec<rodio::cpal::Device> = Vec::new();
    if let Some(d) = host.default_output_device() {
        devices.push(d);
    }
    if let Ok(rest) = host.output_devices() {
        devices.extend(rest);
    }

    let mut last_err: Option<String> = None;
    for device in devices {
        let name = device.name().unwrap_or_else(|_| "unknown".to_string());
        let builder = match DeviceSinkBuilder::from_device(device) {
            Ok(b) => b,
            Err(e) => {
                last_err = Some(e.to_string());
                continue;
            }
        };
        match builder
            .with_error_callback(error_callback(health.clone()))
            .open_sink_or_fallback()
        {
            Ok(sink) => return Ok((sink, name)),
            Err(e) => last_err = Some(e.to_string()),
        }
    }

    Err(AudioError::OutputError(last_err.unwrap_or_else(|| {
        "no output device available".to_string()
    })))
}

pub struct AudioEngine {
    /// `None` once the engine has given up on a stream that would not stop
    /// failing: there is no output, and playing anything has to say so rather
    /// than pretend.
    _device: Option<MixerDeviceSink>,
    player: Player,
    volume: f32,
    timer: PlaybackTimer,
    current_duration_ms: u64,
    was_playing: bool,
    has_source: bool,
    /// Cooldown: ignore track_finished() briefly after play_file() to prevent
    /// rodio's player.empty() returning true before the mixer starts consuming
    /// the new source (race condition that causes rapid track-skipping).
    play_started_at: Instant,
    fade_on_track_change: bool,
    fade_seconds: f32,
    /// How many times a track with no ending of its own replays its tagged
    /// length, and the fade that closes it. Handed to the decoder per track;
    /// the defaults are the decoder's own (two loops, eight-second fade).
    loop_count: u32,
    loop_fade_ms: u64,
    /// How many times a looping vgmstream stream repeats. `None` leaves the
    /// decoder on its default — which is also what the scanner used. Only a
    /// user setting puts a number here, and it must match what the scanner
    /// used, or the progress bar disagrees with what is heard.
    vgm_loop_count: Option<f64>,
    /// When the last stream rebuild happened. Rebuilding restarts and re-seeks
    /// the current track, so a burst of errors must not be able to do it over
    /// and over -- that turns a glitch into a loop of restarts.
    last_rebuild: Instant,
    /// Bumped on each new fade run; in-progress fades check this and abort
    /// when superseded so rapid track changes don't overlap fades.
    fade_generation: u64,
    /// Desired output device (system default vs a specific device).
    desired_output: OutputSelection,
    /// Name of the device the current stream is actually open on.
    active_device_name: Option<String>,
    /// What the cpal error callback reports: a stream that died (device
    /// unplugged) and wants re-opening, or one that has failed so relentlessly
    /// that it is not worth re-opening at all. Polled by the output supervisor.
    health: Arc<StreamHealth>,
    /// Consecutive failed attempts at re-opening the output, and the device
    /// name the last one failed on.
    ///
    /// A device *change* normally skips the rebuild cooldown, because plugging
    /// in headphones is a one-off event that should be honoured at once. But
    /// when the reopen fails, the name it was reaching for stays different from
    /// the one still open, so "changed" never stops being true and the cooldown
    /// never applies: measured, that is one failed attempt and one error line
    /// every five seconds, for as long as the app runs. Remembering the name
    /// that refused turns the event back into what it is — the same failure,
    /// still happening — and the count decides when to stop asking.
    failed_rebuilds: u32,
    last_failed_target: Option<String>,
    /// The path (incl. any vgm subsong suffix) of the current track, so the
    /// output can be rebuilt without losing what's playing.
    current_path: Option<String>,
    current_duration_hint: i64,
    /// DSP parameters, shared with the audio thread through atomics so effects
    /// can be changed mid-track without rebuilding the player.
    dsp: DspSettings,
}

// Safety: AudioEngine is always accessed through a Mutex, ensuring single-threaded access.
unsafe impl Send for AudioEngine {}
unsafe impl Sync for AudioEngine {}

impl AudioEngine {
    pub fn new() -> Result<Self, AudioError> {
        let health = Arc::new(StreamHealth::new());
        let (device, active_name) = open_device_sink(&OutputSelection::System, health.clone())
            // Fall back to rodio's resilient chain if the direct open fails —
            // rebuilt in `open_fallback_sink` so that it carries the same error
            // callback. It used to call rodio's own, which did not.
            .or_else(|_| open_fallback_sink(health.clone()))?;
        let player = Player::connect_new(&device.mixer());
        player.set_volume(0.8);

        Ok(Self {
            _device: Some(device),
            player,
            volume: 0.8,
            timer: PlaybackTimer::new(),
            current_duration_ms: 0,
            was_playing: false,
            has_source: false,
            play_started_at: Instant::now(),
            fade_on_track_change: false,
            fade_seconds: 2.0,
            loop_count: 2,
            loop_fade_ms: 8_000,
            vgm_loop_count: None,
            last_rebuild: Instant::now() - Duration::from_secs(60),
            fade_generation: 0,
            desired_output: OutputSelection::System,
            active_device_name: Some(active_name),
            health,
            failed_rebuilds: 0,
            last_failed_target: None,
            current_path: None,
            current_duration_hint: 0,
            dsp: DspSettings::default(),
        })
    }

    pub fn play_file(&mut self, path: &Path, duration_hint_ms: i64) -> Result<(), AudioError> {
        self.play_file_at_volume(path, duration_hint_ms, self.volume)
    }

    /// The single point where a decoded source enters the player.
    ///
    /// Every format converges here, so the DSP chain is applied once and covers
    /// all of them — there is no per-decoder wiring to keep in sync, and any
    /// future effect only has to be added to [`DspSettings::build_chain`].
    ///
    /// The chain is always installed, even when every effect is off (it is then a
    /// bit-exact passthrough costing one atomic load per processor per frame).
    /// That is what lets the UI toggle effects *while a track plays*: deciding
    /// here would mean rebuilding the player to apply a change, which cuts the
    /// sound.
    fn append_source<S>(&mut self, source: S)
    where
        S: Source + Send + 'static,
    {
        let duration = source.total_duration();
        self.player
            .append(DspSource::new(source, self.dsp.build_chain()));
        self.player.play();
        self.current_duration_ms = duration.map(|d| d.as_millis() as u64).unwrap_or(0);
    }

    /// Shared handle to the DSP parameters, for the UI layer's commands.
    pub fn dsp(&self) -> &DspSettings {
        &self.dsp
    }

    pub fn play_file_at_volume(
        &mut self,
        path: &Path,
        duration_hint_ms: i64,
        initial_volume: f32,
    ) -> Result<(), AudioError> {
        // Nothing to play into: the engine gave up on a stream that would not
        // stop failing. Say so once, per attempt, instead of opening a decoder
        // to feed a device that is not there.
        if self._device.is_none() {
            return Err(AudioError::OutputError("no audio output".to_string()));
        }

        // Remember what's playing so the output device can be rebuilt (on a
        // device switch/unplug) by reopening this same source at its position.
        self.current_path = Some(path.to_string_lossy().to_string());
        self.current_duration_hint = duration_hint_ms;

        // Recreate the Player to fully reset rodio's internal resampler state.
        // Without this, switching between tracks with different sample rates
        // (e.g. 48kHz PSF2/Opus → 44.1kHz GSF) can corrupt the resampler,
        // causing audio to play at the wrong speed until app restart.
        //
        // The in-process engine also slept 50 ms here so the old decoder's C
        // globals were torn down before the next one came up. Gone: the old
        // decoder's globals die with the old decoder's process.
        self.player.stop();
        self.player = Player::connect_new(&self.mixer_device().mixer());
        self.player.set_volume(initial_volume.clamp(0.0, 1.0));

        log::info!("[play_file] path={}", path.display());

        // Format dispatch lives in tunante-codec — on the other side of the
        // pipe, inside tunante-decoder, so a format is only ever wired up once
        // and none of the vendored cores link into this process.
        let source = PipeSource::open_with(
            path,
            duration_hint_ms,
            self.loop_count,
            self.loop_fade_ms,
            self.vgm_loop_count,
        )
        .map_err(AudioError::DecoderError)?;
        self.append_source(source);

        self.timer.start();
        self.was_playing = true;
        self.has_source = true;
        self.play_started_at = Instant::now();

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
        self.current_path = None;
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

    /// Set the rodio player's playback volume without changing the user-visible
    /// volume (`self.volume`). Used by the fade orchestrator so the UI slider
    /// stays at the user's setting while the actual output is ramped.
    pub fn set_player_volume_raw(&mut self, volume: f32) {
        self.player.set_volume(volume.clamp(0.0, 1.0));
    }

    pub fn set_vgm_loop_count(&mut self, count: f64) {
        self.vgm_loop_count = Some(count.clamp(0.0, 20.0));
    }

    /// How many times a track with no ending replays its tagged length, and the
    /// closing fade. Applies from the next track on: changing it mid-track
    /// would mean restarting the decoder, and losing your place to change a
    /// setting is a worse trade than waiting for the next song.
    pub fn set_loop_settings(&mut self, loops: u32, fade_ms: u64) {
        self.loop_count = loops.max(1);
        self.loop_fade_ms = fade_ms;
    }

    pub fn fade_on_track_change(&self) -> bool {
        self.fade_on_track_change
    }

    pub fn fade_seconds(&self) -> f32 {
        self.fade_seconds
    }

    pub fn set_fade_on_track_change(&mut self, enabled: bool) {
        self.fade_on_track_change = enabled;
    }

    pub fn set_fade_seconds(&mut self, seconds: f32) {
        self.fade_seconds = seconds.clamp(0.0, 10.0);
    }

    pub fn has_source(&self) -> bool {
        self.has_source
    }

    /// Bump the fade generation counter and return the new value. Any in-progress
    /// fade comparing against an older value should bail out.
    pub fn bump_fade_generation(&mut self) -> u64 {
        self.fade_generation = self.fade_generation.wrapping_add(1);
        self.fade_generation
    }

    pub fn fade_generation(&self) -> u64 {
        self.fade_generation
    }

    pub fn is_playing(&self) -> bool {
        self.has_source && !self.player.is_paused() && !self.player.empty()
    }

    pub fn track_finished(&self) -> bool {
        // Ignore for the first second after play_file() — rodio's mixer may
        // not have started consuming the new source yet, so player.empty()
        // can briefly return true and trigger an immediate (false) auto-advance.
        if self.play_started_at.elapsed() < Duration::from_secs(1) {
            return false;
        }
        self.was_playing && self.has_source && self.player.empty()
    }

    pub fn position_ms(&self) -> u64 {
        self.timer.position_ms()
    }

    pub fn duration_ms(&self) -> u64 {
        self.current_duration_ms
    }

    // ---- Output device management ----

    /// The currently desired output (system default vs a specific device).
    pub fn output_selection(&self) -> OutputSelection {
        self.desired_output.clone()
    }

    /// Name of the device the stream is actually open on right now.
    pub fn active_device_name(&self) -> Option<String> {
        self.active_device_name.clone()
    }

    /// Change the desired output and rebuild the stream immediately, preserving
    /// the current track and playback position.
    pub fn set_output_selection(&mut self, selection: OutputSelection) -> Result<(), AudioError> {
        self.desired_output = selection;
        // A device the user picked deserves the full countdown, not what is
        // left of the previous one's — and this is the way back from a stream
        // the engine had given up on.
        self.health.reset();
        self.failed_rebuilds = 0;
        self.last_failed_target = None;
        self.rebuild_output()
    }

    /// True once the engine has stopped an output that would not stop failing.
    /// The app shows its "no audio output" banner on this.
    pub fn output_lost(&self) -> bool {
        self.health.is_lost() && self._device.is_none()
    }

    /// The open sink. Only called where one has just been checked for or
    /// installed; `_device` is `None` exactly while the output is given up on,
    /// and every path into playback returns early in that case.
    fn mixer_device(&self) -> &MixerDeviceSink {
        self._device
            .as_ref()
            .expect("output sink checked before use")
    }

    /// Stop an output that only produces errors.
    ///
    /// Dropping the sink is the point: it is what ends the stream, and with it
    /// the flood of callbacks. Everything else here is the app being told the
    /// truth — nothing is playing, because nothing can.
    fn shutdown_output(&mut self) {
        self.player.stop();
        self.timer.pause();
        self.was_playing = false;
        self.has_source = false;
        self._device = None;
    }

    /// Re-open the OS audio sink for the currently desired output and resume the
    /// current track at its previous position. A rodio source cannot be moved
    /// between mixers, so we re-open the current file and seek back.
    pub fn rebuild_output(&mut self) -> Result<(), AudioError> {
        let pos = self.timer.position_ms();
        let was_playing = self.was_playing;
        let had_source = self.has_source;
        let path = self.current_path.clone();
        let hint = self.current_duration_hint;

        self.health.clear_pending();
        let (device, name) = open_device_sink(&self.desired_output, self.health.clone())?;

        // Drop the old stream and connect a fresh player to the new device.
        self.player.stop();
        self._device = Some(device);
        self.active_device_name = Some(name);
        self.player = Player::connect_new(&self.mixer_device().mixer());
        self.player.set_volume(self.volume);

        // Restore the current track at its previous position and play state.
        if had_source {
            if let Some(p) = path {
                self.play_file_at_volume(Path::new(&p), hint, self.volume)?;
                let _ = self.seek(pos);
                if !was_playing {
                    self.pause();
                }
            }
        }
        Ok(())
    }

    /// Called periodically by the output supervisor. Rebuilds the stream when it
    /// reported an error (device unplugged) or when the effective target device
    /// changed (system default switched to freshly-connected headphones). Returns
    /// the new active device name when a rebuild happened, so the UI can be told.
    pub fn reconcile_output(&mut self) -> Option<String> {
        let flagged = self.health.take_rebuild();
        let target = self.resolve_target_name();
        let changed = match (&target, &self.active_device_name) {
            (Some(t), Some(a)) => t != a,
            (Some(_), None) => true,
            _ => false,
        };

        // A target we have not already failed on. Only this counts as the
        // one-off event that jumps the rebuild cooldown; the name that just
        // refused to open is not news however many ticks it is repeated over.
        let new_target = changed && self.last_failed_target.as_deref() != target.as_deref();

        // The stream has been failing without a single quiet gap: every poll
        // an error, nothing ever coming out. Re-opening it would open the same
        // dead device and start the same spin, so stop instead and let the app
        // say there is no audio output. Hardware that genuinely arrived is the
        // exception and still gets an attempt — it is exactly the thing that
        // could fix this.
        if self.health.is_lost() && !new_target {
            if self._device.is_some() {
                log::error!(
                    "[audio] output failing continuously with nothing coming out; \
                     stopping the stream. Pick an output device in Ajustes to retry."
                );
                self.shutdown_output();
            }
            return None;
        }

        // Enough refusals from the same target: stop asking. Note what this
        // does *not* do — it does not touch the open stream. A reopen that
        // fails says nothing about the sink already playing, and tearing that
        // down over it would silence a player that was working.
        const GIVE_UP_AFTER: u32 = 5;
        if self.failed_rebuilds >= GIVE_UP_AFTER && !new_target {
            return None;
        }

        if flagged || changed {
            // A device change is a deliberate, one-off event and always wins.
            // An error flag is rate-limited: rebuilding costs a restart+seek,
            // so repeating it on every tick would be worse than the glitch.
            const MIN_GAP: Duration = Duration::from_secs(5);
            if !new_target && self.last_rebuild.elapsed() < MIN_GAP {
                log::debug!("[audio] rebuild requested again too soon; ignoring");
                return None;
            }
            self.last_rebuild = Instant::now();
            match self.rebuild_output() {
                Ok(()) => {
                    self.failed_rebuilds = 0;
                    self.last_failed_target = None;
                    return self.active_device_name.clone();
                }
                Err(e) => {
                    self.failed_rebuilds = self.failed_rebuilds.saturating_add(1);
                    self.last_failed_target = target;
                    // Said once. After that the only thing left to report is
                    // that it is over.
                    if self.failed_rebuilds == 1 {
                        log::error!("[audio] output rebuild failed: {e}");
                    } else if self.failed_rebuilds == GIVE_UP_AFTER {
                        log::error!(
                            "[audio] the output has refused to open {GIVE_UP_AFTER} times \
                             ({e}); no longer retrying. Pick an output device in Ajustes."
                        );
                    }
                }
            }
        }
        None
    }

    /// The device name we *should* currently be playing on. For a specific device
    /// that has gone away, this falls back to the system default so we don't try
    /// to reopen a missing device on every supervisor tick.
    fn resolve_target_name(&self) -> Option<String> {
        match &self.desired_output {
            OutputSelection::System => default_output_device_name(),
            OutputSelection::Device(name) => {
                if list_output_devices().iter().any(|n| n == name) {
                    Some(name.clone())
                } else {
                    default_output_device_name()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::cpal::{BackendSpecificError, StreamError};
    use std::sync::atomic::AtomicUsize;

    /// Every line the code under test writes, counted. `StreamHealth` reports
    /// through the `log` facade, so counting there is the only way to assert
    /// what it actually says out loud.
    static LINES: AtomicUsize = AtomicUsize::new(0);

    struct Counter;
    impl log::Log for Counter {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, _: &log::Record) {
            LINES.fetch_add(1, Ordering::SeqCst);
        }
        fn flush(&self) {}
    }

    fn written() -> usize {
        LINES.swap(0, Ordering::SeqCst)
    }

    fn pollerr() -> StreamError {
        StreamError::BackendSpecific {
            err: BackendSpecificError {
                description: "`alsa::poll()` returned POLLERR".to_string(),
            },
        }
    }

    /// The whole policy in one test on purpose: the line counter is global, so
    /// two tests running side by side would count each other's output.
    #[test]
    fn a_stream_that_only_fails_is_logged_once_and_then_dropped() {
        let _ = log::set_logger(&Counter);
        log::set_max_level(log::LevelFilter::Trace);

        let health = StreamHealth::new();
        let err = pollerr();
        let t0 = Instant::now();
        written();

        // Twelve seconds of the same error every 10 ms — the shape ALSA
        // produces on a machine with no sound server, once per buffer period.
        let mut lost_at = None;
        for i in 0..1200u64 {
            health.note(&err, t0 + Duration::from_millis(i * 10));
            if lost_at.is_none() && health.is_lost() {
                lost_at = Some(i * 10);
            }
        }

        // 1200 errors, three lines: the first, and a tally at five and ten
        // seconds. This is the whole point — the version this replaces wrote
        // 181 million of them.
        assert_eq!(written(), 3);
        assert_eq!(lost_at, Some(GIVE_UP.as_millis() as u64));

        // A quiet gap means the stream worked in between, so the next error is
        // a new problem: it is logged in full and the countdown starts over.
        health.reset();
        health.note(&err, t0);
        health.note(&err, t0 + QUIET);
        assert_eq!(written(), 2);
        assert!(!health.is_lost());

        // The same twelve seconds of underruns instead: collapsed just as
        // hard, but never rebuilt and never given up on. A machine too slow to
        // keep up stutters; it does not deserve to have its output taken away.
        health.reset();
        for i in 0..1200u64 {
            health.note(
                &StreamError::BufferUnderrun,
                t0 + Duration::from_millis(i * 10),
            );
        }
        assert_eq!(written(), 3);
        assert!(!health.is_lost());
        assert!(!health.take_rebuild());
    }
}
