//! Stereo width (mid/side).

use super::{DspProcessor, DspSettings};

/// Widens or narrows the stereo image by scaling the side signal.
///
/// `0.0` collapses to mono, `1.0` leaves the source untouched, `2.0` doubles the
/// separation. Useful on chiptunes in particular: NES and SNES rips often come
/// hard-panned per channel, which is exhausting on headphones and narrows well.
///
/// Works on the first two channels only; anything beyond stereo passes through.
pub struct StereoWidth {
    settings: DspSettings,
}

impl StereoWidth {
    pub fn new(settings: DspSettings) -> Self {
        Self { settings }
    }
}

impl DspProcessor for StereoWidth {
    fn id(&self) -> &'static str {
        "width"
    }

    fn is_active(&self) -> bool {
        self.settings.width_enabled.get() && self.settings.width.get() != 1.0
    }

    fn process(&mut self, frame: &mut [f32], _sample_rate: u32) {
        if frame.len() < 2 {
            return;
        }
        let width = self.settings.width.get().clamp(0.0, 2.0);
        let mid = (frame[0] + frame[1]) * 0.5;
        let side = (frame[0] - frame[1]) * 0.5 * width;
        frame[0] = mid + side;
        frame[1] = mid - side;
    }
}
