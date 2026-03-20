use crate::db::models::Track;
use crate::metadata;
use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, State};
use walkdir::WalkDir;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "wav", "aac", "aiff", "wma", "m4a", "opus", "ape", "wv",
];

pub fn is_audio_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[tauri::command]
pub fn get_all_tracks(state: State<'_, Arc<AppState>>) -> Result<Vec<Track>, String> {
    state
        .db
        .lock()
        .get_all_tracks()
        .map_err(|e| e.to_string())
}

#[derive(Clone, serde::Serialize)]
struct ScanProgress {
    scanned: usize,
    total: usize,
    current_path: String,
}

pub fn scan_folder_sync(state: &Arc<AppState>, app: &tauri::AppHandle, path: &str) {
    let scan_path = PathBuf::from(path);

    let audio_files: Vec<PathBuf> = WalkDir::new(&scan_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_audio_file(e.path()))
        .map(|e| e.into_path())
        .collect();

    let total = audio_files.len();

    for (i, file_path) in audio_files.iter().enumerate() {
        let _ = app.emit(
            "scan-progress",
            ScanProgress {
                scanned: i + 1,
                total,
                current_path: file_path.to_string_lossy().to_string(),
            },
        );

        match metadata::read_metadata(file_path) {
            Ok(track) => {
                let db = state.db.lock();
                if let Err(e) = db.insert_track(&track) {
                    log::error!("Failed to insert track {}: {}", file_path.display(), e);
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to read metadata for {}: {}",
                    file_path.display(),
                    e
                );
            }
        }
    }

    let _ = app.emit("scan-complete", ());
}

#[tauri::command]
pub fn scan_folder(path: String, state: State<'_, Arc<AppState>>, app: tauri::AppHandle) {
    let state = state.inner().clone();
    let app = app.clone();

    std::thread::spawn(move || {
        scan_folder_sync(&state, &app, &path);
    });
}

#[tauri::command]
pub fn add_files(paths: Vec<String>, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let db = state.db.lock();

    for path_str in paths {
        let path = PathBuf::from(&path_str);
        if is_audio_file(&path) {
            match metadata::read_metadata(&path) {
                Ok(track) => {
                    if let Err(e) = db.insert_track(&track) {
                        log::error!("Failed to insert track {}: {}", path.display(), e);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to read metadata for {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn get_artwork(track_path: String) -> Result<Option<String>, String> {
    metadata::extract_artwork_base64(&PathBuf::from(track_path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_containing_folder(path: String) -> Result<(), String> {
    let file_path = PathBuf::from(&path);
    let folder = file_path.parent().unwrap_or(&file_path);

    #[cfg(target_os = "linux")]
    {
        // Try xdg-open first, fall back to nautilus/dolphin/thunar
        std::process::Command::new("xdg-open")
            .arg(folder)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    Ok(())
}
