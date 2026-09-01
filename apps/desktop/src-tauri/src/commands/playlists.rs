//! Tauri shells over `services::playlists` — the bodies live there.

use crate::db::models::{Playlist, Track};
use crate::services::playlists as svc;
use crate::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_playlists(state: State<'_, Arc<AppState>>) -> Result<Vec<Playlist>, String> {
    svc::get_playlists(&state)
}

#[tauri::command]
pub fn get_playlist_tracks(
    playlist_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Track>, String> {
    svc::get_playlist_tracks(&state, &playlist_id)
}

#[tauri::command]
pub fn create_playlist(name: String, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    svc::create_playlist(&state, &name)
}

#[tauri::command]
pub fn delete_playlist(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::delete_playlist(&state, &id)
}

#[tauri::command]
pub fn rename_playlist(
    id: String,
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::rename_playlist(&state, &id, &name)
}

#[tauri::command]
pub fn reorder_playlists(
    ordered_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::reorder_playlists(&state, &ordered_ids)
}

#[tauri::command]
pub fn add_tracks_to_playlist(
    playlist_id: String,
    track_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::add_tracks_to_playlist(&state, &playlist_id, &track_ids)
}

#[tauri::command]
pub fn remove_track_from_playlist(
    playlist_id: String,
    track_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::remove_track_from_playlist(&state, &playlist_id, &track_id)
}

#[tauri::command]
pub fn create_playlist_from_folder(
    path: String,
    playlist_id: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    svc::create_playlist_from_folder(&state, crate::events::tauri_events(app), path, playlist_id)
}
