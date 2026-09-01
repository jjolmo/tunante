//! Tauri shells over `services::covers` — the bodies live there.
//!
//! The services are blocking on purpose (a lookup can take thirty seconds), so
//! every async shell here wraps its call in `spawn_blocking` — never on a
//! tokio worker. Another UI will call the same services from its own threads.

use crate::events::tauri_events;
use crate::services::covers as svc;
use crate::AppState;
use std::sync::Arc;
use tauri::State;
use tunante_art::resolver::Plan;

pub use crate::services::covers::{CoverOption, TrackNames};

#[tauri::command]
pub async fn resolve_cover(
    track_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || svc::resolve_cover(&state, &track_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refetch_cover(
    track_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || svc::refetch_cover(&state, &track_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn search_cover_options(
    track_path: String,
    query: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<CoverOption>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        svc::search_cover_options(&state, &track_path, query.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn choose_cover(
    track_path: String,
    url: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || svc::choose_cover(&state, &track_path, &url))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn preview_cover_downloads(
    scope: String,
    target: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<Vec<Plan>, String> {
    let state = state.inner().clone();
    let events = tauri_events(app);
    tauri::async_runtime::spawn_blocking(move || {
        svc::preview_cover_downloads(&state, &events, &scope, &target)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn download_covers(
    scope: String,
    target: String,
    replace_existing: bool,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<u64, String> {
    svc::download_covers(&state, tauri_events(app), &scope, &target, replace_existing)
}

#[tauri::command]
pub fn cancel_cover_download() {
    svc::cancel_cover_download();
}

#[tauri::command]
pub fn undo_cover_run(stamp: u64) -> Result<usize, String> {
    svc::undo_cover_run(stamp)
}

#[tauri::command]
pub fn clear_cover_cache(app: tauri::AppHandle) -> Result<u32, String> {
    use tauri::Manager;
    svc::clear_cover_cache(app.path().app_data_dir().ok())
}

#[tauri::command]
pub async fn suggest_game_names(
    console_id: String,
    query: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        svc::suggest_game_names(&state, &console_id, &query)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn suggest_track_names(
    track_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<TrackNames, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || svc::suggest_track_names(&state, &track_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn apply_track_names(
    file: String,
    titles: Vec<String>,
    lengths: Vec<String>,
    only_index: Option<usize>,
    replace: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<usize, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        svc::apply_track_names(&state, &file, &titles, &lengths, only_index, replace)
    })
    .await
    .map_err(|e| e.to_string())?
}
