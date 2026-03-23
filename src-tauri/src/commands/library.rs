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
    "dsp", "idsp", "bfsar", "bars", "strm", "csmp", "cstm",
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

/// Fetch cover art from iTunes Search API with local file cache.
/// Returns base64 data URI if found, None otherwise.
#[tauri::command]
pub async fn fetch_cover_art(
    album: String,
    artist: String,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    use sha2::{Sha256, Digest};
    use base64::Engine;
    use tauri::Manager;

    // 1. Compute cache key
    let cache_key = {
        let input = format!("{}\0{}", album.to_lowercase(), artist.to_lowercase());
        let hash = Sha256::digest(input.as_bytes());
        format!("{:x}", hash)[..16].to_string()
    };

    // 2. Resolve cache directory
    let cache_dir = app.path().app_data_dir()
        .map_err(|e| format!("Cannot get app data dir: {}", e))?
        .join("covers");
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Cannot create covers cache dir: {}", e))?;

    let cache_path = cache_dir.join(format!("{}.jpg", cache_key));
    let miss_path = cache_dir.join(format!("{}.miss", cache_key));

    // 3. Check cache
    if cache_path.exists() {
        let bytes = std::fs::read(&cache_path)
            .map_err(|e| format!("Cannot read cached cover: {}", e))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Ok(Some(format!("data:image/jpeg;base64,{}", b64)));
    }
    if miss_path.exists() {
        return Ok(None); // Already searched, not found
    }

    // 4. Search iTunes API
    let query = if !artist.is_empty() && !album.is_empty() {
        format!("{} {}", album, artist)
    } else if !album.is_empty() {
        album.clone()
    } else {
        artist.clone()
    };

    let url = format!(
        "https://itunes.apple.com/search?term={}&media=music&entity=album&limit=3",
        urlencoding::encode(&query)
    );

    let client = reqwest::Client::builder()
        .user_agent("Tunante/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&url).send().await
        .map_err(|e| format!("iTunes search failed: {}", e))?;

    if !response.status().is_success() {
        // Don't cache network errors — might be temporary
        return Ok(None);
    }

    let data: serde_json::Value = response.json().await
        .map_err(|e| format!("Failed to parse iTunes response: {}", e))?;

    // 5. Find best match
    let results = data["results"].as_array();
    let artwork_url = results
        .and_then(|arr| {
            // Try to find exact album name match first
            let album_lower = album.to_lowercase();
            arr.iter()
                .find(|r| {
                    r["collectionName"].as_str()
                        .map(|n| n.to_lowercase().contains(&album_lower))
                        .unwrap_or(false)
                })
                .or_else(|| arr.first())
        })
        .and_then(|r| r["artworkUrl100"].as_str())
        .map(|url| url.replace("100x100bb", "600x600bb"));

    let artwork_url = match artwork_url {
        Some(url) => url,
        None => {
            // No results — cache the miss
            let _ = std::fs::write(&miss_path, b"");
            return Ok(None);
        }
    };

    // 6. Download the artwork
    let img_response = client.get(&artwork_url).send().await
        .map_err(|e| format!("Failed to download artwork: {}", e))?;

    if !img_response.status().is_success() {
        let _ = std::fs::write(&miss_path, b"");
        return Ok(None);
    }

    let bytes = img_response.bytes().await
        .map_err(|e| format!("Failed to read artwork bytes: {}", e))?;

    // 7. Save to cache
    std::fs::write(&cache_path, &bytes)
        .map_err(|e| format!("Failed to cache cover art: {}", e))?;

    // 8. Return as base64 data URI
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(Some(format!("data:image/jpeg;base64,{}", b64)))
}

/// Detect MIME type from the first bytes of an image file.
fn mime_from_bytes(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        "image/jpeg"
    } else if bytes.len() >= 4 && bytes[..4] == [0x89, 0x50, 0x4E, 0x47] {
        "image/png"
    } else {
        "image/jpeg"
    }
}

