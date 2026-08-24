//! Preamp.

use super::{DspProcessor, DspSettings};

/// Flat gain in decibels.
///
/// Chiptune rips vary wildly in level — an SPC can sit 12 dB under a vgmstream
/// track from the same game — so a manual preamp is the cheapest way to even them
/// out. Pair it with the limiter if you push it into positive territory.
///
/// The linear gain is derived from dB with a `powf`, which is far too expensive to
/// redo on every frame, so it's cached and only recomputed when the parameter's
/// raw bits change.
pub struct Preamp {
    settings: DspSettings,
    cached_bits: u32,
    gain: f32,
}

impl Preamp {
    pub fn new(settings: DspSettings) -> Self {
        let db = settings.preamp_db.get();
        Self {
            cached_bits: settings.preamp_db.bits(),
            gain: db_to_linear(db),
            settings,
        }
    }
}

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl DspProcessor for Preamp {
    fn id(&self) -> &'static str {
        "preamp"
    }

    fn is_active(&self) -> bool {
        self.settings.preamp_enabled.get() && self.settings.preamp_db.get() != 0.0
    }

    fn process(&mut self, frame: &mut [f32], _sample_rate: u32) {
        let bits = self.settings.preamp_db.bits();
        if bits != self.cached_bits {
            self.cached_bits = bits;
            self.gain = db_to_linear(self.settings.preamp_db.get().clamp(-20.0, 20.0));
        }
        for sample in frame.iter_mut() {
            *sample *= self.gain;
        }
    }
}
