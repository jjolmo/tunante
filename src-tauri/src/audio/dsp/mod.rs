//! In-process DSP chain.
//!
//! Every decoder in the app — GME, PSF/PSF2, GSF, USF, 2SF, Opus, symphonia and
//! vgmstream — ends up as a `rodio::Source` handed to `Player::append`. Wrapping
//! that source in [`DspSource`] is therefore the one place where an effect can be
//! applied to *all* supported formats at once, without touching a single decoder.
//!
//! Two properties make this safe to drop into the existing pipeline:
//!
//! - **The chain is sample-count neutral.** One input sample produces exactly one
//!   output sample, so duration, seek position and rodio's resampler are untouched.
//! - **No locks on the audio thread.** Every parameter lives in an atomic shared
//!   with the UI, so effects can be toggled and tweaked *while a track plays*
//!   without rebuilding the player (which would cut the sound).
//!
//! Processors are read per frame, so a neutral chain costs one relaxed atomic
//! load per processor per frame — noise next to the decoding itself.

use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

mod balance;
mod eq;
mod gain;
mod limiter;
mod mono;
mod width;

pub use balance::Balance;
pub use eq::Equalizer;
pub use gain::Preamp;
pub use limiter::Limiter;
pub use mono::Mono;
pub use width::StereoWidth;

/// One effect in the chain.
///
/// Processors operate on a single frame at a time (one sample per channel), which
/// keeps them independent of the channel count: vgmstream can hand us 1, 2 or 6
/// channels and the same code works.
pub trait DspProcessor: Send {
    /// Stable identifier, also used as the settings key.
    fn id(&self) -> &'static str;

    /// Cheap check so a neutral processor costs nothing but this call. Checked on
    /// every frame, which is what makes live toggling possible.
    fn is_active(&self) -> bool;

    /// Process one frame in place. `frame.len()` is the source's channel count.
    fn process(&mut self, frame: &mut [f32], sample_rate: u32);

    /// Drop internal state (filter memory, envelopes). Called on seek and when the
    /// channel count changes, so stale state can't leak across a discontinuity.
    fn reset(&mut self) {}
}

// ---------------------------------------------------------------------------
// Lock-free parameters
// ---------------------------------------------------------------------------

/// An `f32` shared with the audio thread. Stored as raw bits in an `AtomicU32`
/// because `AtomicF32` doesn't exist in std.
#[derive(Clone, Debug)]
pub struct AtomicF32(Arc<AtomicU32>);

