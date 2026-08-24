//! Mono downmix.

use super::{DspProcessor, DspSettings};

/// How fast the level compensation follows the material. Long enough that it
/// settles to a near-constant gain for a given piece instead of pumping like a
/// compressor.
const FOLLOW_SECS: f32 = 0.3;
/// Never boost more than +12 dB, so a near-silent passage can't be blown up.
const MAX_GAIN: f32 = 4.0;
/// Correlation is a statistic, not a signal: measure it over a long window so it
/// reflects the arrangement rather than individual notes.
const CORRELATION_SECS: f32 = 1.0;
/// How fast the polarity crossfades when the decision flips. Slow enough to be a
/// fade rather than a click.
const POLARITY_SECS: f32 = 0.2;
/// Dead zone around zero correlation, so material that is neither in nor out of
/// phase doesn't make the polarity flap back and forth.
const CORRELATION_HYSTERESIS: f32 = 0.1;

/// Collapses every channel into their average.
///
/// The average — not the sum — so the result can never clip, and not rodio's own
/// channel conversion, which *discards* the extra channels (see
/// `ChannelCountConverter` in `rodio::conversions::channels`) and would leave you
/// with the left channel alone rather than a mix.
///
/// The channel count is deliberately left untouched: a stereo track stays stereo
/// with both channels carrying the same signal. Reporting 1 channel instead would
/// just make rodio duplicate it again downstream, and would change the frame size
/// mid-chain for no gain.
///
/// # Level compensation
///
/// Summing to mono costs level, and how much depends entirely on how correlated
/// the channels are: identical channels lose nothing, independent ones lose 3 dB,
/// and anti-correlated ones cancel and lose more. SNES rips are a bad case — the
/// SPC700's volume registers are *signed*, so games pan voices out of phase for a
/// wide effect, and measured Seiken Densetsu 3 tracks sit at a correlation of
/// -0.31 to -0.60 and lose 4.6 to 6.8 dB.
///
/// The cancelled content itself cannot be recovered by a level change — but it can
/// be kept from cancelling in the first place, which is what the phase-safe mode
/// below does.
///
/// # Phase-safe downmix
///
/// A plain `(L + R) / 2` destroys anything that lives in the *difference* between
/// the channels: a drum panned as `+d` left and `-d` right sums to exactly zero
/// and vanishes. That is not hypothetical — it is what made this feature
/// necessary, on a single earbud that sums both channels itself.
///
/// When enabled, this tracks the running correlation between the channels and
/// inverts the right one before summing while they are anti-phase, so those parts
/// reinforce instead of cancelling. The polarity is crossfaded rather than
/// switched, and a dead zone around zero keeps it from flapping.
///
/// This is a deliberate alteration of the signal, not a neutral downmix, which is
/// why it is opt-in. It applies to stereo only: for more channels there is no
/// single "other channel" to align against, so the plain average is used.
pub struct Mono {
    settings: DspSettings,
    /// Smoothed mean energy per channel going in.
    in_energy: f32,
    /// Smoothed energy of the downmix coming out.
    out_energy: f32,
    gain: f32,
    coefficient: f32,
    /// Smoothed products for the running correlation.
    e_lr: f32,
    e_ll: f32,
    e_rr: f32,
    correlation_coefficient: f32,
    /// Current polarity applied to the right channel, in -1.0 ..= 1.0.
    polarity: f32,
    /// Where the polarity is heading. Only moves outside the dead zone.
    polarity_target: f32,
    polarity_coefficient: f32,
    cached_rate: u32,
}

impl Mono {
    pub fn new(settings: DspSettings) -> Self {
        Self {
            settings,
            in_energy: 0.0,
            out_energy: 0.0,
            gain: 1.0,
            coefficient: 0.0,
            e_lr: 0.0,
            e_ll: 0.0,
            e_rr: 0.0,
            correlation_coefficient: 0.0,
            polarity: 1.0,
            polarity_target: 1.0,
            polarity_coefficient: 0.0,
            cached_rate: 0,
        }
    }

    fn retune(&mut self, sample_rate: u32) {
        let rate = sample_rate as f32;
        self.coefficient = 1.0 - (-1.0 / (rate * FOLLOW_SECS)).exp();
        self.correlation_coefficient = 1.0 - (-1.0 / (rate * CORRELATION_SECS)).exp();
        self.polarity_coefficient = 1.0 - (-1.0 / (rate * POLARITY_SECS)).exp();
        self.cached_rate = sample_rate;
    }

    /// Update the running correlation and move the polarity towards it.
    /// Returns the polarity to apply to the right channel.
    fn track_polarity(&mut self, left: f32, right: f32) -> f32 {
        let c = self.correlation_coefficient;
        self.e_lr += (left * right - self.e_lr) * c;
        self.e_ll += (left * left - self.e_ll) * c;
        self.e_rr += (right * right - self.e_rr) * c;

        let denominator = (self.e_ll * self.e_rr).sqrt();
        if denominator > 1e-12 {
            let correlation = self.e_lr / denominator;
            // Outside the dead zone the decision is unambiguous; inside it, hold
            // whatever we were doing rather than dithering between the two.
            if correlation < -CORRELATION_HYSTERESIS {
                self.polarity_target = -1.0;
            } else if correlation > CORRELATION_HYSTERESIS {
                self.polarity_target = 1.0;
            }
        }

        self.polarity += (self.polarity_target - self.polarity) * self.polarity_coefficient;
        self.polarity
    }
}

impl DspProcessor for Mono {
    fn id(&self) -> &'static str {
        "mono"
    }

    fn is_active(&self) -> bool {
        self.settings.mono.get()
    }

    fn process(&mut self, frame: &mut [f32], sample_rate: u32) {
        if frame.len() < 2 {
            return;
        }
        if sample_rate != self.cached_rate {
            self.retune(sample_rate);
        }

        let mut avg = if frame.len() == 2 && self.settings.mono_phase_safe.get() {
            let polarity = self.track_polarity(frame[0], frame[1]);
            (frame[0] + polarity * frame[1]) * 0.5
        } else {
            frame.iter().sum::<f32>() / frame.len() as f32
        };

        if self.settings.mono_compensate.get() {
            // Measured before the frame is overwritten.
            let in_energy = frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32;
            self.in_energy += (in_energy - self.in_energy) * self.coefficient;
            self.out_energy += (avg * avg - self.out_energy) * self.coefficient;

            // Below this the track is effectively silent and the ratio is noise.
            let target = if self.out_energy > 1e-10 {
                (self.in_energy / self.out_energy).sqrt().clamp(1.0, MAX_GAIN)
            } else {
                1.0
            };
            self.gain += (target - self.gain) * self.coefficient;
            avg *= self.gain;
        }

        frame.fill(avg);
    }

    fn reset(&mut self) {
        self.in_energy = 0.0;
        self.out_energy = 0.0;
        self.gain = 1.0;
        self.e_lr = 0.0;
        self.e_ll = 0.0;
        self.e_rr = 0.0;
        self.polarity = 1.0;
        self.polarity_target = 1.0;
    }
}
