//! Every event the backend can tell the UI about, as one typed enum.
//!
//! Until fase 1 of docs/plan-desktop-slint.md these were thirteen string
//! names scattered over the `app.emit(...)` calls, and the payload of each
//! was whatever the call site happened to serialize. The strings still exist
//! — the Svelte frontend listens by name — but in exactly one place now:
//! [`tauri_events`], the adapter that turns a typed event back into the wire
//! format the frontend already speaks. Nothing about the wire changed.
//!
//! The point of the detour is who *else* can now emit: an extracted service
//! function takes an [`Events`] and knows nothing about Tauri, so the same
//! body will serve the Slint app by handing it an `Events` built on Slint
//! channels instead of on `AppHandle`.

use crate::db::models::Track;
use crate::watcher::WatcherEvent;
use std::sync::Arc;
use tauri::Emitter;
use tunante_art::resolver::{BulkProgress, Plan};

#[derive(Clone, Debug, serde::Serialize)]
pub struct ScanProgress {
    pub scanned: usize,
    pub total: usize,
    pub current_path: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PlaybackErrorPayload {
    pub message: String,
    pub path: String,
}

pub enum AppEvent {
    PlayerStateUpdate {
        is_playing: bool,
        position_ms: u64,
        duration_ms: u64,
        volume: f32,
    },
    TrackChanged(Track),
    PlaybackStopped,
    PlaybackError(PlaybackErrorPayload),
    VolumeScrolled(f32),
    AudioOutputChanged(String),
    ShortcutAction(String),
    /// `None` is the broad "something changed, reload" nudge; `Some` carries
    /// which file, from the folder watcher. The frontend accepts both shapes
    /// on the same name, and that asymmetry is preserved on the wire.
    LibraryUpdated(Option<WatcherEvent>),
    ScanProgress(ScanProgress),
    ScanComplete,
    PlaylistCreated {
        id: String,
        track_count: usize,
    },
    CoverProgress(BulkProgress),
    CoverComplete(Vec<Plan>),
}

/// A sink for [`AppEvent`]s that service code can hold without knowing where
/// they go. Cheap to clone; safe to move into worker threads.
#[derive(Clone)]
pub struct Events(Arc<dyn Fn(AppEvent) + Send + Sync>);

impl Events {
    pub fn new(sink: impl Fn(AppEvent) + Send + Sync + 'static) -> Self {
        Self(Arc::new(sink))
    }

    pub fn emit(&self, event: AppEvent) {
        (self.0)(event);
    }

    /// A sink that drops everything — for callers that want the work done and
    /// nobody told, like tests.
    #[allow(dead_code)]
    pub fn silent() -> Self {
        Self::new(|_| {})
    }
}

/// The Tauri adapter: the one place a typed event becomes a named broadcast.
///
/// Names and payload shapes here are the frontend's contract — change one and
/// a Svelte listener somewhere goes quiet without a compile error on either
/// side. That failure mode is why this match exists instead of thirteen
/// scattered `emit` calls.
pub fn tauri_events(app: tauri::AppHandle) -> Events {
    Events::new(move |event| {
        let _ = match event {
            AppEvent::PlayerStateUpdate {
                is_playing,
                position_ms,
                duration_ms,
                volume,
            } => app.emit(
                "player-state-update",
                serde_json::json!({
                    "is_playing": is_playing,
                    "position_ms": position_ms,
                    "duration_ms": duration_ms,
                    "volume": volume,
                }),
            ),
            AppEvent::TrackChanged(track) => app.emit("track-changed", track),
            AppEvent::PlaybackStopped => app.emit("playback-stopped", ()),
            AppEvent::PlaybackError(payload) => app.emit("playback-error", payload),
            AppEvent::VolumeScrolled(volume) => app.emit("volume-scrolled", volume),
            AppEvent::AudioOutputChanged(name) => app.emit("audio-output-changed", name),
            AppEvent::ShortcutAction(action_id) => app.emit("shortcut-action", action_id),
            AppEvent::LibraryUpdated(None) => app.emit("library-updated", ()),
            AppEvent::LibraryUpdated(Some(ev)) => app.emit("library-updated", ev),
            AppEvent::ScanProgress(progress) => app.emit("scan-progress", progress),
            AppEvent::ScanComplete => app.emit("scan-complete", ()),
            AppEvent::PlaylistCreated { id, track_count } => app.emit(
                "playlist-created",
                serde_json::json!({ "id": id, "track_count": track_count }),
            ),
            AppEvent::CoverProgress(progress) => app.emit("cover-progress", progress),
            AppEvent::CoverComplete(plans) => app.emit("cover-complete", plans),
        };
    })
}