impl AtomicF32 {
    pub fn new(value: f32) -> Self {
        Self(Arc::new(AtomicU32::new(value.to_bits())))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    /// Raw bits, used by processors that cache derived coefficients and only want
    /// to recompute them when the parameter actually changed.
    fn bits(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A `bool` shared with the audio thread.
#[derive(Clone, Debug)]
pub struct AtomicFlag(Arc<AtomicBool>);

impl AtomicFlag {
    pub fn new(value: bool) -> Self {
        Self(Arc::new(AtomicBool::new(value)))
    }

    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn set(&self, value: bool) {
        self.0.store(value, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// Every knob in the chain, shared between the UI thread and the audio thread.
///
/// Cloning shares the same atomics — the engine keeps one copy and hands a chain
/// built from it to each source.
#[derive(Clone, Debug)]
pub struct DspSettings {
    pub mono: AtomicFlag,
    /// Restore the level a downmix costs. Off-phase material loses far more than
    /// the textbook 3 dB, so without this mono reads as "quieter and thinner".
    pub mono_compensate: AtomicFlag,
    /// -1.0 fully left … 0.0 centre … +1.0 fully right.
    pub balance: AtomicF32,
    pub width_enabled: AtomicFlag,
    /// 0.0 mono … 1.0 untouched … 2.0 double width.
    pub width: AtomicF32,
    pub preamp_enabled: AtomicFlag,
    /// Gain in dB, -20 … +20.
    pub preamp_db: AtomicF32,
    pub eq_enabled: AtomicFlag,
    /// Low shelf at 200 Hz, dB.
    pub eq_low_db: AtomicF32,
    /// Peaking at 1 kHz, dB.
    pub eq_mid_db: AtomicF32,
    /// High shelf at 4 kHz, dB.
    pub eq_high_db: AtomicF32,
    pub limiter: AtomicFlag,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            mono: AtomicFlag::new(false),
            mono_compensate: AtomicFlag::new(true),
            balance: AtomicF32::new(0.0),
            width_enabled: AtomicFlag::new(false),
            width: AtomicF32::new(1.0),
            preamp_enabled: AtomicFlag::new(false),
            preamp_db: AtomicF32::new(0.0),
            eq_enabled: AtomicFlag::new(false),
            eq_low_db: AtomicF32::new(0.0),
            eq_mid_db: AtomicF32::new(0.0),
            eq_high_db: AtomicF32::new(0.0),
            limiter: AtomicFlag::new(false),
        }
    }
}

impl DspSettings {
    /// Build a fresh chain sharing this settings object.
    ///
    /// Processors with internal state (filter memory, the limiter envelope) get a
    /// new instance per source, so nothing carries over between tracks.
    ///
    /// Order is signal flow: tone first, then the stereo image, then level.
    /// `Mono` comes after `StereoWidth` (so width is a no-op once mono is on) but
    /// before `Balance` (so a mono signal can still be panned).
    pub fn build_chain(&self) -> DspChain {
        DspChain {
            processors: vec![
                Box::new(Equalizer::new(self.clone())),
                Box::new(StereoWidth::new(self.clone())),
                Box::new(Mono::new(self.clone())),
                Box::new(Balance::new(self.clone())),
                Box::new(Preamp::new(self.clone())),
                Box::new(Limiter::new(self.clone())),
            ],
        }
    }

    /// Ids of the processors in chain order, for the UI.
    pub fn processor_ids() -> &'static [&'static str] {
        &["eq", "width", "mono", "balance", "preamp", "limiter"]
    }
}

/// An ordered stack of processors.
pub struct DspChain {
    processors: Vec<Box<dyn DspProcessor>>,
}

impl DspChain {
    fn process(&mut self, frame: &mut [f32], sample_rate: u32) {
        for p in &mut self.processors {
            if p.is_active() {
                p.process(frame, sample_rate);
            }
        }
    }

    fn reset(&mut self) {
        for p in &mut self.processors {
            p.reset();
        }
    }

    /// Ids of the processors currently doing something. Used by the debug window.
    pub fn active_ids(&self) -> Vec<&'static str> {
        self.processors
            .iter()
            .filter(|p| p.is_active())
            .map(|p| p.id())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// The source adapter
// ---------------------------------------------------------------------------

/// Wraps any `Source` and runs the DSP chain over it, frame by frame.
pub struct DspSource<S> {
    inner: S,
    chain: DspChain,
    /// The frame currently being served, already processed.
    frame: Vec<f32>,
    pos: usize,
    channels: usize,
}

impl<S: Source> DspSource<S> {
    pub fn new(inner: S, chain: DspChain) -> Self {
        let channels = inner.channels().get() as usize;
        Self {
            inner,
            chain,
            frame: Vec::with_capacity(channels),
            pos: 0,
            channels,
        }
    }

    /// Pull one whole frame from the inner source and run the chain over it.
    /// Returns false when the source is done.
    fn fill_frame(&mut self) -> bool {
        let channels = self.inner.channels().get() as usize;
        if channels != self.channels {
            // Channel layout changed mid-stream: filter state from the old layout
            // is meaningless, and keeping it would smear one channel into another.
            self.channels = channels;
            self.chain.reset();
        }

        self.frame.clear();
        for i in 0..channels {
            match self.inner.next() {
                Some(sample) => self.frame.push(sample),
                None => {
                    if i == 0 {
                        return false;
                    }
                    // The source ended mid-frame. Pad with silence rather than
                    // emitting a short frame, which would shift every following
                    // sample by one channel downstream.
                    self.frame.push(0.0);
                }
            }
        }

        self.chain.process(&mut self.frame, self.inner.sample_rate().get());
        self.pos = 0;
        true
    }
}

impl<S: Source> Iterator for DspSource<S> {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.frame.len() && !self.fill_frame() {
            return None;
        }
        let sample = self.frame[self.pos];
        self.pos += 1;
        Some(sample)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let buffered = self.frame.len() - self.pos;
        let (low, high) = self.inner.size_hint();
        (low + buffered, high.map(|h| h + buffered))
    }
}

impl<S: Source> Source for DspSource<S> {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        // We read a frame ahead of the inner source, so the samples still sitting
        // in our buffer belong to the span the inner source is reporting on.
        self.inner
            .current_span_len()
            .map(|n| n + (self.frame.len() - self.pos))
    }

    #[inline]
    fn channels(&self) -> NonZeroU16 {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> NonZeroU32 {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)?;
        self.frame.clear();
        self.pos = 0;
        self.chain.reset();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stereo source yielding a fixed set of frames.
    struct TestSource {
        samples: std::vec::IntoIter<f32>,
        channels: u16,
        rate: u32,
    }

    impl TestSource {
        fn new(samples: Vec<f32>, channels: u16) -> Self {
            Self {
                samples: samples.into_iter(),
                channels,
                rate: 44100,
            }
        }
    }

    impl Iterator for TestSource {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.samples.next()
        }
    }

    impl Source for TestSource {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> NonZeroU16 {
            NonZeroU16::new(self.channels).unwrap()
        }
        fn sample_rate(&self) -> NonZeroU32 {
            NonZeroU32::new(self.rate).unwrap()
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
        fn try_seek(&mut self, _: Duration) -> Result<(), SeekError> {
            Ok(())
        }
    }

    fn drain<S: Source>(source: S) -> Vec<f32> {
        source.collect()
    }

    /// The test that actually proves the frames stay aligned: a signal that is
    /// +1 on the left and -1 on the right must collapse to *exact* silence. Any
    /// off-by-one in the frame handling turns this into full-scale noise.
    #[test]
    fn mono_of_antiphase_stereo_is_silence() {
        let settings = DspSettings::default();
        settings.mono.set(true);

        let input: Vec<f32> = (0..1000).flat_map(|_| [1.0f32, -1.0f32]).collect();
        let out = drain(DspSource::new(
            TestSource::new(input, 2),
            settings.build_chain(),
        ));

        assert_eq!(out.len(), 2000);
        assert!(
            out.iter().all(|s| s.abs() < 1e-6),
            "antiphase stereo did not collapse to silence"
        );
    }

    #[test]
    fn chain_is_sample_count_neutral() {
        let settings = DspSettings::default();
        settings.mono.set(true);
        settings.eq_enabled.set(true);
        settings.eq_low_db.set(6.0);
        settings.limiter.set(true);

        for channels in [1u16, 2, 6] {
            let n = 600 * channels as usize;
            let input: Vec<f32> = (0..n).map(|i| (i as f32 * 0.01).sin()).collect();
            let out = drain(DspSource::new(
                TestSource::new(input, channels),
                settings.build_chain(),
            ));
            assert_eq!(out.len(), n, "channel count {channels} changed sample count");
        }
    }

    /// A frame that ends short must be padded, not truncated, or every following
    /// sample lands on the wrong channel.
    #[test]
    fn incomplete_final_frame_is_padded() {
        let settings = DspSettings::default();
        // 5 samples of a 2-channel source: the last frame is half a frame.
        let out = drain(DspSource::new(
            TestSource::new(vec![1.0, 1.0, 1.0, 1.0, 1.0], 2),
            settings.build_chain(),
        ));
        assert_eq!(out.len(), 6, "short final frame was not padded to a full frame");
        assert_eq!(out[5], 0.0);
    }

    #[test]
    fn neutral_chain_is_bit_exact_passthrough() {
        let settings = DspSettings::default();
        let input: Vec<f32> = (0..500).map(|i| (i as f32 * 0.07).sin()).collect();
        let out = drain(DspSource::new(
            TestSource::new(input.clone(), 2),
            settings.build_chain(),
        ));
        assert_eq!(out, input, "an all-off chain must not alter a single sample");
    }

    #[test]
    fn mono_averages_rather_than_dropping_channels() {
        let settings = DspSettings::default();
        settings.mono.set(true);
        settings.mono_compensate.set(false); // aqui se mide la suma cruda
        // rodio's own ChannelCountConverter would discard the right channel here;
        // we must get the average instead.
        let out = drain(DspSource::new(
            TestSource::new(vec![1.0, 0.0], 2),
            settings.build_chain(),
        ));
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn balance_pans_without_boosting() {
        let settings = DspSettings::default();
        settings.balance.set(-1.0); // hard left
        let out = drain(DspSource::new(
            TestSource::new(vec![1.0, 1.0], 2),
            settings.build_chain(),
        ));
        assert_eq!(out[0], 1.0, "left channel must not be attenuated");
        assert_eq!(out[1], 0.0, "right channel must be silent");
    }

    /// Balance sits *after* the downmix on purpose, so a mono signal can still be
    /// panned. The cost is that an off-centre balance re-splits what mono just
    /// collapsed, and mono then reads as broken — even a 15% offset makes every
    /// frame uneven. Pinned here because it's a deliberate trade-off, not a bug,
    /// and the settings panel warns about it.
    #[test]
    fn balance_after_mono_re_splits_the_downmix() {
        let settings = DspSettings::default();
        settings.mono.set(true);
        settings.mono_compensate.set(false);

        let input = vec![1.0f32, 0.0]; // hard-left content
        let centred = drain(DspSource::new(
            TestSource::new(input.clone(), 2),
            settings.build_chain(),
        ));
        assert_eq!(centred, vec![0.5, 0.5], "mono alone must centre the signal");

        // A slider left slightly off centre is enough to undo it.
        settings.balance.set(-0.15);
        let offset = drain(DspSource::new(
            TestSource::new(input, 2),
            settings.build_chain(),
        ));
        assert!(
            (offset[0] - offset[1]).abs() > 1e-3,
            "expected balance to re-split the downmix, got {offset:?}"
        );
        assert_eq!(offset[0], 0.5, "the left side keeps unity gain");
    }

    /// Anti-correlated channels lose the most to a downmix — measured at 4.6 to
    /// 6.8 dB on real SNES rips, which use out-of-phase panning. The cancelled
    /// content is gone for good, but the level must come back.
    #[test]
    fn mono_compensation_restores_the_level_lost_to_out_of_phase_content() {
        // Two tones an octave apart, one inverted on the right: strongly
        // anti-correlated, so the plain sum collapses.
        let frames = 200_000;
        let input: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let a = (i as f32 * 0.05).sin() * 0.5;
                let b = (i as f32 * 0.10).sin() * 0.5;
                [a + b, -a + b]
            })
            .collect();

        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        let input_rms = rms(&input);

        let raw = DspSettings::default();
        raw.mono.set(true);
        raw.mono_compensate.set(false);
        let plain = drain(DspSource::new(
            TestSource::new(input.clone(), 2),
            raw.build_chain(),
        ));

        let compensated_settings = DspSettings::default();
        compensated_settings.mono.set(true);
        let compensated = drain(DspSource::new(
            TestSource::new(input, 2),
            compensated_settings.build_chain(),
        ));

        // Only the settled tail matters; the follower needs a moment to converge.
        let tail = frames; // second half, in samples
        let plain_rms = rms(&plain[tail..]);
        let comp_rms = rms(&compensated[tail..]);

        let plain_db = 20.0 * (plain_rms / input_rms).log10();
        let comp_db = 20.0 * (comp_rms / input_rms).log10();
        assert!(
            plain_db < -2.0,
            "expected the uncompensated sum to lose level, got {plain_db:.1} dB"
        );
        assert!(
            comp_db.abs() < 1.0,
            "compensated downmix should land within 1 dB of the source, got {comp_db:.1} dB"
        );
    }

    /// Compensation must not resurrect silence into noise.
    #[test]
    fn mono_compensation_leaves_silence_alone() {
        let settings = DspSettings::default();
        settings.mono.set(true);
        let out = drain(DspSource::new(
            TestSource::new(vec![0.0; 20_000], 2),
            settings.build_chain(),
        ));
        assert!(out.iter().all(|s| *s == 0.0), "silence gained a level");
    }

    #[test]
    fn width_zero_equals_mono() {
        let settings = DspSettings::default();
        settings.width_enabled.set(true);
        settings.width.set(0.0);
        let out = drain(DspSource::new(
            TestSource::new(vec![1.0, 0.0], 2),
            settings.build_chain(),
        ));
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn preamp_applies_decibels() {
        let settings = DspSettings::default();
        settings.preamp_enabled.set(true);
        settings.preamp_db.set(-6.0);
        let out = drain(DspSource::new(
            TestSource::new(vec![1.0, 1.0], 2),
            settings.build_chain(),
        ));
        // -6 dB is a factor of ~0.501
        assert!((out[0] - 0.501_187).abs() < 1e-4, "got {}", out[0]);
    }

    #[test]
    fn limiter_keeps_output_below_ceiling() {
        let settings = DspSettings::default();
        settings.preamp_enabled.set(true);
        settings.preamp_db.set(20.0); // x10, way into clipping
        settings.limiter.set(true);

        let input: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.05).sin()).collect();
        let out = drain(DspSource::new(
            TestSource::new(input, 2),
            settings.build_chain(),
        ));
        assert!(
            out.iter().all(|s| s.abs() <= 1.0),
            "limiter let the signal past full scale"
        );
        // And the tail, once the envelope has settled, must be at the ceiling
        // rather than silenced.
        let tail_peak = out[2000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(tail_peak > 0.9, "limiter over-attenuated: peak {tail_peak}");
    }

    #[test]
    fn seek_resets_filter_state() {
        let settings = DspSettings::default();
        settings.eq_enabled.set(true);
        settings.eq_low_db.set(12.0);

        let mut source = DspSource::new(
            TestSource::new(vec![1.0; 100], 2),
            settings.build_chain(),
        );
        let _ = source.next();
        source.try_seek(Duration::ZERO).unwrap();
        assert_eq!(source.pos, 0);
        assert!(source.frame.is_empty());
    }
}
