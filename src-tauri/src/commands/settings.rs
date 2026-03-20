use crate::db::models::{MonitoredFolder, Setting};
use crate::AppState;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Result<Vec<Setting>, String> {
    state
        .db
        .lock()
        .get_all_settings()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_setting(key: String, state: State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    state
        .db
        .lock()
        .get_setting(&key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(
    key: String,
    value: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .db
        .lock()
        .set_setting(&key, &value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_monitored_folders(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<MonitoredFolder>, String> {
    state
        .db
        .lock()
        .get_monitored_folders()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_monitored_folder(
    path: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<MonitoredFolder, String> {
    let id = Uuid::new_v4().to_string();

    state
        .db
        .lock()
        .add_monitored_folder(&id, &path)
        .map_err(|e| e.to_string())?;

    // Start watching
    let mut watcher_lock = state.watcher.lock();
    if let Some(ref mut watcher) = *watcher_lock {
        if let Err(e) = watcher.start_watching(&path) {
            log::error!("Failed to start watching {}: {}", path, e);
        }
    }
    drop(watcher_lock);

    // Trigger initial scan in background
    let state_inner = state.inner().clone();
    let scan_path = path.clone();
    let id_clone = id.clone();
    std::thread::spawn(move || {
        crate::commands::library::scan_folder_sync(&state_inner, &app, &scan_path);
        let db = state_inner.db.lock();
        let _ = db.update_folder_scan_time(&id_clone);
    });

    Ok(MonitoredFolder {
        id,
        path,
        watching_enabled: true,
        last_scanned_at: 0,
        added_at: 0,
    })
}

#[tauri::command]
pub fn remove_monitored_folder(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let db = state.db.lock();
    let folders = db.get_monitored_folders().map_err(|e| e.to_string())?;
    let folder = folders.iter().find(|f| f.id == id).cloned();
    drop(db);

    if let Some(folder) = folder {
        let mut watcher_lock = state.watcher.lock();
        if let Some(ref mut watcher) = *watcher_lock {
            let _ = watcher.stop_watching(&folder.path);
        }
        drop(watcher_lock);
    }

    state
        .db
        .lock()
        .remove_monitored_folder(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_folder_watching(
    id: String,
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .db
        .lock()
        .toggle_folder_watching(&id, enabled)
        .map_err(|e| e.to_string())?;

    let db = state.db.lock();
    let folders = db.get_monitored_folders().map_err(|e| e.to_string())?;
    let folder = folders.iter().find(|f| f.id == id).cloned();
    drop(db);

    if let Some(folder) = folder {
        let mut watcher_lock = state.watcher.lock();
        if let Some(ref mut watcher) = *watcher_lock {
            if enabled {
                watcher
                    .start_watching(&folder.path)
                    .map_err(|e| e.to_string())?;
            } else {
                let _ = watcher.stop_watching(&folder.path);
            }
        }
    }

    Ok(())
}
