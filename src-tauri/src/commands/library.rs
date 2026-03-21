use crate::audio::vgm_path::parse_vgm_path;
use crate::db::models::Track;
use crate::metadata;
use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, State};
use walkdir::WalkDir;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    // Standard audio
    "mp3", "flac", "ogg", "wav", "aac", "aiff", "wma", "m4a", "opus", "ape", "wv",
    // GME chiptune
    "nsf", "nsfe", "spc", "gbs", "vgm", "vgz", "hes", "kss", "ay", "sap", "gym",
    // vgmstream (Nintendo, common game audio)
    "bcstm", "bfstm", "brstm", "bcwav", "bfwav", "brwav",
    "adx", "hca", "aax", "scd", "at3", "at9",
    "dsp", "idsp", "bfsar", "bars",
    "fsb", "bnk", "wem", "mus",
    "xma", "xma2", "xwb",
    "genh", "txth", "txtp",
    "nub", "nus3bank", "lopus",
    "rwsd", "rwar", "rwav",
    "sad", "sgd", "sab",
    "acb", "awb",
    "ktss", "kvs",
    "csmp", "cstm",
    // PSF family (GBA, NDS, PS1, PS2, N64, Saturn, Dreamcast)
    "gsf", "minigsf",
    "2sf", "mini2sf",
    "psf", "minipsf",
    "psf2", "minipsf2",
    "usf", "miniusf",
    "ssf", "minissf",
    "dsf", "minidsf",
    "qsf", "miniqsf",
    "ncsf", "minincsf",
];

pub fn is_audio_file(path: &std::path::Path) -> bool {
    let ext_match = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false);

    if ext_match {
        return true;
    }

    // Also check vgmstream's dynamic extension list for formats not in our static list
    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
        return vgmstream_rs::Vgmstream::is_valid(filename);
    }

    false
}

#[tauri::command]
pub fn get_all_tracks(state: State<'_, Arc<AppState>>) -> Result<Vec<Track>, String> {
    state
        .db
        .lock()
        .get_all_tracks()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_track_rating(
    track_id: String,
    rating: i32,
    write_to_file: Option<bool>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // DB operations: get path and save rating
    let track_path = {
        let db = state.db.lock();
        let path = db
            .get_track_by_id(&track_id)
            .map_err(|e| e.to_string())?
            .map(|t| t.path);
        db.set_track_rating(&track_id, rating)
            .map_err(|e| e.to_string())?;
        path
    }; // DB lock released here

    // Write rating to the file's metadata (best-effort, no lock held)
    if write_to_file.unwrap_or(true) {
        if let Some(path) = track_path {
            match metadata::write_rating_to_file(&path, rating) {
                Ok(true) => log::info!("Rating {} written to file: {}", rating, path),
                Ok(false) => log::debug!("File format doesn't support rating writing: {}", path),
                Err(e) => log::warn!("Failed to write rating to file {}: {}", path, e),
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn get_faved_tracks(state: State<'_, Arc<AppState>>) -> Result<Vec<Track>, String> {
    state
        .db
        .lock()
        .get_faved_tracks()
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

        match metadata::read_metadata_all(file_path) {
            Ok(tracks) => {
                let db = state.db.lock();
                for track in tracks {
                    if let Err(e) = db.insert_track(&track) {
                        log::error!("Failed to insert track {}: {}", track.path, e);
                    }
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
            match metadata::read_metadata_all(&path) {
                Ok(tracks) => {
                    for track in tracks {
                        if let Err(e) = db.insert_track(&track) {
                            log::error!("Failed to insert track {}: {}", track.path, e);
                        }
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
pub fn resync_library(state: State<'_, Arc<AppState>>, app: tauri::AppHandle) {
    let state = state.inner().clone();
    let app = app.clone();

    std::thread::spawn(move || {
        // Clear all tracks
        {
            let db = state.db.lock();
            if let Err(e) = db.clear_all_tracks() {
                log::error!("Failed to clear tracks: {}", e);
                return;
            }
        }

        // Get monitored folders
        let folders = {
            let db = state.db.lock();
            db.get_monitored_folders().unwrap_or_default()
        };

        // Re-scan all monitored folders
        for folder in &folders {
            scan_folder_sync(&state, &app, &folder.path);
        }

        let _ = app.emit("scan-complete", ());
    });
}

#[tauri::command]
pub fn get_artwork(track_path: String) -> Result<Option<String>, String> {
    let (actual_path, _) = parse_vgm_path(&track_path);
    metadata::extract_artwork_base64(&PathBuf::from(actual_path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_track_metadata(
    track_ids: Vec<String>,
    fields: std::collections::HashMap<String, serde_json::Value>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let db = state.db.lock();

    let title = fields.get("title").and_then(|v| v.as_str());
    let artist = fields.get("artist").and_then(|v| v.as_str());
    let album = fields.get("album").and_then(|v| v.as_str());
    let album_artist = fields.get("album_artist").and_then(|v| v.as_str());
    let track_number = fields.get("track_number").map(|v| {
        if v.is_null() { None } else { v.as_i64().map(|n| n as i32) }
    });
    let disc_number = fields.get("disc_number").map(|v| {
        if v.is_null() { None } else { v.as_i64().map(|n| n as i32) }
    });

    for track_id in &track_ids {
        if let Err(e) = db.update_track_metadata(
            track_id,
            title,
            artist,
            album,
            album_artist,
            track_number,
            disc_number,
        ) {
            log::error!("Failed to update metadata for track {}: {}", track_id, e);
        }
    }

    Ok(())
}

#[tauri::command]
pub fn open_containing_folder(path: String) -> Result<(), String> {
    // Strip virtual path suffix (#N) for multi-track VGM files
    let (actual_path, _) = parse_vgm_path(&path);
    let file_path = PathBuf::from(actual_path);
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
            .args(["-R", actual_path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", actual_path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    Ok(())
}
