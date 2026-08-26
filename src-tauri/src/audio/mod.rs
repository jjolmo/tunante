mod engine;

// The queue, the DSP chain and the `path#subsong` scheme are UI-agnostic and now
// live in `tunante-core`; every decoder and metadata reader lives in
// `tunante-codec`. Re-exported here so the rest of this crate keeps referring to
// them as `audio::…`.
pub use tunante_core::{dsp, queue, vgm_path};
pub use tunante_codec::{
    GmeSource, GsfSource, OggOpusSource, Psf2Source, PsfSource, TwoSfSource, UsfSource,
    VgmstreamSource,
};

pub use dsp::DspSettings;
pub use engine::{list_output_devices, AudioEngine, OutputSelection};
pub use queue::{PlayQueue, RepeatMode};
