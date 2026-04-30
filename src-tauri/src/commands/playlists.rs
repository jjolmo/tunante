use crate::commands::library::{augment_ratings, is_audio_file};
use crate::db::models::{Playlist, Track};
use crate::metadata;
use crate::AppState;
use std::sync::Arc;
use tauri::{Emitter, State};
use uuid::Uuid;
use walkdir::WalkDir;

#[tauri::command]
pub fn get_playlists(state: State<'_, Arc<AppState>>) -> Result<Vec<Playlist>, String> {
    state
        .db
        .lock()
        .get_playlists()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_playlist_tracks(
    playlist_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Track>, String> {
    let mut tracks = state
        .db
        .lock()
        .get_playlist_tracks(&playlist_id)
        .map_err(|e| e.to_string())?;
    augment_ratings(&state, &mut tracks);
    Ok(tracks)
}

#[tauri::command]
pub fn create_playlist(name: String, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    state
        .db
        .lock()
        .create_playlist(&id, &name)
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub fn delete_playlist(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state
        .db
        .lock()
        .delete_playlist(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_playlist(
    id: String,
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .db
        .lock()
        .rename_playlist(&id, &name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_playlists(
    ordered_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .db
        .lock()
        .reorder_playlists(&ordered_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_tracks_to_playlist(
    playlist_id: String,
    track_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let db = state.db.lock();
    for track_id in track_ids {
        let entry_id = Uuid::new_v4().to_string();
        db.add_track_to_playlist(&entry_id, &playlist_id, &track_id)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn remove_track_from_playlist(
    playlist_id: String,
    track_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .db
        .lock()
        .remove_track_from_playlist(&playlist_id, &track_id)
        .map_err(|e| e.to_string())
}

/// Scan a folder for audio files, add them to the library, and populate
/// an existing playlist with the discovered tracks. The playlist should be
/// created by the frontend first (so it appears immediately in the sidebar).
/// Runs in a background thread and emits scan-progress / scan-complete /
/// playlist-created events.
#[tauri::command]
pub fn create_playlist_from_folder(
    path: String,
    playlist_id: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let state_inner = state.inner().clone();
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
        let mut track_ids: Vec<String> = Vec::with_capacity(total);
        for (i, file_path) in audio_files.iter().enumerate() {
            let _ = app.emit("scan-progress", serde_json::json!({
                "scanned": i,
                "total": total,
                "current_path": file_path.to_string_lossy(),
            }));

            match metadata::read_metadata_all(file_path) {
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
        {
            let db = state_inner.db.lock();
            for track_id in &track_ids {
                let entry_id = Uuid::new_v4().to_string();
                if let Err(e) = db.add_track_to_playlist(&entry_id, &playlist_id, track_id) {
                    log::error!("Failed to add track to playlist: {}", e);
                }
            }
        }

        // 4. Emit completion events
        let _ = app.emit("scan-complete", ());
        let _ = app.emit("playlist-created", serde_json::json!({
            "id": playlist_id,
            "track_count": track_ids.len(),
        }));

        log::info!(
            "Created playlist '{}' with {} tracks from '{}'",
            playlist_id, track_ids.len(), path
        );
    });

    Ok(())
}
