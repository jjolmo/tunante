//! The audio output, and the queue on top of it.
//!
//! Deliberately thin. Everything hard about decoding happens in another process
//! (see [`crate::decoder`]); what is left here is opening one ALSA client, keeping
//! it open for the life of the app, and pushing sources at it.
//!
//! One ALSA client for the whole session matters on this hardware: handing the
//! device back and re-taking it on every track change is what produces the click
//! between tracks.

use std::path::Path;
use std::time::{Duration, Instant};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player as RodioPlayer, Source};
use tunante_core::db::models::Track;
use tunante_core::{PlayQueue, RepeatMode};

use crate::decoder::PipeSource;

pub struct Player {
    _device: MixerDeviceSink,
    player: RodioPlayer,
    queue: PlayQueue,
    volume: f32,
    /// Wall-clock position. The decoder pipe carries no timing of its own, and
    /// this is accurate enough for a progress bar.
    started_at: Option<Instant>,
    accumulated: Duration,
    duration_ms: u64,
    /// rodio reports an empty player for a moment after a source is appended,
    /// before the mixer starts pulling. Without a short grace period the
    /// end-of-track check fires immediately and the queue runs away.
    appended_at: Instant,
    has_source: bool,
    /// How long a looping track should last. Console music mostly loops by
    /// design, so the player has to decide when to stop.
    loops: u32,
    fade_ms: u64,
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
            started_at: None,
            accumulated: Duration::ZERO,
            duration_ms: 0,
            appended_at: Instant::now(),
            has_source: false,
            loops: 2,
            fade_ms: 8_000,
        })
    }

    pub fn queue(&self) -> &PlayQueue {
        &self.queue
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.queue.set_tracks(tracks);
    }

    pub fn current(&self) -> Option<&Track> {
        self.queue.current()
    }

    /// Play the track at `index` in the queue.
    pub fn play_index(&mut self, index: usize) -> Result<(), String> {
        self.queue.play_index(index);
        self.play_current()
    }

    fn play_current(&mut self) -> Result<(), String> {
        let (path, hint) = match self.queue.current() {
            Some(t) => (t.path.clone(), t.duration_ms),
            None => return Ok(()),
        };

        // Dropping the old source kills the old helper process, which is what
        // frees its console RAM. No sleep is needed here — unlike the in-process
        // desktop engine, there are no C globals to tear down first.
        self.player.stop();

        let source = PipeSource::open(Path::new(&path), hint, self.loops, self.fade_ms)?;
        self.duration_ms = source
            .total_duration()
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.player.append(source);
        self.player.set_volume(self.volume);
        self.player.play();

        self.started_at = Some(Instant::now());
        self.accumulated = Duration::ZERO;
        self.appended_at = Instant::now();
        self.has_source = true;
        Ok(())
    }

    pub fn toggle_play(&mut self) {
        if self.player.is_paused() {
            self.player.play();
            if self.started_at.is_none() {
                self.started_at = Some(Instant::now());
            }
        } else {
            self.player.pause();
            if let Some(s) = self.started_at.take() {
                self.accumulated += s.elapsed();
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        self.has_source && !self.player.is_paused()
    }

    pub fn next(&mut self) -> Result<(), String> {
        if self.queue.next().is_some() {
            self.play_current()
        } else {
            self.stop();
            Ok(())
        }
    }

    pub fn prev(&mut self) -> Result<(), String> {
        if self.queue.prev().is_some() {
            self.play_current()
        } else {
            Ok(())
        }
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.has_source = false;
        self.started_at = None;
        self.accumulated = Duration::ZERO;
        self.duration_ms = 0;
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.player.set_volume(self.volume);
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Applies from the next track on: changing it mid-track would mean
    /// restarting the decoder, and losing your place to change a setting is a
    /// worse trade than waiting for the next song.
    pub fn set_loop_settings(&mut self, loops: u32, fade_ms: u64) {
        self.loops = loops;
        self.fade_ms = fade_ms;
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.queue.set_repeat(mode);
    }

    pub fn repeat(&self) -> RepeatMode {
        self.queue.repeat()
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.queue.set_shuffle(on);
    }

    pub fn shuffle(&self) -> bool {
        self.queue.shuffle()
    }

    /// Jump within the current track.
    ///
    /// The wall-clock position is moved to match immediately, so the progress
    /// bar lands where the finger left it rather than snapping back while the
    /// helper catches up.
    pub fn seek(&mut self, ms: u64) {
        let pos = Duration::from_millis(ms.min(self.duration_ms));
        if self.player.try_seek(pos).is_ok() {
            self.accumulated = pos;
            if self.started_at.is_some() {
                self.started_at = Some(Instant::now());
            }
        }
    }

    /// Put a track at the end of the queue.
    ///
    /// Appends to the context rather than to the priority queue: swiping a track
    /// in the library means "and then this one", which is a different intent
    /// from the desktop's play-next queue.
    pub fn enqueue(&mut self, track: Track) {
        self.enqueue_many(vec![track]);
    }

    /// Put a batch at the end of the queue, without touching what is playing.
    ///
    /// One copy of the context and one `update_context` for the whole batch, not
    /// one per track. Per-track it is quadratic — every call clones the entire
    /// vector and regenerates the shuffle permutation — and the batches here are
    /// not small: a folder tree or a whole playlist is thousands of tracks.
    pub fn enqueue_many(&mut self, more: Vec<Track>) {
        if more.is_empty() {
            return;
        }
        let mut tracks = self.queue.tracks().to_vec();
        let current = self.queue.current().map(|t| t.id.clone());
        tracks.extend(more);
        match current {
            Some(id) => self.queue.update_context(tracks, &id),
            // Nothing playing, so there is no current track to keep hold of. This
            // leaves `current_index` at None: a full queue and silence, which is
            // exactly what "add to the queue" should do on its own.
            None => self.queue.set_tracks(tracks),
        }
    }

    /// Empty the queue and stop.
    ///
    /// Unlike removing a single row, this does stop the music. Leaving a track
    /// playing with nothing behind it would put the player in a state the queue
    /// no longer explains, and "empty the queue" plainly means "stop".
    ///
    /// Both levels go: the context the queue was built from and the user queue
    /// layered on top, which does not appear in `tracks()` and would otherwise
    /// survive to start playing on its own.
    pub fn clear_queue(&mut self) {
        self.stop();
        self.queue.set_tracks(Vec::new());
        self.queue.clear_user_queue();
    }

    /// Take a track out of the queue.
    ///
    /// Removing the one that is playing leaves it playing: stopping the music
    /// because a row was swiped would be a surprise, and the track is gone from
    /// the list either way.
    pub fn remove_from_queue(&mut self, index: usize) {
        let mut tracks = self.queue.tracks().to_vec();
        if index >= tracks.len() {
            return;
        }
        let current = self.queue.current().map(|t| t.id.clone());
        tracks.remove(index);

        match current {
            Some(id) if tracks.iter().any(|t| t.id == id) => {
                self.queue.update_context(tracks, &id)
            }
            _ => self.queue.set_tracks(tracks),
        }
    }

    /// Move a track to another position in the queue.
    ///
    /// The playing track keeps playing wherever it lands: reordering a list is
    /// housekeeping, and stopping the music for it would be a surprise.
    pub fn reorder(&mut self, from: usize, to: usize) {
        let mut tracks = self.queue.tracks().to_vec();
        if from >= tracks.len() || from == to {
            return;
        }
        let to = to.min(tracks.len().saturating_sub(1));
        let track = tracks.remove(from);
        tracks.insert(to, track);

        match self.queue.current().map(|t| t.id.clone()) {
            Some(id) => self.queue.update_context(tracks, &id),
            None => self.queue.set_tracks(tracks),
        }
    }

    /// Index of the current track in the queue, for marking it in the UI.
    pub fn current_index(&self) -> Option<usize> {
        self.queue.current_index()
    }

    pub fn position_ms(&self) -> u64 {
        let running = self.started_at.map(|s| s.elapsed()).unwrap_or(Duration::ZERO);
        (self.accumulated + running).as_millis() as u64
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    /// Advance the queue if the current track has run out.
    ///
    /// Call this on a timer from the UI thread. The grace period covers the gap
    /// between appending a source and the mixer starting to pull from it, during
    /// which rodio would otherwise report the track as already over.
    pub fn poll_track_end(&mut self) -> bool {
        if !self.has_source || self.player.is_paused() {
            return false;
        }
        if self.appended_at.elapsed() < Duration::from_millis(500) {
            return false;
        }
        if self.player.empty() {
            let _ = self.next();
            return true;
        }
        false
    }
}
