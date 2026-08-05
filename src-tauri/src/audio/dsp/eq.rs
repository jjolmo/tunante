//! Three-band equalizer.

use super::{DspProcessor, DspSettings};

/// Low shelf corner.
const LOW_HZ: f32 = 200.0;
/// Mid peak centre.
const MID_HZ: f32 = 1000.0;
/// High shelf corner.
const HIGH_HZ: f32 = 4000.0;
/// Q of the mid band. Wide enough to be a tone control rather than a notch.
const MID_Q: f32 = 0.9;

/// A biquad in transposed direct form II.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    fn set_coefficients(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// RBJ audio-EQ-cookbook low shelf, slope S = 1.
    fn low_shelf(&mut self, freq: f32, sample_rate: f32, db: f32) {
        let a = 10f32.powf(db / 40.0);
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / 2.0 * std::f32::consts::SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        self.set_coefficients(
            a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
            a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
            (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
            (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
        );
    }

    /// RBJ audio-EQ-cookbook high shelf, slope S = 1.
    fn high_shelf(&mut self, freq: f32, sample_rate: f32, db: f32) {
        let a = 10f32.powf(db / 40.0);
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / 2.0 * std::f32::consts::SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        self.set_coefficients(
            a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
            a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
            (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
            2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
            (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
        );
    }

    /// RBJ audio-EQ-cookbook peaking filter.
    fn peaking(&mut self, freq: f32, sample_rate: f32, q: f32, db: f32) {
        let a = 10f32.powf(db / 40.0);
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        self.set_coefficients(
            1.0 + alpha * a,
            -2.0 * cos_w0,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos_w0,
            1.0 - alpha / a,
        );
    }
}

/// Bass / mid / treble, applied independently to every channel.
///
/// Coefficients depend on the sample rate as well as the gains, and computing them
/// costs several transcendentals, so they're recomputed only when a gain or the
/// sample rate actually changes — not on every frame.
///
/// Filter memory is per channel and is allocated lazily, which keeps the processor
/// correct for the 1- and 6-channel files vgmstream can produce without paying for
/// them on the stereo path.
pub struct Equalizer {
    settings: DspSettings,
    /// `[channel][band]`
    filters: Vec<[Biquad; 3]>,
    cached_gains: [u32; 3],
    cached_rate: u32,
}

impl Equalizer {
    pub fn new(settings: DspSettings) -> Self {
        Self {
            settings,
            filters: Vec::new(),
            cached_gains: [u32::MAX; 3],
            cached_rate: 0,
        }
    }

    /// Recompute the prototype coefficients and copy them into every channel,
    /// preserving each channel's filter memory so a live tweak doesn't click.
    fn retune(&mut self, sample_rate: u32) {
        let rate = sample_rate as f32;
        let low = self.settings.eq_low_db.get().clamp(-20.0, 20.0);
        let mid = self.settings.eq_mid_db.get().clamp(-20.0, 20.0);
        let high = self.settings.eq_high_db.get().clamp(-20.0, 20.0);

        let mut prototype = [Biquad::default(); 3];
        prototype[0].low_shelf(LOW_HZ, rate, low);
        prototype[1].peaking(MID_HZ, rate, MID_Q, mid);
        prototype[2].high_shelf(HIGH_HZ, rate, high);

        for channel in &mut self.filters {
            for (band, proto) in channel.iter_mut().zip(prototype.iter()) {
                let (z1, z2) = (band.z1, band.z2);
                *band = *proto;
                band.z1 = z1;
                band.z2 = z2;
            }
        }

        self.cached_gains = [
            self.settings.eq_low_db.bits(),
            self.settings.eq_mid_db.bits(),
            self.settings.eq_high_db.bits(),
        ];
        self.cached_rate = sample_rate;
    }
}

impl DspProcessor for Equalizer {
    fn id(&self) -> &'static str {
        "eq"
    }

    fn is_active(&self) -> bool {
        self.settings.eq_enabled.get()
            && (self.settings.eq_low_db.get() != 0.0
                || self.settings.eq_mid_db.get() != 0.0
                || self.settings.eq_high_db.get() != 0.0)
    }

    fn process(&mut self, frame: &mut [f32], sample_rate: u32) {
        if self.filters.len() < frame.len() {
            self.filters.resize(frame.len(), [Biquad::default(); 3]);
            // New channels start with zeroed coefficients, so force a retune.
            self.cached_rate = 0;
        }

        let gains = [
            self.settings.eq_low_db.bits(),
            self.settings.eq_mid_db.bits(),
            self.settings.eq_high_db.bits(),
        ];
        if gains != self.cached_gains || sample_rate != self.cached_rate {
            self.retune(sample_rate);
        }

        for (sample, bands) in frame.iter_mut().zip(self.filters.iter_mut()) {
            let mut x = *sample;
            for band in bands.iter_mut() {
                x = band.process(x);
            }
            *sample = x;
        }
    }

    fn reset(&mut self) {
        for channel in &mut self.filters {
            for band in channel.iter_mut() {
                band.reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measure the steady-state gain of the chain at a given frequency.
    fn gain_at(eq: &mut Equalizer, freq: f32, rate: u32) -> f32 {
        let n = (rate as f32 / freq * 200.0) as usize;
        let mut peak = 0.0f32;
        for i in 0..n {
            let mut frame = [(std::f32::consts::TAU * freq * i as f32 / rate as f32).sin()];
            eq.process(&mut frame, rate);
            // Skip the settling transient.
            if i > n / 2 {
                peak = peak.max(frame[0].abs());
            }
        }
        peak
    }

    #[test]
    fn bands_hit_their_target_gain() {
        let settings = DspSettings::default();
        settings.eq_enabled.set(true);
        settings.eq_low_db.set(6.0);
        let mut eq = Equalizer::new(settings.clone());
        // Well below the 200 Hz corner the low shelf reaches its full +6 dB.
        let g = gain_at(&mut eq, 40.0, 44100);
        assert!((g - 1.995).abs() < 0.1, "low shelf gain {g}, expected ~1.995");

        // And well above it the shelf is out of the way.
        let mut eq = Equalizer::new(settings.clone());
        let g = gain_at(&mut eq, 8000.0, 44100);
        assert!((g - 1.0).abs() < 0.05, "low shelf leaked into 8 kHz: {g}");
    }

    #[test]
    fn mid_band_is_centred_where_advertised() {
        let settings = DspSettings::default();
        settings.eq_enabled.set(true);
        settings.eq_mid_db.set(-12.0);
        let mut eq = Equalizer::new(settings);
        let g = gain_at(&mut eq, MID_HZ, 44100);
        // -12 dB is a factor of ~0.251
        assert!((g - 0.251).abs() < 0.03, "mid band gain {g}");
    }

    #[test]
    fn filters_stay_stable_at_extreme_settings() {
        let settings = DspSettings::default();
        settings.eq_enabled.set(true);
        settings.eq_low_db.set(20.0);
        settings.eq_mid_db.set(20.0);
        settings.eq_high_db.set(20.0);
        let mut eq = Equalizer::new(settings);

        for i in 0..200_000 {
            let mut frame = [(i as f32 * 0.31).sin(), (i as f32 * 0.17).sin()];
            eq.process(&mut frame, 48000);
            assert!(
                frame.iter().all(|s| s.is_finite()),
                "filter blew up at sample {i}"
            );
        }
    }
}
