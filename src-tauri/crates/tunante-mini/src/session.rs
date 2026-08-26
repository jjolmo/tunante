//! Remembering where you were, and the sleep timer.
//!
//! Both live in the `settings` table the core schema already has, so there is no
//! new storage and no new file to lose.

use tunante_core::db::Database;

const KEY_TRACK: &str = "mini.last_track";
const KEY_POSITION: &str = "mini.last_position_ms";
const KEY_VOLUME: &str = "mini.volume";
const KEY_SHUFFLE: &str = "mini.shuffle";
const KEY_REPEAT: &str = "mini.repeat";

pub struct Session {
    pub track_path: Option<String>,
    pub position_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: u8,
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
    ) {
        let _ = db.set_setting(KEY_TRACK, track_path.unwrap_or(""));
        let _ = db.set_setting(KEY_POSITION, &position_ms.to_string());
        let _ = db.set_setting(KEY_VOLUME, &volume.to_string());
        let _ = db.set_setting(KEY_SHUFFLE, if shuffle { "1" } else { "0" });
        let _ = db.set_setting(KEY_REPEAT, &repeat.to_string());
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
