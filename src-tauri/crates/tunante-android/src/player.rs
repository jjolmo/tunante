//! The player: one audio device, one queue, one decoder process at a time.
//!
//! Modelled on `tunante-mini/src/player.rs`, minus everything that was really
//! about Slint. The important difference is who drives it: in mini a 500 ms
//! `slint::Timer` on the UI thread ticks the queue, saves the session and polls
//! for the end of a track, so all of it stops when the window does. Here
//! [`Player::tick`] is called by the foreground service instead, which is the
//! only thing on Android that is allowed to keep running while the screen is
//! off — and is required to, since Android 17 refuses background audio to an app
//! without one.

use std::time::{Duration, Instant};

/// How often the foreground service calls [`Player::tick`].
///
/// The sleep timer counts in these, so it has to agree with the service. Named
/// here rather than passed in so the two cannot drift apart silently.
pub const TICK_MS: u64 = 500;

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player as RodioPlayer, Source};
use tunante_core::db::models::Track;
use tunante_core::{PlayClock, PlayQueue, RepeatMode, SleepTimer};
use tunante_helper::PipeSource;

pub struct Player {
    _device: MixerDeviceSink,
    player: RodioPlayer,
    queue: PlayQueue,
    volume: f32,
    /// Where the track is. Wall-clock, because the decoder pipe carries no
    /// timing of its own — see `tunante_core::clock`, which is where the rules
    /// about seeking a paused track live and are tested.
    clock: PlayClock,
    duration_ms: u64,
    /// rodio reports an empty player for a moment after a source is appended,
    /// before the mixer starts pulling. Without a grace period the end-of-track
    /// check fires immediately and the queue runs away.
    appended_at: Instant,
    has_source: bool,
    loops: u32,
    fade_ms: u64,
    /// A user-facing intention, not an audio concern: "stop in twenty minutes"
    /// survives skipping tracks, pausing and resuming.
    sleep: SleepTimer,
}

