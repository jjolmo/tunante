use crate::db::models::{Playlist, Track};
use crate::AppState;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

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
    state
        .db
        .lock()
        .get_playlist_tracks(&playlist_id)
        .map_err(|e| e.to_string())
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
