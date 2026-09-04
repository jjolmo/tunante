//! Every decoder and metadata reader Tunante supports.
//!
//! This crate is where all the vendored C/C++ lives — GME, VBA-M, mGBA, DeSmuME,
//! Highly Experimental, lazyusf2 and vgmstream — plus the pure-Rust Opus decoder
//! and the symphonia-backed standard formats.
//!
//! It is deliberately separate from [`tunante_core`]: `tunante` links this
//! crate only into its small `tunante-decoder` helper process, so the tens of
//! megabytes of console RAM these cores allocate never land in the UI process.
//!
//! The module layout mirrors the old `audio` module of the desktop app, so the
//! decoders stay reachable as `tunante_codec::GmeSource` and friends.

mod gme;
mod gsf;
mod opus;
mod psf;
mod psf2;
mod twosf;
mod usf;
mod vgmstream;

pub mod metadata;
mod open;

pub use open::{open_source, open_source_with, BoxedSource, OpenError, PlaybackOptions};

// Re-exported from the core so the decoders and the smoke test keep referring to
// them as `crate::vgm_path` / `crate::dsp`.
pub use tunante_core::{dsp, vgm_path};

#[cfg(test)]
mod format_smoke;

/// Whether vgmstream's own extension list accepts this filename.
///
/// This list is built inside vgmstream and is far broader than the static
/// `AUDIO_EXTENSIONS` table the library scanner walks with, so it is what decides
/// whether an otherwise unrecognised file is still worth opening. Exposed here so
/// callers do not need to depend on `vgmstream-rs` directly.
pub fn vgmstream_accepts(filename: &str) -> bool {
    vgmstream_rs::Vgmstream::is_valid(filename)
}

pub use gme::GmeSource;
pub use gsf::GsfSource;
pub use opus::OggOpusSource;
pub use psf::PsfSource;
pub use psf2::Psf2Source;
pub use twosf::TwoSfSource;
pub use usf::UsfSource;
pub use vgmstream::VgmstreamSource;

/// How many times a looping vgmstream stream repeats by default.
///
/// Re-exported because vgmstream now lives behind this crate: the desktop app
/// no longer links `vgmstream_rs` itself and has no other way to name it.
pub const DEFAULT_VGM_LOOP_COUNT: f64 = vgmstream_rs::Vgmstream::DEFAULT_LOOP_COUNT;
