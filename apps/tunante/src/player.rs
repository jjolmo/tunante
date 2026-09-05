//! The audio output, and the queue on top of it.
//!
//! The output half is [`tunante_audio::AudioEngine`], shared by every app:
//! decoding stays out of process in `tunante-decoder`, and on top of what this
//! file used to do itself the engine brings output-device selection, recovery
//! when the device dies under us (Bluetooth, unplugs), and the DSP chain
//! installed over every source. What is left here is the queue and the
//! adaptation between "what a screen asks" and "what the engine does".

use std::path::Path;

use tunante_audio::AudioEngine;
use tunante_core::db::models::Track;
use tunante_core::{PlayQueue, RepeatMode};

/// A crossfade in flight. The desktop ran this on a thread against a Mutex;
/// here the player lives in an `Rc`, so the fade is a state machine stepped
/// by a 25 ms Slint timer that only runs while there is one.
enum Fade {
    Idle,
    /// Ramping the old track down; the new one starts when it reaches zero.
    Out {
        step: u32,
        steps: u32,
        path: String,
        hint: i64,
    },
    /// Ramping the new track up to the user's volume.
    In { step: u32, steps: u32 },
}

pub struct Player {
    engine: AudioEngine,
    queue: PlayQueue,
    /// How long a looping track should last. Console music mostly loops by
    /// design, so the player has to decide when to stop. Mirrored into the
    /// engine; kept here too because the settings screen reads it back.
    loops: u32,
    fade_ms: u64,
    fade: Fade,
    /// Wakes the fade timer the moment a crossfade begins — set by main.rs
    /// once the timer exists, because the player is built first.
    fade_kick: Option<Box<dyn Fn()>>,
    /// The track actually sounding. Usually the queue's current, but a
    /// user-queued "play next" track plays while the context index stays
    /// put — the queue is the map, this is where the needle is.
    now: Option<Track>,
}

