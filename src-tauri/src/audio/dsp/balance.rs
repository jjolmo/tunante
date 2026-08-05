//! Left/right balance.

use super::{DspProcessor, DspSettings};

/// Pans the signal between the first two channels.
///
/// Attenuation only: at hard left the left channel keeps unity gain and the right
/// is silenced, rather than boosting the left, so balance can never introduce
/// clipping. Channels beyond the first two (vgmstream can hand us 5.1) are left
/// alone — there is no meaningful "balance" for a centre or LFE channel.
pub struct Balance {
    settings: DspSettings,
}

impl Balance {
    pub fn new(settings: DspSettings) -> Self {
        Self { settings }
    }
}

impl DspProcessor for Balance {
    fn id(&self) -> &'static str {
        "balance"
    }

    fn is_active(&self) -> bool {
        self.settings.balance.get() != 0.0
    }

    fn process(&mut self, frame: &mut [f32], _sample_rate: u32) {
        if frame.len() < 2 {
            return;
        }
        let balance = self.settings.balance.get().clamp(-1.0, 1.0);
        frame[0] *= (1.0 - balance).min(1.0);
        frame[1] *= (1.0 + balance).min(1.0);
    }
}
