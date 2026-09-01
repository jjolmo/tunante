//! The app's actual behaviour, with Tauri subtracted.
//!
//! Fase 1 of docs/plan-desktop-slint.md, punto 3: every `#[tauri::command]`
//! in `commands/` is a one-line shell over a free function here. The
//! functions take `&AppState` and, where they have something to announce, an
//! [`crate::events::Events`] — never a `State<…>` extractor, an `AppHandle`,
//! or anything else from Tauri. That rule is the whole point of the module:
//! the Slint app will call these same functions with an `Events` built on its
//! own channels, and the logic the desktop has been exercising for months
//! comes along unrewritten.
//!
//! Threading stays the caller's business where it was the caller's business
//! before: a service that used to spawn keeps spawning, one that blocked
//! keeps blocking, and lock order inside each body is preserved verbatim
//! from the command it came out of.

pub mod classification;
pub mod covers;
pub mod settings;
pub mod library;
pub mod player;
pub mod playlists;