/// Map console ID to Libretro thumbnail system name.
fn libretro_system_name(console_name: &str) -> Option<&'static str> {
    match console_name {
        "NES" => Some("Nintendo - Nintendo Entertainment System"),
        "SNES" => Some("Nintendo - Super Nintendo Entertainment System"),
        "Game Boy" => Some("Nintendo - Game Boy"),
        "GB Advance" => Some("Nintendo - Game Boy Advance"),
        "Nintendo DS" => Some("Nintendo - Nintendo DS"),
        "Nintendo 64" => Some("Nintendo - Nintendo 64"),
        "Nintendo 3DS" => Some("Nintendo - Nintendo 3DS"),
        "GameCube" => Some("Nintendo - GameCube"),
        "Wii" => Some("Nintendo - Wii"),
        "Wii U" => Some("Nintendo - Wii U"),
        "Sega Genesis" => Some("Sega - Mega Drive - Genesis"),
        "Sega Saturn" => Some("Sega - Saturn"),
        "Sega Dreamcast" => Some("Sega - Dreamcast"),
        "PlayStation" => Some("Sony - PlayStation"),
        "PlayStation 2" => Some("Sony - PlayStation 2"),
        "TurboGrafx-16" => Some("NEC - PC Engine - TurboGrafx 16"),
        "MSX" => Some("Microsoft - MSX"),
        "Atari" => Some("Atari - 2600"),
        "ZX Spectrum" => Some("Sinclair - ZX Spectrum"),
        _ => None,
    }
}

/// Sanitize a game name for Libretro thumbnail URL (special char replacement).
fn libretro_game_name(name: &str) -> String {
    name.replace('&', "_")
        .replace('/', "_")
        .replace('\\', "_")
        .replace(':', "")
        .replace('?', "")
        .replace('*', "")
        .replace('\"', "")
        .replace('<', "")
        .replace('>', "")
        .replace('|', "")
        .trim()
        .to_string()
}

/// Try to download an image from a URL. Returns bytes if successful.
async fn try_download_image(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.bytes().await.ok().map(|b| b.to_vec())
}

/// Search Wikipedia for a game's page image (box art).
async fn search_wikipedia_cover(
    client: &reqwest::Client,
    game_name: &str,
    console_name: &str,
) -> Option<String> {
    // Try multiple search queries in order of specificity
    let queries = if !console_name.is_empty() {
        vec![
            format!("\"{}\" {} video game", game_name, console_name),
            format!("{} {} video game", game_name, console_name),
            format!("{} video game", game_name),
        ]
    } else {
        vec![
            format!("\"{}\" video game", game_name),
            format!("{} video game", game_name),
        ]
    };

    let game_lower = game_name.to_lowercase();

    for query in &queries {
        let url = format!(
            "https://en.wikipedia.org/w/api.php?action=query&generator=search&gsrsearch={}&gsrlimit=5&prop=pageimages&piprop=original&format=json",
            urlencoding::encode(query)
        );

        let response = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let data: serde_json::Value = match response.json().await {
            Ok(d) => d,
            Err(_) => continue,
        };

        let artwork_url = data["query"]["pages"].as_object()
            .and_then(|pages| {
                // Try exact title match first
                pages.values()
                    .find(|p| {
                        p["title"].as_str()
                            .map(|t| t.to_lowercase().contains(&game_lower))
                            .unwrap_or(false)
                            && p["original"]["source"].as_str()
                                .map(|u| !u.contains(".svg"))
                                .unwrap_or(false)
                    })
                    .or_else(|| {
                        pages.values().find(|p| {
                            p["original"]["source"].as_str()
                                .map(|u| !u.contains(".svg"))
                                .unwrap_or(false)
                        })
                    })
            })
            .and_then(|page| page["original"]["source"].as_str().map(String::from));

        if artwork_url.is_some() {
            return artwork_url;
        }
    }
    None
}

