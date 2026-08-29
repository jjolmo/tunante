//! Cover art: finding it on disk, matching a game against the Libretro archive,
//! fetching it, and writing it somewhere it will still be tomorrow.
//!
//! # Why this is its own crate
//!
//! All of it used to live in the Tauri crate, which meant `tunante-mini` and the
//! Android app could not fetch a cover at all — `reqwest` appears in no other
//! crate in this workspace. This one is shared by all three.
//!
//! It is deliberately **not** a module of `tunante-helper`. That crate's job is
//! spawning and talking to the decoder process; the desktop app does not depend
//! on it and should not have to, and putting a TLS stack inside the crate whose
//! business is `execve` and pipes would make every consumer of "spawn the
//! decoder" link one too.
//!
//! It is also deliberately **free of `tunante-core`**. It never opens the
//! database. The caller assembles a [`CoverRequest`] — including the Libretro
//! archive name, which it looks up in `tunante_core::console` — and this crate
//! answers it. That keeps the one console table where it belongs, keeps SQLite
//! out of these tests, and stops a matching algorithm from depending on display
//! strings.
//!
//! # The one irreversible thing here
//!
//! Everything is a cache except [`folder::store_cover`], which writes into the
//! user's own library. A cover written there syncs to their other machines and
//! has to be deleted by hand on each. So that path validates before it writes,
//! never overwrites an image that is already there, renames atomically, and
//! records what it did so a run can be undone.

pub mod archive;
pub mod cache;
pub mod folder;
pub mod http;
pub mod image;
pub mod index;
pub mod name;
pub mod resolver;
pub mod search;
pub mod sources;
pub mod tracklist;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ArtError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http {status} for {url}")]
    Http { status: u16, url: String },
    #[error("network: {0}")]
    Network(String),
    #[error("not an image: {0}")]
    NotAnImage(String),
    #[error("{0}")]
    Rejected(String),
}

/// How much to trust a match.
///
/// The distinction earns its keep because the output can be a permanent write
/// into a synced library: a wrong cover is worse than no cover, since a missing
/// one is visibly missing and a wrong one looks deliberate. Anything below
/// [`Confidence::High`] is offered for review rather than applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// A fuzzy or last-resort match. Never applied automatically.
    Low,
    /// A token-subset match against a single unambiguous game.
    Medium,
    /// The archive had a subtitle we lacked, or vice versa, and only one game
    /// could be meant.
    High,
    /// The normalised names are equal.
    Exact,
}
