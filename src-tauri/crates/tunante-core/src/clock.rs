//! Where a track is, measured against the wall.
//!
//! Not against the audio device. The decoder arrives as raw PCM down a pipe and
//! carries no timing of its own, so both phone apps count elapsed real time
//! instead. That is accurate enough for a progress bar and for what a media
//! session reports, and it is wrong by exactly the buffer the mixer is holding.
//!
//! Small enough to look obviously right and it was not: the Android player
//! restarted the clock on every seek, so dragging the bar on a paused track
//! left the position climbing with nothing playing. `tunante-mini` had it right
//! — this is mini's version, with tests, in one place so the two cannot drift
//! apart again.

use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct PlayClock {
    /// When the current run began. `None` means stopped or paused.
    started_at: Option<Instant>,
    /// Everything counted before the current run.
    accumulated: Duration,
}

impl PlayClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new track, playing from the beginning.
    pub fn start(&mut self) {
        self.accumulated = Duration::ZERO;
        self.started_at = Some(Instant::now());
    }

    /// A new track, loaded at `at` and **not** playing.
    ///
    /// What resuming a saved session needs: the position is known, and nothing
    /// should be counting until somebody presses play.
    pub fn load_paused(&mut self, at: Duration) {
        self.accumulated = at;
        self.started_at = None;
    }

    pub fn pause(&mut self) {
        if let Some(at) = self.started_at.take() {
            self.accumulated += at.elapsed();
        }
    }

    pub fn resume(&mut self) {
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }
    }

    /// Jump to `to`, keeping whatever the clock was doing.
    ///
    /// Running stays running, paused stays paused. Restarting it here is the
    /// bug this module was written for: the bar crept forward on a track that
    /// was not playing.
    pub fn seek(&mut self, to: Duration) {
        self.accumulated = to;
        if self.started_at.is_some() {
            self.started_at = Some(Instant::now());
        }
    }

    pub fn stop(&mut self) {
        self.started_at = None;
        self.accumulated = Duration::ZERO;
    }

    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn position(&self) -> Duration {
        let running = self.started_at.map(|s| s.elapsed()).unwrap_or(Duration::ZERO);
        self.accumulated + running
    }

    pub fn position_ms(&self) -> u64 {
        self.position().as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long enough to be measurable, short enough not to slow the suite.
    const TICK: Duration = Duration::from_millis(30);

    #[test]
    fn a_fresh_clock_is_at_zero_and_stopped() {
        let c = PlayClock::new();
        assert_eq!(c.position(), Duration::ZERO);
        assert!(!c.is_running());
    }

    #[test]
    fn a_paused_clock_does_not_move() {
        let mut c = PlayClock::new();
        c.start();
        std::thread::sleep(TICK);
        c.pause();

        let at = c.position();
        assert!(at >= TICK, "it should have counted the time it ran");
        std::thread::sleep(TICK);
        assert_eq!(c.position(), at, "a paused clock counted anyway");
    }

    /// The bug. Seeking is not resuming.
    #[test]
    fn seeking_while_paused_leaves_it_paused() {
        let mut c = PlayClock::new();
        c.start();
        c.pause();

        c.seek(Duration::from_secs(30));
        assert!(!c.is_running(), "a seek started the clock on a paused track");

        std::thread::sleep(TICK);
        assert_eq!(
            c.position(),
            Duration::from_secs(30),
            "the position crept forward with nothing playing"
        );
    }

    #[test]
    fn seeking_while_playing_keeps_playing_from_there() {
        let mut c = PlayClock::new();
        c.start();
        c.seek(Duration::from_secs(30));
        assert!(c.is_running());

        std::thread::sleep(TICK);
        let p = c.position();
        assert!(p >= Duration::from_secs(30) + TICK, "{p:?}");
        assert!(p < Duration::from_secs(31), "it jumped, it did not add: {p:?}");
    }

    #[test]
    fn resuming_twice_does_not_lose_the_time_already_counted() {
        let mut c = PlayClock::new();
        c.start();
        std::thread::sleep(TICK);
        c.pause();
        let at = c.position();

        c.resume();
        c.resume(); // the second one must be a no-op, not a restart
        std::thread::sleep(TICK);
        assert!(
            c.position() >= at + TICK,
            "a second resume threw away what had already been counted"
        );
    }

    /// Resuming a saved session: the position is known and nothing is playing.
    #[test]
    fn a_loaded_position_is_held_until_something_presses_play() {
        let mut c = PlayClock::new();
        c.load_paused(Duration::from_secs(90));

        assert!(!c.is_running());
        std::thread::sleep(TICK);
        assert_eq!(c.position(), Duration::from_secs(90));

        c.resume();
        std::thread::sleep(TICK);
        assert!(c.position() >= Duration::from_secs(90) + TICK);
    }

    #[test]
    fn pausing_a_paused_clock_changes_nothing() {
        let mut c = PlayClock::new();
        c.start();
        std::thread::sleep(TICK);
        c.pause();
        let at = c.position();
        c.pause();
        assert_eq!(c.position(), at);
    }

    #[test]
    fn stopping_puts_it_back_to_nothing() {
        let mut c = PlayClock::new();
        c.start();
        std::thread::sleep(TICK);
        c.stop();
        assert_eq!(c.position(), Duration::ZERO);
        assert!(!c.is_running());
    }
}
