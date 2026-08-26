//! Peak limiter.

use super::{DspProcessor, DspSettings};

/// Highest peak the limiter will let through.
const CEILING: f32 = 0.98;
/// Attack time constant: fast enough to catch a transient before it clips.
const ATTACK_SECS: f32 = 0.005;
/// Release time constant: slow enough not to pump on sustained material.
const RELEASE_SECS: f32 = 0.150;

/// Feed-forward peak limiter with a smoothed gain envelope.
///
/// Its job is to make the preamp and the equalizer safe to push: both can drive
/// the signal past full scale, and without this you'd get hard digital clipping
/// straight into the mixer. Gain is shared across all channels so the stereo image
/// doesn't shift when only one side is loud.
///
/// There is no look-ahead, so a single very fast transient can still poke above
/// the ceiling for a handful of samples; the final clamp catches that. For a music
/// player that trade-off is right — look-ahead would add latency to every seek and
/// track change.
pub struct Limiter {
    settings: DspSettings,
    gain: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
    cached_rate: u32,
}

impl Limiter {
    pub fn new(settings: DspSettings) -> Self {
        Self {
            settings,
            gain: 1.0,
            attack_coefficient: 0.0,
            release_coefficient: 0.0,
            cached_rate: 0,
        }
    }

    fn retune(&mut self, sample_rate: u32) {
        let rate = sample_rate as f32;
        self.attack_coefficient = 1.0 - (-1.0 / (rate * ATTACK_SECS)).exp();
        self.release_coefficient = 1.0 - (-1.0 / (rate * RELEASE_SECS)).exp();
        self.cached_rate = sample_rate;
    }
}

impl DspProcessor for Limiter {
    fn id(&self) -> &'static str {
        "limiter"
    }

    fn is_active(&self) -> bool {
        self.settings.limiter.get()
    }

    fn process(&mut self, frame: &mut [f32], sample_rate: u32) {
        if sample_rate != self.cached_rate {
            self.retune(sample_rate);
        }

        let peak = frame.iter().fold(0.0f32, |max, s| max.max(s.abs()));
        let target = if peak > CEILING { CEILING / peak } else { 1.0 };

        // Clamping down happens fast, letting go happens slowly.
        let coefficient = if target < self.gain {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.gain += (target - self.gain) * coefficient;

        for sample in frame.iter_mut() {
            // The clamp is the backstop for transients the envelope hasn't caught
            // up with yet — without look-ahead, a few samples can still overshoot.
            *sample = (*sample * self.gain).clamp(-1.0, 1.0);
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }
}
