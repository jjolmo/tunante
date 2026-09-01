// The engine itself lives in `tunante-audio` now (docs/plan-desktop-slint.md,
// fase 1): the same device selection and unplug recovery as always, but the
// decoding happens out of process through tunante-decoder. The queue, the DSP
// chain and the `path#subsong` scheme are UI-agnostic and live in
// `tunante-core`. Re-exported here so the rest of this crate keeps referring
// to them as `audio::…`.
pub use tunante_audio::{list_output_devices, AudioEngine, OutputSelection};
pub use tunante_core::{dsp, queue, vgm_path};

pub use dsp::DspSettings;
pub use queue::{PlayQueue, RepeatMode};