/// Fetch cover art for video game music.
/// Searches multiple sources in priority order:
///   1. Libretro thumbnails (retro game box art database, direct URL)
///   2. Wikipedia (game article page image)
///   3. iTunes (music album artwork, last resort)
#[tauri::command]
pub async fn fetch_vgm_cover_art(
    game_name: String,
    console_name: String,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    use sha2::{Sha256, Digest};
    use base64::Engine;
    use tauri::Manager;

    if game_name.is_empty() {
        return Ok(None);
    }

    // 1. Compute cache key
    let cache_key = {
        let input = format!("vgm2\0{}\0{}", game_name.to_lowercase(), console_name.to_lowercase());
        let hash = Sha256::digest(input.as_bytes());
        format!("{:x}", hash)[..16].to_string()
    };

    // 2. Resolve cache directory
    let cache_dir = app.path().app_data_dir()
        .map_err(|e| format!("Cannot get app data dir: {}", e))?
        .join("covers");
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Cannot create covers cache dir: {}", e))?;

    let cache_path = cache_dir.join(format!("{}.img", cache_key));
    let miss_path = cache_dir.join(format!("{}.miss", cache_key));

    // 3. Check cache
    if cache_path.exists() {
        let bytes = std::fs::read(&cache_path)
            .map_err(|e| format!("Cannot read cached cover: {}", e))?;
        let mime = mime_from_bytes(&bytes);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return Ok(Some(format!("data:{};base64,{}", mime, b64)));
    }
    if miss_path.exists() {
        return Ok(None);
    }

    let client = reqwest::Client::builder()
        .user_agent("Tunante/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    // === SOURCE 1: Libretro thumbnails (retro game box art) ===
    if let Some(system) = libretro_system_name(&console_name) {
        let clean_name = libretro_game_name(&game_name);
        let base = "https://thumbnails.libretro.com";
        let encoded_system = urlencoding::encode(system);
        let encoded_name = urlencoding::encode(&clean_name);
        let libretro_url = format!("{}/{}/Named_Boxarts/{}.png", base, encoded_system, encoded_name);

        if let Some(bytes) = try_download_image(&client, &libretro_url).await {
            if bytes.len() > 100 { // Sanity check: not an error page
                std::fs::write(&cache_path, &bytes)
                    .map_err(|e| format!("Failed to cache cover art: {}", e))?;
                let mime = mime_from_bytes(&bytes);
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                log::info!("VGM cover from Libretro: {} ({})", game_name, console_name);
                return Ok(Some(format!("data:{};base64,{}", mime, b64)));
            }
        }
    }

    // === SOURCE 2: Wikipedia (game article page image) ===
    if let Some(artwork_url) = search_wikipedia_cover(&client, &game_name, &console_name).await {
        if let Some(bytes) = try_download_image(&client, &artwork_url).await {
            std::fs::write(&cache_path, &bytes)
                .map_err(|e| format!("Failed to cache cover art: {}", e))?;
            let mime = mime_from_bytes(&bytes);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            log::info!("VGM cover from Wikipedia: {}", game_name);
            return Ok(Some(format!("data:{};base64,{}", mime, b64)));
        }
    }

    // === SOURCE 3: iTunes (music soundtrack, last resort) ===
    {
        let query = format!("{} soundtrack", game_name);
        let url = format!(
            "https://itunes.apple.com/search?term={}&media=music&entity=album&limit=3",
            urlencoding::encode(&query)
        );
        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                if let Ok(data) = response.json::<serde_json::Value>().await {
                    let game_lower = game_name.to_lowercase();
                    let artwork_url = data["results"].as_array()
                        .and_then(|arr| {
                            arr.iter()
                                .find(|r| {
                                    r["collectionName"].as_str()
                                        .map(|n| n.to_lowercase().contains(&game_lower))
                                        .unwrap_or(false)
                                })
                                .or_else(|| arr.first())
                        })
                        .and_then(|r| r["artworkUrl100"].as_str())
                        .map(|url| url.replace("100x100bb", "600x600bb"));

                    if let Some(art_url) = artwork_url {
                        if let Some(bytes) = try_download_image(&client, &art_url).await {
                            std::fs::write(&cache_path, &bytes)
                                .map_err(|e| format!("Failed to cache cover art: {}", e))?;
                            let mime = mime_from_bytes(&bytes);
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                            log::info!("VGM cover from iTunes: {}", game_name);
                            return Ok(Some(format!("data:{};base64,{}", mime, b64)));
                        }
                    }
                }
            }
        }
    }

    // No source found — cache the miss
    let _ = std::fs::write(&miss_path, b"");
    Ok(None)
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

    log::info!("open_containing_folder: path={}, folder={}", path, folder.display());

    if !folder.exists() {
        return Err(format!("Folder does not exist: {}", folder.display()));
    }

    #[cfg(target_os = "linux")]
    {
        // Use dbus to ask the file manager to show the file (highlights it)
        // Falls back to xdg-open on the folder if dbus fails
        let file_uri = format!("file://{}", file_path.display());
        let dbus_result = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{}", file_uri),
                "string:",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();

        match dbus_result {
            Ok(output) if output.status.success() => {
                log::info!("Opened via dbus FileManager1");
            }
            _ => {
                log::info!("dbus FileManager1 failed, falling back to xdg-open");
                std::process::Command::new("xdg-open")
                    .arg(folder)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Failed to open folder: {}", e))?;
            }
        }
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

#[tauri::command]
pub fn is_directory(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}
