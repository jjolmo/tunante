//! Tauri shells over `services::settings` — the bodies live there. The one
//! body that stayed is `set_tray_visible`: it talks to the tray handle and
//! nothing else, so it *is* shell.

use crate::db::models::{MonitoredFolder, PinnedFolder, Setting};
use crate::events::tauri_events;
use crate::services::settings as svc;
use crate::AppState;
use std::sync::Arc;
use tauri::State;

pub(crate) use crate::services::settings::is_path_within;
pub use crate::services::settings::refresh_desktop_icon;

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Result<Vec<Setting>, String> {
    svc::get_settings(&state)
}

#[tauri::command]
pub fn get_setting(key: String, state: State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    svc::get_setting(&state, &key)
}

#[tauri::command]
pub fn set_setting(
    key: String,
    value: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::set_setting(&state, &key, &value)
}

#[tauri::command]
pub fn get_monitored_folders(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<MonitoredFolder>, String> {
    svc::get_monitored_folders(&state)
}

#[tauri::command]
pub fn add_monitored_folder(
    path: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<MonitoredFolder, String> {
    svc::add_monitored_folder(&state, tauri_events(app), path)
}

#[tauri::command]
pub fn remove_monitored_folder(
    id: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    svc::remove_monitored_folder(&state, &tauri_events(app), &id)
}

#[tauri::command]
pub fn toggle_folder_watching(
    id: String,
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::toggle_folder_watching(&state, &id, enabled)
}

#[tauri::command]
pub fn get_pinned_folders(state: State<'_, Arc<AppState>>) -> Result<Vec<PinnedFolder>, String> {
    svc::get_pinned_folders(&state)
}

#[tauri::command]
pub fn add_pinned_folder(
    path: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<PinnedFolder, String> {
    svc::add_pinned_folder(&state, tauri_events(app), path)
}

#[tauri::command]
pub fn remove_pinned_folder(
    id: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    svc::remove_pinned_folder(&state, &tauri_events(app), &id)
}

#[tauri::command]
pub fn set_tray_visible(visible: bool, app: tauri::AppHandle) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_visible(visible).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_desktop_entry_path() -> String {
    svc::get_desktop_entry_path()
}

#[tauri::command]
pub fn create_desktop_entry(_app: tauri::AppHandle) -> Result<String, String> {
    svc::create_desktop_entry()
}
