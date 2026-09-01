//! Playlist CRUD, and the one long job: filling a playlist from a folder.

use crate::services::library::{augment_ratings, is_audio_file};
use crate::db::models::{Playlist, Track};
use crate::events::{AppEvent, Events, ScanProgress};
use crate::metadata;
use crate::AppState;
use std::sync::Arc;
use uuid::Uuid;
use walkdir::WalkDir;

pub fn get_playlists(state: &AppState) -> Result<Vec<Playlist>, String> {
    state.db.lock().get_playlists().map_err(|e| e.to_string())
}

pub fn get_playlist_tracks(state: &Arc<AppState>, playlist_id: &str) -> Result<Vec<Track>, String> {
    let mut tracks = state
        .db
        .lock()
        .get_playlist_tracks(playlist_id)
        .map_err(|e| e.to_string())?;
    augment_ratings(state, &mut tracks);
    Ok(tracks)
}

pub fn create_playlist(state: &AppState, name: &str) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    state
        .db
        .lock()
        .create_playlist(&id, name)
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn delete_playlist(state: &AppState, id: &str) -> Result<(), String> {
    state.db.lock().delete_playlist(id).map_err(|e| e.to_string())
}

pub fn rename_playlist(state: &AppState, id: &str, name: &str) -> Result<(), String> {
    state
        .db
        .lock()
        .rename_playlist(id, name)
        .map_err(|e| e.to_string())
}

pub fn reorder_playlists(state: &AppState, ordered_ids: &[String]) -> Result<(), String> {
    state
        .db
        .lock()
        .reorder_playlists(ordered_ids)
        .map_err(|e| e.to_string())
}

pub fn add_tracks_to_playlist(
    state: &AppState,
    playlist_id: &str,
    track_ids: &[String],
) -> Result<(), String> {
    let db = state.db.lock();
    // One transaction for the batch. Per track it was four committed
    // transactions each, and this command is what a drag of a multi-selection
    // onto a playlist lands in.
    let added = db
        .add_tracks_to_playlist(playlist_id, track_ids)
        .map_err(|e| e.to_string())?;

    // Not the same as the old per-track loop, which aborted the whole call the
    // moment one id failed its foreign key. Skipping is the better answer — the
    // rest of the selection still lands — but it must not be a silent one, or a
    // library that has drifted under the UI looks like a playlist that quietly
    // drops tracks. Duplicates are skipped too, and are the ordinary case.
    let skipped = track_ids.len().saturating_sub(added);
    if skipped > 0 {
        log::info!(
            "add_tracks_to_playlist: {added} added, {skipped} skipped (already present, or no such track)"
        );
    }
    Ok(())
}

pub fn remove_track_from_playlist(
    state: &AppState,
    playlist_id: &str,
    track_id: &str,
) -> Result<(), String> {
    state
        .db
        .lock()
        .remove_track_from_playlist(playlist_id, track_id)
        .map_err(|e| e.to_string())
}

/// Scan a folder for audio files, add them to the library, and populate
/// an existing playlist with the discovered tracks. The playlist should be
/// created by the caller first (so it appears immediately in the sidebar).
/// Runs in a background thread and emits scan-progress / scan-complete /
/// playlist-created events.
pub fn create_playlist_from_folder(
    state: &Arc<AppState>,
    events: Events,
    path: String,
    playlist_id: String,
) -> Result<(), String> {
    let state_inner = state.clone();
    std::thread::spawn(move || {
        // 1. Discover all audio files in the folder
        let audio_files: Vec<std::path::PathBuf> = WalkDir::new(&path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && is_audio_file(e.path()))
            .map(|e| e.into_path())
            .collect();

        let total = audio_files.len();

        // 2. Scan each file: read metadata, insert into library
        let opts = crate::services::library::scan_opts(&state_inner);
        let mut track_ids: Vec<String> = Vec::with_capacity(total);
        for (i, file_path) in audio_files.iter().enumerate() {
            events.emit(AppEvent::ScanProgress(ScanProgress {
                scanned: i,
                total,
                current_path: file_path.to_string_lossy().to_string(),
            }));

            match metadata::read_metadata_all_with_opts(file_path, opts) {
                Ok(tracks) => {
                    let db = state_inner.db.lock();
                    for track in tracks {
                        match db.insert_track(&track) {
                            Ok(actual_id) => track_ids.push(actual_id),
                            Err(e) => {
                                log::error!("Failed to insert track {:?}: {}", file_path, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to read metadata for {:?}: {}", file_path, e);
                }
            }
        }

        // 3. Add all tracks to the playlist
        //
        // One transaction, which matters most here: this is the path that walks
        // a whole folder tree, so the batch is the entire scan.
        {
            let db = state_inner.db.lock();
            if let Err(e) = db.add_tracks_to_playlist(&playlist_id, &track_ids) {
                log::error!("Failed to add tracks to playlist: {}", e);
            }
        }

        // 4. Emit completion events
        events.emit(AppEvent::ScanComplete);
        events.emit(AppEvent::PlaylistCreated {
            id: playlist_id.clone(),
            track_count: track_ids.len(),
        });

        log::info!(
            "Created playlist '{}' with {} tracks from '{}'",
            playlist_id,
            track_ids.len(),
            path
        );
    });

    Ok(())
}
