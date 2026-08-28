//! Remembering where you were, and the sleep timer.
//!
//! Both live in the `settings` table the core schema already has, so there is no
//! new storage and no new file to lose.
//!
//! Shared by `tunante-mini` and `tunante-android`. The keys still say `mini.`
//! because they are already written in the databases on the phone that runs it,
//! and renaming them would silently lose everyone's resume position for the
//! sake of a tidier string. Each app has its own database file anyway.

use crate::db::Database;

const KEY_TRACK: &str = "mini.last_track";
const KEY_POSITION: &str = "mini.last_position_ms";
const KEY_VOLUME: &str = "mini.volume";
const KEY_SHUFFLE: &str = "mini.shuffle";
const KEY_REPEAT: &str = "mini.repeat";
const KEY_LOOPS: &str = "mini.loop_count";
const KEY_FADE: &str = "mini.fade_seconds";

pub struct Session {
    pub track_path: Option<String>,
    pub position_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: u8,
    /// How many times a track that loops forever is played through. 0 is
    /// "forever", which is a real choice for background music.
    pub loops: u32,
    /// Seconds of fade at the end of a looped track. 0 is a hard stop.
    pub fade_seconds: u64,
}

impl Session {
    pub fn load(db: &Database) -> Self {
        let get = |k: &str| db.get_setting(k).ok().flatten();

        Self {
            track_path: get(KEY_TRACK).filter(|s| !s.is_empty()),
            position_ms: get(KEY_POSITION).and_then(|s| s.parse().ok()).unwrap_or(0),
            // Not silence by default: a fresh install that plays nothing
            // audible reads as broken rather than as quiet.
            volume: get(KEY_VOLUME).and_then(|s| s.parse().ok()).unwrap_or(1.0),
            shuffle: get(KEY_SHUFFLE).map(|s| s == "1").unwrap_or(false),
            repeat: get(KEY_REPEAT).and_then(|s| s.parse().ok()).unwrap_or(0),
            // Two loops and an eight-second fade: the usual choice for a
            // chiptune rip, and what both apps started life hardcoded to.
            loops: get(KEY_LOOPS).and_then(|s| s.parse().ok()).unwrap_or(2),
            fade_seconds: get(KEY_FADE).and_then(|s| s.parse().ok()).unwrap_or(8),
        }
    }

    /// Write the session back.
    ///
    /// Called on a timer while playing, not only on exit: a phone app is killed
    /// by the system far more often than it is closed by the user, and a
    /// resume that only works on a clean exit is a resume that rarely works.
    pub fn save(
        db: &Database,
        track_path: Option<&str>,
        position_ms: u64,
        volume: f32,
        shuffle: bool,
        repeat: u8,
        loops: u32,
        fade_seconds: u64,
    ) {
        let _ = db.set_setting(KEY_TRACK, track_path.unwrap_or(""));
        let _ = db.set_setting(KEY_POSITION, &position_ms.to_string());
        let _ = db.set_setting(KEY_VOLUME, &volume.to_string());
        let _ = db.set_setting(KEY_SHUFFLE, if shuffle { "1" } else { "0" });
        let _ = db.set_setting(KEY_REPEAT, &repeat.to_string());
        let _ = db.set_setting(KEY_LOOPS, &loops.to_string());
        let _ = db.set_setting(KEY_FADE, &fade_seconds.to_string());
    }
}

/// Counts down to silence.
///
/// Kept here rather than in the player because it is a user-facing intention,
/// not an audio concern: "stop in twenty minutes" survives skipping tracks,
/// pausing and resuming.
pub struct SleepTimer {
    remaining_ms: u64,
    running: bool,
}

impl SleepTimer {
    pub fn new() -> Self {
        Self { remaining_ms: 0, running: false }
    }

    pub fn start(&mut self, minutes: u64) {
        self.remaining_ms = minutes * 60_000;
        self.running = minutes > 0;
    }

    pub fn cancel(&mut self) {
        self.running = false;
        self.remaining_ms = 0;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn remaining_minutes(&self) -> u64 {
        // Round up, so a timer with thirty seconds left reads "1" rather than
        // "0" while music is still playing.
        self.remaining_ms.div_ceil(60_000)
    }

    /// Advance by `elapsed_ms`. Returns true exactly once, when it runs out.
    pub fn tick(&mut self, elapsed_ms: u64) -> bool {
        if !self.running {
            return false;
        }
        self.remaining_ms = self.remaining_ms.saturating_sub(elapsed_ms);
        if self.remaining_ms == 0 {
            self.running = false;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::SleepTimer;

    /// The tick interval the phone apps drive this with.
    const TICK: u64 = 500;

    #[test]
    fn a_timer_that_was_never_started_never_fires() {
        let mut t = SleepTimer::new();
        assert!(!t.is_running());
        for _ in 0..10_000 {
            assert!(!t.tick(TICK), "an idle timer must not fire");
        }
    }

    #[test]
    fn zero_minutes_means_cancel_not_fire_immediately() {
        // "Off" and "stop right now" are one tap apart in the UI, and only one
        // of them should ever silence the music.
        let mut t = SleepTimer::new();
        t.start(0);
        assert!(!t.is_running());
        assert!(!t.tick(TICK));
    }

    #[test]
    fn it_fires_exactly_once_after_the_time_asked_for() {
        let mut t = SleepTimer::new();
        t.start(1);

        let mut fired = 0;
        // Two minutes of ticks: it must go off inside the first, and then stay
        // off however long the app keeps ticking.
        for _ in 0..(2 * 60 * 1000 / TICK) {
            if t.tick(TICK) {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "a sleep timer that fires twice pauses a track you just resumed");
        assert!(!t.is_running());
    }

    #[test]
    fn the_remaining_minutes_round_up_while_there_is_still_music() {
        let mut t = SleepTimer::new();
        t.start(2);
        assert_eq!(t.remaining_minutes(), 2);

        // Ninety seconds in: thirty seconds left, and it must not read "0"
        // while the music is still playing.
        for _ in 0..(90 * 1000 / TICK) {
            t.tick(TICK);
        }
        assert_eq!(t.remaining_minutes(), 1);
    }

    #[test]
    fn cancelling_stops_it_dead() {
        let mut t = SleepTimer::new();
        t.start(5);
        t.cancel();
        assert!(!t.is_running());
        assert_eq!(t.remaining_minutes(), 0);
        for _ in 0..(10 * 60 * 1000 / TICK) {
            assert!(!t.tick(TICK));
        }
    }
}
