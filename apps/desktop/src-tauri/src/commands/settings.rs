use crate::db::models::{MonitoredFolder, PinnedFolder, Setting};
use crate::AppState;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
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
    let result = state
        .db
        .lock()
        .set_setting(&key, &value)
        .map_err(|e| e.to_string());

    // Reordering the rating sources changes what a resolution would find, so
    // let it run again instead of waiting for the next app start.
    if key == crate::metadata::rating_source::SETTING_KEY {
        crate::commands::library::reset_rating_resolution();
    }

    result
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

/// True if `inner` is the same path as `outer` or nested inside it.
pub(crate) fn is_path_within(inner: &str, outer: &str) -> bool {
    if inner == outer {
        return true;
    }
    let prefix = if outer.ends_with('/') || outer.ends_with('\\') {
        outer.to_string()
    } else {
        format!("{}/", outer)
    };
    inner.starts_with(&prefix)
}

#[tauri::command]
pub fn add_monitored_folder(
    path: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<MonitoredFolder, String> {
    let id = Uuid::new_v4().to_string();

    // --- Overlap handling: monitored folders must never nest inside each other ---
    // Otherwise a recursive scan/watch would cover the same files twice, and
    // removing one folder would delete tracks still owned by the other.
    let existing = state
        .db
        .lock()
        .get_monitored_folders()
        .map_err(|e| e.to_string())?;

    // Case A: the new folder is already covered by an existing monitored folder.
    if let Some(parent) = existing.iter().find(|f| is_path_within(&path, &f.path)) {
        return Err(format!(
            "This folder is already covered by monitored folder: {}",
            parent.path
        ));
    }

    // Case B: the new folder is a parent of one or more existing folders.
    // Absorb them — drop their records and watchers, but keep their tracks
    // (this folder's scan will re-own them).
    let children: Vec<MonitoredFolder> = existing
        .iter()
        .filter(|f| is_path_within(&f.path, &path))
        .cloned()
        .collect();
    if !children.is_empty() {
        let mut watcher_lock = state.watcher.lock();
        for child in &children {
            if let Some(ref mut watcher) = *watcher_lock {
                let _ = watcher.stop_watching(&child.path);
            }
        }
        drop(watcher_lock);

        let db = state.db.lock();
        for child in &children {
            if let Err(e) = db.remove_monitored_folder(&child.id) {
                log::error!("Failed to absorb child folder {}: {}", child.path, e);
            } else {
                log::info!("Absorbed child folder into {}: {}", path, child.path);
            }
        }
        drop(db);
    }

    state
        .db
        .lock()
        .add_monitored_folder(&id, &path)
        .map_err(|e| e.to_string())?;

    // Scan first, THEN start watching.
    // On macOS, PollWatcher traverses the entire directory tree on its first
    // cycle. Running it concurrently with the initial scan causes FD exhaustion
    // on large libraries. Scanning first ensures all files are indexed, then
    // the watcher picks up future changes only.
    let state_inner = state.inner().clone();
    let scan_path = path.clone();
    let id_clone = id.clone();
    std::thread::spawn(move || {
        crate::commands::library::scan_folder_sync(&state_inner, &app, &scan_path);
        let db = state_inner.db.lock();
        let _ = db.update_folder_scan_time(&id_clone);
        drop(db);

        // Now start watching for future changes
        let mut watcher_lock = state_inner.watcher.lock();
        if let Some(ref mut watcher) = *watcher_lock {
            if let Err(e) = watcher.start_watching(&scan_path) {
                log::error!("Failed to start watching {}: {}", scan_path, e);
            } else {
                log::info!("Started watching: {}", scan_path);
            }
        }
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
    app: tauri::AppHandle,
) -> Result<(), String> {
    let db = state.db.lock();
    let folders = db.get_monitored_folders().map_err(|e| e.to_string())?;
    let folder = folders.iter().find(|f| f.id == id).cloned();
    drop(db);

    if let Some(ref folder) = folder {
        // Stop file watcher for this folder
        let mut watcher_lock = state.watcher.lock();
        if let Some(ref mut watcher) = *watcher_lock {
            let _ = watcher.stop_watching(&folder.path);
        }
        drop(watcher_lock);

        // Remove tracks belonging to this folder, but keep any still covered by
        // another monitored folder OR a pinned folder, so we never orphan tracks
        // that a remaining entry still relies on.
        let db = state.db.lock();
        let mut keep_prefixes: Vec<String> = db
            .get_monitored_folders()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.id != id)
            .map(|f| f.path)
            .collect();
        keep_prefixes.extend(
            db.get_pinned_folders()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.path),
        );
        match db.remove_tracks_by_folder_path_excluding(&folder.path, &keep_prefixes) {
            Ok(count) => {
                log::info!("Removed {} tracks from folder: {}", count, folder.path);
            }
            Err(e) => {
                log::error!("Failed to remove tracks for folder {}: {}", folder.path, e);
            }
        }
        drop(db);
    }

    // Remove the folder record from DB
    let db = state.db.lock();
    db.remove_monitored_folder(&id)
        .map_err(|e| e.to_string())?;

    // If no monitored folders remain, wipe the entire library to ensure no orphans
    let remaining = db.get_monitored_folders().unwrap_or_default();
    if remaining.is_empty() {
        if let Err(e) = db.clear_all_tracks() {
            log::error!("Failed to clear all tracks: {}", e);
        } else {
            log::info!("Last monitored folder removed — cleared all tracks");
        }
    }
    drop(db);

    // Notify frontend to refresh the track list
    let _ = app.emit("library-updated", ());

    Ok(())
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

// --- Pinned folders ---
// A pinned folder is a folder-based "playlist": it shows in the sidebar Folders
// list and its contents are derived live from the library by path prefix, so it
// auto-updates as files are added/removed. Unlike monitored folders, pinned
// folders MAY nest inside a monitored folder, and unpinning never deletes tracks
// that are still covered by a monitored folder or another pin.

#[tauri::command]
pub fn get_pinned_folders(state: State<'_, Arc<AppState>>) -> Result<Vec<PinnedFolder>, String> {
    state
        .db
        .lock()
        .get_pinned_folders()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_pinned_folder(
    path: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<PinnedFolder, String> {
    let id = Uuid::new_v4().to_string();

    // Reject redundant pins.
    let covered_by_monitored = {
        let db = state.db.lock();
        let monitored = db.get_monitored_folders().map_err(|e| e.to_string())?;
        if monitored.iter().any(|f| f.path == path) {
            return Err("This folder is already a monitored folder.".to_string());
        }
        let pins = db.get_pinned_folders().map_err(|e| e.to_string())?;
        if pins.iter().any(|p| p.path == path) {
            return Err("This folder is already pinned.".to_string());
        }
        monitored.iter().any(|f| is_path_within(&path, &f.path))
    };

    state
        .db
        .lock()
        .add_pinned_folder(&id, &path)
        .map_err(|e| e.to_string())?;

    // Scan the folder so its tracks are present (idempotent upsert), then start
    // an independent watcher only if no monitored folder already covers it
    // (avoids double-watching the same files).
    let state_inner = state.inner().clone();
    let scan_path = path.clone();
    std::thread::spawn(move || {
        crate::commands::library::scan_folder_sync(&state_inner, &app, &scan_path);
        if !covered_by_monitored {
            let mut watcher_lock = state_inner.watcher.lock();
            if let Some(ref mut watcher) = *watcher_lock {
                if let Err(e) = watcher.start_watching(&scan_path) {
                    log::error!("Failed to watch pinned folder {}: {}", scan_path, e);
                } else {
                    log::info!("Started watching pinned folder: {}", scan_path);
                }
            }
        }
    });

    Ok(PinnedFolder {
        id,
        path,
        added_at: 0,
    })
}

#[tauri::command]
pub fn remove_pinned_folder(
    id: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let db = state.db.lock();
    let pins = db.get_pinned_folders().map_err(|e| e.to_string())?;
    let pin = pins.iter().find(|p| p.id == id).cloned();
    drop(db);

    let pin = match pin {
        Some(p) => p,
        None => return Ok(()),
    };

    // Stop any independent watcher on this path. If the path was only covered by
    // a monitored folder's recursive watch (never watched individually), this is
    // a harmless no-op and does NOT disturb the parent watch.
    {
        let mut watcher_lock = state.watcher.lock();
        if let Some(ref mut watcher) = *watcher_lock {
            let _ = watcher.stop_watching(&pin.path);
        }
    }

    // Remove this folder's tracks, but keep any still covered by a monitored
    // folder or another pinned folder — so unpinning never orphans shared tracks.
    let db = state.db.lock();
    let mut keep: Vec<String> = db
        .get_monitored_folders()
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.path)
        .collect();
    keep.extend(
        db.get_pinned_folders()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.id != id)
            .map(|p| p.path),
    );
    if let Err(e) = db.remove_tracks_by_folder_path_excluding(&pin.path, &keep) {
        log::error!("Failed to remove tracks for pinned folder {}: {}", pin.path, e);
    }
    db.remove_pinned_folder(&id).map_err(|e| e.to_string())?;
    drop(db);

    let _ = app.emit("library-updated", ());
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

/// Embedded 128x128 PNG icon — compiled into the binary so it works
/// regardless of runtime paths (AppImage, dev mode, installed binary).
#[cfg(target_os = "linux")]
static ICON_PNG: &[u8] = include_bytes!("../../icons/128x128.png");

/// Creates a .desktop entry for Tunante on Linux.
/// Writes the embedded icon and generates the .desktop file.
#[tauri::command]
pub fn create_desktop_entry(_app: tauri::AppHandle) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let home_path = PathBuf::from(&home);

        // Determine the real executable path:
        // - AppImage: use $APPIMAGE env var (the .AppImage file itself)
        //   current_exe() returns the temp mount path which disappears on exit
        // - Release binary / dev: use current_exe() directly
        let exe_str = if let Ok(appimage) = std::env::var("APPIMAGE") {
            appimage
        } else {
            std::env::current_exe()
                .map_err(|e| format!("Cannot find executable: {}", e))?
                .to_string_lossy()
                .to_string()
        };

        // Write embedded icon to ~/.local/share/icons/tunante.png
        let icon_dir = home_path.join(".local/share/icons");
        std::fs::create_dir_all(&icon_dir)
            .map_err(|e| format!("Cannot create icon dir: {}", e))?;
        let icon_dest = icon_dir.join("tunante.png");
        std::fs::write(&icon_dest, ICON_PNG)
            .map_err(|e| format!("Cannot write icon: {}", e))?;

        // Create .desktop file
        let desktop_dir = home_path.join(".local/share/applications");
        std::fs::create_dir_all(&desktop_dir)
            .map_err(|e| format!("Cannot create applications dir: {}", e))?;

        let desktop_path = desktop_dir.join("tunante.desktop");
        let icon_str = icon_dest.to_string_lossy();

        let desktop_content = format!(
            "[Desktop Entry]\n\
             Name=Tunante\n\
             Comment=Cross-platform music player for video game music\n\
             Exec=env GDK_BACKEND=x11 WEBKIT_EXEC_PATH=/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 \"{exe_str}\"\n\
             Icon={icon_str}\n\
             Terminal=false\n\
             Type=Application\n\
             Categories=Player;\n\
             MimeType=audio/mpeg;audio/ogg;audio/flac;audio/wav;\n"
        );

        std::fs::write(&desktop_path, desktop_content)
            .map_err(|e| format!("Cannot write .desktop file: {}", e))?;

        return Ok(desktop_path.to_string_lossy().to_string());
    }

    #[cfg(not(target_os = "linux"))]
    Err("Desktop entries are only supported on Linux".to_string())
}
