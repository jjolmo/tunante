//! Mono downmix.

use super::{DspProcessor, DspSettings};

/// How fast the level compensation follows the material. Long enough that it
/// settles to a near-constant gain for a given piece instead of pumping like a
/// compressor.
const FOLLOW_SECS: f32 = 0.3;
/// Never boost more than +12 dB, so a near-silent passage can't be blown up.
const MAX_GAIN: f32 = 4.0;

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
/// The cancelled content itself cannot be recovered — that is what mono means —
/// but the level can. When enabled, this tracks the input and output energy and
/// restores the gap, so the downmix lands at the level the track had before.
pub struct Mono {
    settings: DspSettings,
    /// Smoothed mean energy per channel going in.
    in_energy: f32,
    /// Smoothed energy of the downmix coming out.
    out_energy: f32,
    gain: f32,
    coefficient: f32,
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
            cached_rate: 0,
        }
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

        let mut avg = frame.iter().sum::<f32>() / frame.len() as f32;

        if self.settings.mono_compensate.get() {
            if sample_rate != self.cached_rate {
                self.coefficient = 1.0 - (-1.0 / (sample_rate as f32 * FOLLOW_SECS)).exp();
                self.cached_rate = sample_rate;
            }

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
    }
}
