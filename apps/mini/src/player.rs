//! The audio output, and the queue on top of it.
//!
//! Since fase 3a of docs/plan-desktop-slint.md the output half is
//! [`tunante_audio::AudioEngine`] — the same engine the desktop app runs on:
//! decoding stays out of process in `tunante-decoder`, and on top of what this
//! file used to do itself the engine brings output-device selection, recovery
//! when the device dies under us (Bluetooth, unplugs), and the DSP chain
//! installed over every source. What is left here is the queue and the
//! adaptation between "what a screen asks" and "what the engine does".

use std::path::Path;

use tunante_audio::AudioEngine;
use tunante_core::db::models::Track;
use tunante_core::{PlayQueue, RepeatMode};

pub struct Player {
    engine: AudioEngine,
    queue: PlayQueue,
    /// How long a looping track should last. Console music mostly loops by
    /// design, so the player has to decide when to stop. Mirrored into the
    /// engine; kept here too because the settings screen reads it back.
    loops: u32,
    fade_ms: u64,
}

impl Player {
    pub fn new() -> Result<Self, String> {
        let engine = AudioEngine::new().map_err(|e| format!("no audio output: {e}"))?;
        let mut p = Self {
            engine,
            queue: PlayQueue::new(),
            loops: 2,
            fade_ms: 8_000,
        };
        // The engine's default is the desktop's 0.8; this app has always
        // started at full volume and the session restore adjusts it after.
        p.engine.set_volume(1.0);
        p.engine.set_loop_settings(p.loops, p.fade_ms);
        Ok(p)
    }

    /// The engine itself, for the screens that talk to it directly: output
    /// device selection and the DSP chain.
    pub fn engine_mut(&mut self) -> &mut AudioEngine {
        &mut self.engine
    }

    /// Rebuild the output if the device died or the system default moved —
    /// call it every few seconds from the UI timer. Returns the new device
    /// name when a rebuild happened.
    pub fn reconcile_output(&mut self) -> Option<String> {
        self.engine.reconcile_output()
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

        self.engine
            .play_file(Path::new(&path), hint)
            .map_err(|e| e.to_string())
    }

    pub fn toggle_play(&mut self) {
        if self.engine.is_playing() {
            self.engine.pause();
        } else {
            self.engine.resume();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.engine.is_playing()
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
        self.engine.stop();
    }

    pub fn set_volume(&mut self, v: f32) {
        self.engine.set_volume(v);
    }

    pub fn volume(&self) -> f32 {
        self.engine.volume()
    }

    /// Applies from the next track on: changing it mid-track would mean
    /// restarting the decoder, and losing your place to change a setting is a
    /// worse trade than waiting for the next song.
    pub fn set_loop_settings(&mut self, loops: u32, fade_ms: u64) {
        self.loops = loops;
        self.fade_ms = fade_ms;
        self.engine.set_loop_settings(loops, fade_ms);
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
    /// Clamped to the known duration so a drag past the end lands on the end;
    /// the engine moves its own wall clock with the request, so the progress
    /// bar lands where the finger left it rather than snapping back while the
    /// helper catches up.
    pub fn seek(&mut self, ms: u64) {
        let _ = self.engine.seek(ms.min(self.engine.duration_ms()));
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
        self.engine.position_ms()
    }

    pub fn duration_ms(&self) -> u64 {
        self.engine.duration_ms()
    }

    /// Advance the queue if the current track has run out.
    ///
    /// Call this on a timer from the UI thread. The engine's own grace period
    /// covers the gap between appending a source and the mixer starting to
    /// pull from it, during which rodio would otherwise report the track as
    /// already over.
    pub fn poll_track_end(&mut self) -> bool {
        if self.engine.track_finished() {
            let _ = self.next();
            return true;
        }
        false
    }
}
