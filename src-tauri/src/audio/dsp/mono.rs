//! Mono downmix.

use super::{DspProcessor, DspSettings};

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
pub struct Mono {
    settings: DspSettings,
}

impl Mono {
    pub fn new(settings: DspSettings) -> Self {
        Self { settings }
    }
}

impl DspProcessor for Mono {
    fn id(&self) -> &'static str {
        "mono"
    }

    fn is_active(&self) -> bool {
        self.settings.mono.get()
    }

    fn process(&mut self, frame: &mut [f32], _sample_rate: u32) {
        if frame.len() < 2 {
            return;
        }
        let sum: f32 = frame.iter().sum();
        let avg = sum / frame.len() as f32;
        frame.fill(avg);
    }
}