impl Player {
    pub fn new() -> Result<Self, String> {
        let device = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("no audio output: {e}"))?;
        let player = RodioPlayer::connect_new(&device.mixer());
        Ok(Self {
            _device: device,
            player,
            queue: PlayQueue::new(),
            volume: 1.0,
            clock: PlayClock::new(),
            duration_ms: 0,
            appended_at: Instant::now(),
            has_source: false,
            loops: 2,
            fade_ms: 8_000,
            sleep: SleepTimer::new(),
        })
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.queue.set_tracks(tracks);
    }

    pub fn play_index(&mut self, index: usize) -> Result<(), String> {
        let track = self
            .queue
            .play_index(index)
            .cloned()
            .ok_or_else(|| format!("no track at {index}"))?;
        self.start(&track, true)
    }

    /// `autoplay` false loads the track without ever unpausing.
    ///
    /// It exists for [`Player::restore`], which used to call this and then
    /// `pause()` immediately after. Microseconds apart, but the source is
    /// already appended and the mixer can pull a buffer in between — which is
    /// an audible blip on an app that promised not to make a sound on resume.
    fn start(&mut self, track: &Track, autoplay: bool) -> Result<(), String> {
        // Dropping the old source kills its decoder, which is what returns the
        // console RAM. Done before spawning the next one so the two never hold
        // their emulated machines at the same time.
        self.player.stop();

        let source = PipeSource::open(
            std::path::Path::new(&track.path),
            track.duration_ms,
            self.loops,
            self.fade_ms,
        )?;

        // From the header the decoder just returned, not from the database.
        //
        // They are different numbers for anything that loops: the scan asks
        // `probe --fast`, which reports the length the file declares, while
        // `play` is told `--loops 2 --fade 8000` and produces a stream as long
        // as that makes it. Console music loops by design, so this is most of
        // the library — and taking the database's answer put the progress bar
        // and the media session's duration against the wrong total.
        self.duration_ms = source
            .total_duration()
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.player.append(source);
        self.player.set_volume(self.volume);
        if autoplay {
            self.player.play();
        } else {
            self.player.pause();
        }

        if autoplay {
            self.clock.start();
        } else {
            self.clock.load_paused(Duration::ZERO);
        }
        self.appended_at = Instant::now();
        self.has_source = true;
        Ok(())
    }

    pub fn toggle_play(&mut self) {
        if self.player.is_paused() {
            self.resume();
        } else {
            self.pause();
        }
    }

    pub fn pause(&mut self) {
        if !self.player.is_paused() {
            self.clock.pause();
            self.player.pause();
        }
    }

    pub fn resume(&mut self) {
        if self.player.is_paused() {
            self.clock.resume();
            self.player.play();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.has_source && !self.player.is_paused()
    }

    pub fn next(&mut self) -> bool {
        match self.queue.next() {
            Some(track) => self.start(&track, true).is_ok(),
            None => {
                self.stop();
                false
            }
        }
    }

    pub fn prev(&mut self) -> bool {
        match self.queue.prev() {
            Some(track) => self.start(&track, true).is_ok(),
            None => false,
        }
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.has_source = false;
        self.clock.stop();
        // Cleared too, or the media session keeps publishing the duration of
        // whatever was playing before.
        self.duration_ms = 0;
    }

    /// The helper does the seeking; the clock has to be told separately, or the
    /// progress bar snaps back on the next tick.
    ///
    /// Clamped, and only moved when the seek was accepted: a clock that says
    /// 4:10 of a 3:20 track is worse than one that did not move.
    pub fn seek(&mut self, ms: u64) {
        let pos = Duration::from_millis(ms.min(self.duration_ms.max(1)));
        if self.player.try_seek(pos).is_ok() {
            self.clock.seek(pos);
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.player.set_volume(self.volume);
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.queue.set_repeat(repeat);
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.queue.set_shuffle(on);
    }

    /// Put a track next in line without disturbing what is playing.
    ///
    /// The user queue is a layer over the queue, not a replacement for it: the
    /// folder you were listening to is still there underneath when the
    /// enqueued tracks run out.
    /// How long a track that loops forever lasts.
    ///
    /// The whole repertoire this player exists for loops by design and has no
    /// ending of its own, so these two numbers *are* the duration. They take
    /// effect on the next track: the one playing was already handed its
    /// settings when its decoder was spawned.
    pub fn set_loop_settings(&mut self, loops: u32, fade_ms: u64) {
        self.loops = loops.max(1);
        self.fade_ms = fade_ms;
    }

    pub fn loops(&self) -> u32 {
        self.loops
    }

    pub fn fade_ms(&self) -> u64 {
        self.fade_ms
    }

    /// Play something that was waiting, now, and take it out of the queue.
    ///
    /// The rest of the queue keeps its order and the folder underneath is
    /// untouched: jumping to the third thing waiting should not throw away the
    /// first two.
    pub fn play_queued(&mut self, track_id: &str) -> Result<(), String> {
        let track = self
            .queue
            .get_user_queue()
            .iter()
            .find(|t| t.id == track_id)
            .cloned()
            .ok_or("that track is no longer waiting")?;
        self.queue.dequeue_track(track_id);
        self.start(&track, true)
    }

    pub fn dequeue(&mut self, track_id: &str) {
        self.queue.dequeue_track(track_id);
    }

    pub fn move_in_queue(&mut self, from: usize, to: usize) {
        self.queue.move_in_user_queue(from, to);
    }

    pub fn enqueue(&mut self, track: Track) {
        self.queue.enqueue_track(track);
    }

    /// What is waiting, in order. Does not include what is playing.
    pub fn user_queue(&self) -> &[Track] {
        self.queue.get_user_queue()
    }

    /// Empty the waiting list, leaving what is playing alone.
    ///
    /// Not `clear_queue`: the context underneath — the folder you were
    /// listening to — is not what "clear the queue" means when the queue is the
    /// thing layered on top of it.
    pub fn clear_user_queue(&mut self) {
        self.queue.clear_user_queue();
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn shuffle(&self) -> bool {
        self.queue.shuffle()
    }

    pub fn repeat(&self) -> RepeatMode {
        self.queue.repeat()
    }

    /// The track the session should come back to.
    pub fn current_path(&self) -> Option<String> {
        self.queue.current().map(|t| t.path.clone())
    }

    pub fn start_sleep_timer(&mut self, minutes: u64) {
        self.sleep.start(minutes);
    }

    pub fn cancel_sleep_timer(&mut self) {
        self.sleep.cancel();
    }

    /// Load a queue and land on `index` **paused**, at `position_ms`.
    ///
    /// Resuming a session must not make noise: a phone that starts playing in a
    /// pocket because it was rebooted is worse than one that forgot.
    pub fn restore(&mut self, tracks: Vec<Track>, index: usize, position_ms: u64) -> Result<(), String> {
        self.set_tracks(tracks);
        let track = self
            .queue
            .play_index(index)
            .cloned()
            .ok_or_else(|| format!("no track at {index}"))?;
        self.start(&track, false)?;
        if position_ms > 0 {
            self.player.try_seek(Duration::from_millis(position_ms)).ok();
            self.clock.load_paused(Duration::from_millis(position_ms));
        }
        Ok(())
    }

    pub fn position_ms(&self) -> u64 {
        self.clock.position_ms()
    }

    /// Advance the queue when the current track has run out.
    ///
    /// Returns true if the track changed, so the caller knows to refresh the
    /// notification and the media session rather than doing it every tick.
    pub fn tick(&mut self) -> bool {
        if !self.has_source || self.player.is_paused() {
            return false;
        }
        // Counted against wall-clock ticks rather than against played samples,
        // which is what the user means: "in twenty minutes", not "after twenty
        // minutes of audio".
        if self.sleep.tick(TICK_MS) {
            // Stopped, not paused, which is what tunante-mini does. Pausing
            // would leave the decoder process alive all night holding its
            // console's RAM — killing it is how that memory comes back.
            self.stop();
            log::info!("sleep timer ran out");
            return true;
        }
        // Covers the gap between appending a source and the mixer pulling from
        // it, during which rodio reports the track as already over.
        if self.appended_at.elapsed() < Duration::from_millis(500) {
            return false;
        }
        if self.player.empty() {
            self.next();
            return true;
        }
        false
    }

    pub fn state(&self) -> serde_json::Value {
        let current = self.queue.current();
        serde_json::json!({
            "ok": true,
            "playing": self.is_playing(),
            "hasSource": self.has_source,
            "positionMs": self.position_ms(),
            "durationMs": self.duration_ms,
            "index": self.queue.current_index(),
            "queueLen": self.queue.tracks().len(),
            "shuffle": self.queue.shuffle(),
            "repeat": self.queue.repeat() as u8,
            "volume": self.volume,
            "sleepMinutes": if self.sleep.is_running() { self.sleep.remaining_minutes() } else { 0 },
            "queued": self.queue.get_user_queue().len(),
            "loops": self.loops,
            "fadeSeconds": self.fade_ms / 1000,
            "queuedNext": self.queue.get_user_queue().first().map(|t| {
                if t.title.is_empty() { t.path.rsplit('/').next().unwrap_or("").to_string() }
                else { t.title.clone() }
            }).unwrap_or_default(),
            "title": current.map(|t| t.title.clone()).unwrap_or_default(),
            "artist": current.map(|t| t.artist.clone()).unwrap_or_default(),
            "album": current.map(|t| t.album.clone()).unwrap_or_default(),
            "path": current.map(|t| t.path.clone()).unwrap_or_default(),
        })
    }
}
