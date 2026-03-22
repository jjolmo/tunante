use crate::db::models::{MonitoredFolder, Setting};
use crate::AppState;
use std::sync::Arc;
use tauri::{Manager, State};
use uuid::Uuid;

#[cfg(target_os = "linux")]
use std::path::PathBuf;

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

#[tauri::command]
pub fn set_tray_visible(visible: bool, app: tauri::AppHandle) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_visible(visible).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Returns the path where the .desktop file would be created, or empty string on non-Linux.
#[tauri::command]
pub fn get_desktop_entry_path() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let path = PathBuf::from(home)
                .join(".local/share/applications/tunante.desktop");
            return path.to_string_lossy().to_string();
        }
    }
    String::new()
}

/// Creates a .desktop entry for Tunante on Linux.
/// Copies the app icon and writes the .desktop file.
#[tauri::command]
pub fn create_desktop_entry(app: tauri::AppHandle) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let home_path = PathBuf::from(&home);

        // Find the current executable path
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Cannot find executable: {}", e))?;

        // Create icon directory
        let icon_dir = home_path.join(".local/share/icons");
        std::fs::create_dir_all(&icon_dir)
            .map_err(|e| format!("Cannot create icon dir: {}", e))?;

        // Copy icon from Tauri resources
        let icon_dest = icon_dir.join("tunante.png");
        let resource_path = app.path()
            .resource_dir()
            .map_err(|e| format!("Cannot get resource dir: {}", e))?;

        // Try to find the icon in resource dir or alongside the executable
        let icon_source_candidates = vec![
            resource_path.join("icons/128x128.png"),
            resource_path.join("icons/icon.png"),
            exe_path.parent().unwrap_or(std::path::Path::new("/")).join("icons/128x128.png"),
        ];

        let mut icon_copied = false;
        for candidate in &icon_source_candidates {
            if candidate.exists() {
                std::fs::copy(candidate, &icon_dest)
                    .map_err(|e| format!("Cannot copy icon: {}", e))?;
                icon_copied = true;
                break;
            }
        }

        // If no resource icon found, try to extract from the binary's embedded icon
        if !icon_copied {
            // Use a placeholder — the .desktop entry will still work without an icon
            log::warn!("No icon found in resource dir, .desktop entry will have no icon");
        }

        // Create .desktop file
        let desktop_dir = home_path.join(".local/share/applications");
        std::fs::create_dir_all(&desktop_dir)
            .map_err(|e| format!("Cannot create applications dir: {}", e))?;

        let desktop_path = desktop_dir.join("tunante.desktop");
        let exe_str = exe_path.to_string_lossy();
        let icon_str = if icon_dest.exists() {
            icon_dest.to_string_lossy().to_string()
        } else {
            "audio-x-generic".to_string()
        };

        let desktop_content = format!(
            "[Desktop Entry]\n\
             Name=Tunante\n\
             Comment=Cross-platform music player for video game music\n\
             Exec=env GDK_BACKEND=x11 WEBKIT_EXEC_PATH=/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 \"{exe_str}\"\n\
             Icon={icon_str}\n\
             Terminal=false\n\
             Type=Application\n\
             Categories=Audio;Music;Player;\n\
             MimeType=audio/mpeg;audio/ogg;audio/flac;audio/wav;\n"
        );

        std::fs::write(&desktop_path, desktop_content)
            .map_err(|e| format!("Cannot write .desktop file: {}", e))?;

        return Ok(desktop_path.to_string_lossy().to_string());
    }

    #[cfg(not(target_os = "linux"))]
    Err("Desktop entries are only supported on Linux".to_string())
}
