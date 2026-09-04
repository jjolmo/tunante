//! UI-agnostic core of Tunante.
//!
//! Everything here is free of Tauri, of any GUI toolkit, and of the vendored
//! C/C++ emulator cores. It is what the desktop app and `tunante` share:
//! the library database, the play queue, the `path#subsong` virtual-path scheme,
//! and the DSP chain.
//!
//! The emulator-backed decoders and metadata readers live in `tunante-codec`,
//! which depends on this crate for the [`db::models::Track`] type and for
//! [`vgm_path`].

pub mod classify;
pub mod clock;
pub mod console;
pub mod db;
pub mod games;
#[cfg(feature = "dsp")]
pub mod dsp;
pub mod queue;
pub mod session;
pub mod tree;
pub mod vgm_path;

pub use db::models;
pub use clock::PlayClock;
pub use queue::{PlayQueue, RepeatMode};
pub use session::{Session, SleepTimer};

#[cfg(feature = "dsp")]
pub use dsp::DspSettings;
