//! Tauri shells over `services::library` — the bodies live there.

use crate::db::models::Track;
use crate::AppState;
use std::sync::Arc;
use tauri::State;

// Re-exported so the neighbours that always reached these through
// `commands::library` (settings, covers, the watcher) keep resolving. They
// live in `services::library` now.
pub use crate::services::library::{is_audio_file, AUDIO_EXTENSIONS};
pub(crate) use crate::services::library::{reset_rating_resolution, scan_opts};

/// Compat wrapper for callers that still hold an `AppHandle` (settings.rs);
/// removed when those are extracted in their turn.
pub fn scan_folder_sync(state: &Arc<AppState>, app: &tauri::AppHandle, path: &str) {
    crate::services::library::scan_folder_sync(state, &crate::events::tauri_events(app.clone()), path);
}

#[tauri::command]
pub fn get_all_tracks(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<Vec<Track>, String> {
    crate::services::library::get_all_tracks(&state, &crate::events::tauri_events(app))
}

#[tauri::command]
pub fn set_track_rating(
    track_id: String,
    rating: i32,
    write_to_file: Option<bool>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    crate::services::library::set_track_rating(&state, &track_id, rating, write_to_file)
}

#[tauri::command]
pub fn get_faved_tracks(state: State<'_, Arc<AppState>>) -> Result<Vec<Track>, String> {
    crate::services::library::get_faved_tracks(&state)
}

#[tauri::command]
pub fn scan_folder(path: String, state: State<'_, Arc<AppState>>, app: tauri::AppHandle) {
    crate::services::library::scan_folder(&state, crate::events::tauri_events(app), path);
}

#[tauri::command]
pub fn add_files(paths: Vec<String>, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    crate::services::library::add_files(&state, paths)
}

#[tauri::command]
pub fn resync_library(state: State<'_, Arc<AppState>>, app: tauri::AppHandle) {
    crate::services::library::resync_library(&state, crate::events::tauri_events(app));
}

#[tauri::command]
pub fn get_artwork(track_path: String) -> Result<Option<String>, String> {
    crate::services::library::get_artwork(&track_path)
}

#[tauri::command]
pub fn update_track_metadata(
    track_ids: Vec<String>,
    fields: std::collections::HashMap<String, serde_json::Value>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    crate::services::library::update_track_metadata(&state, &track_ids, &fields)
}

#[tauri::command]
pub fn open_containing_folder(path: String) -> Result<(), String> {
    crate::services::library::open_containing_folder(&path)
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    crate::services::library::open_folder(&path)
}

#[tauri::command]
pub fn is_directory(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}