impl Player {
    pub fn new() -> Result<Self, String> {
        let engine = AudioEngine::new().map_err(|e| format!("no audio output: {e}"))?;
        let mut p = Self {
            engine,
            queue: PlayQueue::new(),
            loops: 2,
            fade_ms: 8_000,
            fade: Fade::Idle,
            fade_kick: None,
            now: None,
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

    pub fn set_fade_kick(&mut self, kick: impl Fn() + 'static) {
        self.fade_kick = Some(Box::new(kick));
    }

    /// One 25 ms step of the crossfade. Returns whether one is still running,
    /// so the timer that drives it can stop itself.
    pub fn tick_fade(&mut self) -> bool {
        let user = self.engine.volume();
        match &mut self.fade {
            Fade::Idle => false,
            Fade::Out { step, steps, path, hint } => {
                *step += 1;
                let t = *step as f32 / *steps as f32;
                self.engine.set_player_volume_raw(user * (1.0 - t));
                if *step >= *steps {
                    let (path, hint, steps) = (path.clone(), *hint, *steps);
                    match self
                        .engine
                        .play_file_at_volume(std::path::Path::new(&path), hint, 0.0)
                    {
                        Ok(()) => self.fade = Fade::In { step: 0, steps },
                        Err(e) => {
                            eprintln!("no se pudo reproducir: {e}");
                            self.engine.set_player_volume_raw(user);
                            self.fade = Fade::Idle;
                            return false;
                        }
                    }
                }
                true
            }
            Fade::In { step, steps } => {
                *step += 1;
                let t = *step as f32 / *steps as f32;
                self.engine.set_player_volume_raw(user * t.min(1.0));
                if *step >= *steps {
                    self.engine.set_player_volume_raw(user);
                    self.fade = Fade::Idle;
                    return false;
                }
                true
            }
        }
    }

    /// A fade interrupted by pause or stop must not leave the queue saying
    /// one thing and the speakers another: an Out that never switched jumps
    /// to the new track immediately (silently — the caller pauses next), and
    /// the raw volume always returns to the user's.
    fn settle_fade(&mut self) {
        match std::mem::replace(&mut self.fade, Fade::Idle) {
            Fade::Idle => {}
            Fade::Out { path, hint, .. } => {
                if let Err(e) = self
                    .engine
                    .play_file_at_volume(std::path::Path::new(&path), hint, self.engine.volume())
                {
                    eprintln!("no se pudo reproducir: {e}");
                }
            }
            Fade::In { .. } => {
                let user = self.engine.volume();
                self.engine.set_player_volume_raw(user);
            }
        }
    }

    pub fn queue(&self) -> &PlayQueue {
        &self.queue
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.queue.set_tracks(tracks);
    }

    /// Forget what was queued by hand. `set_tracks` deliberately keeps it (the
    /// context and the queue are two layers); the touch shells call this first
    /// when a tap on a category is meant to replace everything.
    pub fn clear_user_queue(&mut self) {
        self.queue.clear_user_queue();
    }

    pub fn current(&self) -> Option<&Track> {
        self.now.as_ref().or_else(|| self.queue.current())
    }

    /// "And *right* after this one": the priority queue the context never
    /// sees. Consumed by `next()` before anything else, shuffle included.
    pub fn play_next(&mut self, tracks: Vec<Track>) {
        for t in tracks {
            self.queue.enqueue_track(t);
        }
    }

    pub fn user_queue(&self) -> &[Track] {
        self.queue.get_user_queue()
    }

    /// Remove the ith user-queued track, returning it.
    pub fn dequeue_user(&mut self, index: usize) -> Option<Track> {
        let t = self.queue.get_user_queue().get(index)?.clone();
        self.queue.dequeue_track(&t.id);
        Some(t)
    }

    pub fn move_user(&mut self, from: usize, to: usize) {
        self.queue.move_in_user_queue(from, to);
    }

    pub fn set_continue_from_queue(&mut self, on: bool) {
        self.queue.set_continue_from_queue(on);
    }

    /// A user-queued track just played that the context does not contain;
    /// with continue-from-queue on, core asks the caller to bring the
    /// track's own context in. `adopt_context` is what answers (and clears
    /// the request); an unanswered one is cleared by the next ordinary
    /// advance, which is core's own rule.
    pub fn take_pending_context(&self) -> Option<Track> {
        self.queue.pending_context_update().cloned()
    }

    pub fn adopt_context(&mut self, tracks: Vec<Track>, current_id: &str) {
        self.queue.update_context(tracks, current_id);
    }

    /// Play a user-queued row right now, jumping the line it was already in.
    pub fn play_user(&mut self, index: usize) -> Result<(), String> {
        match self.dequeue_user(index) {
            Some(t) => self.start_track(t),
            None => Ok(()),
        }
    }

    /// Play the track at `index` in the queue.
    pub fn play_index(&mut self, index: usize) -> Result<(), String> {
        self.queue.play_index(index);
        self.play_current()
    }

    fn play_current(&mut self) -> Result<(), String> {
        match self.queue.current().cloned() {
            Some(t) => self.start_track(t),
            None => Ok(()),
        }
    }

    /// The one place a track starts. `next()`/`prev()` must come through here
    /// with the track the queue *returned*: the user queue plays things that
    /// are not the context's current, and re-reading `queue.current()` after a
    /// pop would play the wrong file.
    fn start_track(&mut self, track: Track) -> Result<(), String> {
        let (path, hint) = (track.path.clone(), track.duration_ms);
        self.now = Some(track);

        // A fade is a transition: it only makes sense when something is
        // audible. Starting from stopped or paused plays immediately — the
        // same gate the desktop's should_fade tests pinned down.
        if self.engine.fade_on_track_change()
            && self.engine.fade_seconds() > 0.0
            && self.engine.is_playing()
        {
            let half_ms = (self.engine.fade_seconds() * 500.0) as u32;
            let steps = (half_ms / 25).max(1);
            self.fade = Fade::Out { step: 0, steps, path, hint };
            if let Some(kick) = &self.fade_kick {
                kick();
            }
            return Ok(());
        }

        self.fade = Fade::Idle;
        self.engine
            .play_file(Path::new(&path), hint)
            .map_err(|e| e.to_string())
    }

    pub fn toggle_play(&mut self) {
        self.settle_fade();
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
        match self.queue.next() {
            Some(t) => self.start_track(t),
            None => {
                self.stop();
                Ok(())
            }
        }
    }

    pub fn prev(&mut self) -> Result<(), String> {
        match self.queue.prev() {
            Some(t) => self.start_track(t),
            None => Ok(()),
        }
    }

    pub fn stop(&mut self) {
        self.fade = Fade::Idle;
        self.now = None;
        let user = self.engine.volume();
        self.engine.set_player_volume_raw(user);
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

    /// Skip tracks shorter than the threshold when the queue advances on its
    /// own. 0 disables. Console rips are full of two-second jingles and SFX
    /// rows, and this is the knob that keeps them out of a listening session.
    pub fn set_short_filter(&mut self, threshold_ms: i64) {
        self.queue.set_short_filter(threshold_ms);
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
