//! The desktop's ear on the library folders.
//!
//! The machinery — backend choice per platform, the polling fallback, the
//! debounce — lives in `tunante_helper::watch` since fase 1 of
//! docs/plan-desktop-slint.md. What stays here is what is this app's own
//! business: which files count as audio (the codec's dynamic vgmstream list,
//! on top of the shared static one), how a changed file is re-read (in
//! process, with the same scan options the scanner uses), and how the
//! frontend hears about it (a Tauri event).

use crate::services::library::is_audio_file;
use crate::metadata;
use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tunante_helper::watch::FileChange;

pub use tunante_helper::watch::FolderWatcher;

#[derive(Clone, serde::Serialize)]
pub struct WatcherEvent {
    pub event_type: String,
    pub path: String,
}

/// Build the watcher with this app's filter and handler plugged in.
pub fn spawn(state: Arc<AppState>, app: AppHandle) -> FolderWatcher {
    FolderWatcher::new(
        |path| is_audio_file(path),
        move |change, path| {
            let path_str = path.to_string_lossy().to_string();

            match change {
                FileChange::Modified => {
                    // Same endless-track limit the scanner uses, so a file picked up
                    // by the watcher doesn't get a different length than one scanned.
                    let opts = crate::services::library::scan_opts(&state);
                    match metadata::read_metadata_all_with_opts(path, opts) {
                        Ok(tracks) => {
                            let db = state.db.lock();
                            // Remove old entries for this file (handles multi-track cleanup)
                            let _ = db.remove_tracks_by_base_path(&path_str);
                            for track in tracks {
                                if let Err(e) = db.insert_track(&track) {
                                    log::error!("Failed to insert track {}: {}", track.path, e);
                                }
                            }
                            drop(db);

                            let _ = app.emit(
                                "library-updated",
                                WatcherEvent {
                                    event_type: "modified".to_string(),
                                    path: path_str,
                                },
                            );
                        }
                        Err(e) => {
                            log::warn!("Failed to read metadata for {}: {}", path_str, e);
                        }
                    }
                }
                FileChange::Removed => {
                    let db = state.db.lock();
                    // Remove all tracks for this base path (handles #N suffixes)
                    if let Err(e) = db.remove_tracks_by_base_path(&path_str) {
                        log::error!("Failed to remove tracks {}: {}", path_str, e);
                    }
                    drop(db);

                    let _ = app.emit(
                        "library-updated",
                        WatcherEvent {
                            event_type: "deleted".to_string(),
                            path: path_str,
                        },
                    );
                }
            }
        },
    )
}
