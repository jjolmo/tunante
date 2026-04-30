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

/// Augment in-memory tracks with ratings read from folder-level `_ratings.m3u`
/// files (for tracks whose DB rating is 0), and persist any new ratings back
/// to the DB so subsequent reads — including filtered ones like
/// `get_faved_tracks` — see them.
pub(crate) fn augment_ratings(state: &Arc<AppState>, tracks: &mut [Track]) {
    let updates = metadata::ratings_sync::apply_file_ratings(tracks);
    if updates.is_empty() {
        return;
    }
    let db = state.db.lock();
    for (id, rating) in updates {
        if let Err(e) = db.set_track_rating(&id, rating) {
            log::warn!("Failed to persist file-derived rating for {}: {}", id, e);
        }
    }
}

#[tauri::command]
pub fn get_all_tracks(state: State<'_, Arc<AppState>>) -> Result<Vec<Track>, String> {
    let mut tracks = state
        .db
        .lock()
        .get_all_tracks()
        .map_err(|e| e.to_string())?;
    augment_ratings(&state, &mut tracks);
    Ok(tracks)
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
    // First sync ratings from `_ratings.m3u` for the whole library, so tracks
    // that are only rated in files (e.g. synced from another machine) become
    // visible here too. Subsequent calls are cheap once the DB is in sync.
    let mut all = state
        .db
        .lock()
        .get_all_tracks()
        .map_err(|e| e.to_string())?;
    augment_ratings(&state, &mut all);
    Ok(all.into_iter().filter(|t| t.rating > 0).collect())
}

#[derive(Clone, serde::Serialize)]
struct ScanProgress {
    scanned: usize,
    total: usize,
    current_path: String,
}

pub fn scan_folder_sync(state: &Arc<AppState>, app: &tauri::AppHandle, path: &str) {
    let scan_path = PathBuf::from(path);
    log::info!("Scan started: {}", path);

    // Check if fast scan is enabled (skips silence detection for GME tracks)
    let fast_scan = {
        let db = state.db.lock();
        db.get_setting("fast_scan")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false)
    };
    if fast_scan {
        log::info!("Fast scan enabled — skipping silence detection");
    }

    let start_time = std::time::Instant::now();

    let audio_files: Vec<PathBuf> = WalkDir::new(&scan_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| {
            match e {
                Ok(entry) => Some(entry),
                Err(err) => {
                    log::warn!("Scan walk error: {}", err);
                    None
                }
            }
        })
        .filter(|e| e.file_type().is_file() && is_audio_file(e.path()))
        .map(|e| e.into_path())
        .collect();

    let total = audio_files.len();
    log::info!("Scan found {} audio files in {} (walk took {:?})", total, path, start_time.elapsed());

    let mut inserted = 0usize;
    let mut errors = 0usize;

    for (i, file_path) in audio_files.iter().enumerate() {
        let _ = app.emit(
            "scan-progress",
            ScanProgress {
                scanned: i + 1,
                total,
                current_path: file_path.to_string_lossy().to_string(),
            },
        );

        match metadata::read_metadata_all_with_opts(file_path, fast_scan) {
            Ok(tracks) => {
                let db = state.db.lock();
                for track in tracks {
                    match db.insert_track(&track) {
                        Ok(_) => inserted += 1,
                        Err(e) => {
                            log::error!("Failed to insert track {}: {}", track.path, e);
                            errors += 1;
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to read metadata for {}: {}",
                    file_path.display(),
                    e
                );
                errors += 1;
            }
        }
    }

    let elapsed = start_time.elapsed();
    log::info!(
        "Scan complete: {} — {} tracks inserted, {} errors, took {:.1}s",
        path, inserted, errors, elapsed.as_secs_f64()
    );

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

/// Save cover art bytes to the track's folder as cover.jpg (if store_in_folder is true).
fn save_cover_to_folder(track_path: &str, bytes: &[u8]) {
    let path = std::path::Path::new(track_path);
    if let Some(folder) = path.parent() {
        let cover_path = folder.join("cover.jpg");
        if !cover_path.exists() {
            match std::fs::write(&cover_path, bytes) {
                Ok(()) => log::info!("Saved cover to: {}", cover_path.display()),
                Err(e) => log::warn!("Failed to save cover to {}: {}", cover_path.display(), e),
            }
        }
    }
}

/// Fetch cover art from iTunes Search API with local file cache.
/// Returns base64 data URI if found, None otherwise.
#[tauri::command]
pub async fn fetch_cover_art(
    album: String,
    artist: String,
    track_path: Option<String>,
    store_in_folder: Option<bool>,
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

    // 7b. Save to track's folder if requested
    if store_in_folder.unwrap_or(false) {
        if let Some(ref tp) = track_path {
            save_cover_to_folder(tp, &bytes);
        }
    }

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

/// Strip the noise commonly found in game-music album/folder names so
/// downstream lookups (Libretro, Wikidata, Wikipedia) actually match.
///
/// Removes:
/// - Parenthesised metadata: `(1987-08-22)(Nintendo EAD)(Nintendo)`, `(USA)`, `(v1.0)`
/// - Bracketed alternate names: `[Estpolis Denki II]`, `[Lufia]`
/// - Curly-braced annotations: `{NTSC}`
/// - Trailing/leading punctuation and stray dashes
/// - Collapsed whitespace
pub(crate) fn sanitize_game_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_curly = 0i32;
    for ch in raw.chars() {
        match ch {
            '(' => depth_paren += 1,
            ')' => depth_paren = (depth_paren - 1).max(0),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = (depth_bracket - 1).max(0),
            '{' => depth_curly += 1,
            '}' => depth_curly = (depth_curly - 1).max(0),
            _ if depth_paren == 0 && depth_bracket == 0 && depth_curly == 0 => {
                out.push(ch);
            }
            _ => {}
        }
    }
    // Collapse whitespace and trim stray dashes/commas at the edges.
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim_matches(|c: char| c.is_whitespace() || c == '-' || c == ',' || c == '_' || c == '.')
        .to_string()
}

/// Build an ordered list of unique, non-empty candidate names to feed to
/// the cover-art lookups. Falls back to the parent folder name when the
/// album metadata is too sparse.
fn build_game_candidates(game_name: &str, track_path: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push_unique = |s: String| {
        let trimmed = s.trim().to_string();
        if trimmed.len() >= 2 && !out.iter().any(|x| x.eq_ignore_ascii_case(&trimmed)) {
            out.push(trimmed);
        }
    };

    let sanitized = sanitize_game_name(game_name);
    if !sanitized.is_empty() {
        push_unique(sanitized);
    }

    if let Some(tp) = track_path {
        let (real_path, _) = parse_vgm_path(tp);
        let path = std::path::Path::new(real_path);
        if let Some(folder) = path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) {
            let folder_sanitized = sanitize_game_name(folder);
            if !folder_sanitized.is_empty() {
                push_unique(folder_sanitized);
            }
        }
    }

    // Last-ditch: the raw album name as provided.
    let raw = game_name.trim().to_string();
    if !raw.is_empty() {
        push_unique(raw);
    }
    out
}

/// Search Wikidata for an entity that is an instance of "video game" (Q7889)
/// or a subclass, then resolve its P18 (image) claim to a Wikimedia Commons URL.
///
/// This is keyless and authoritative: filtering by P31 avoids the Wikipedia
/// disambiguation problem (where "Final Fantasy" returns the franchise page,
/// not a specific game).
async fn search_wikidata_cover(client: &reqwest::Client, game_name: &str) -> Option<String> {
    // Items whose P31 (instance of) we accept as "this is a video game".
    // Includes plain video game (Q7889), expansion packs, mods, demos, etc.
    const VIDEO_GAME_QIDS: &[&str] = &[
        "Q7889",   // video game
        "Q21125433", // role-playing video game franchise (rare but seen)
        "Q1066707", // game soundtrack — fallback
        "Q865493",  // video game series (last-resort)
    ];

    let search_url = format!(
        "https://www.wikidata.org/w/api.php?action=wbsearchentities&search={}&language=en&type=item&limit=10&format=json",
        urlencoding::encode(game_name)
    );
    let resp = client.get(&search_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: serde_json::Value = resp.json().await.ok()?;
    let candidates = data["search"].as_array()?;

    // Collect Q-IDs in order. Wikidata search ranks roughly by relevance.
    let qids: Vec<String> = candidates
        .iter()
        .filter_map(|c| c["id"].as_str().map(String::from))
        .take(8)
        .collect();
    if qids.is_empty() {
        return None;
    }

    // Batch-fetch claims for the candidates.
    let ids_param = qids.join("|");
    let entities_url = format!(
        "https://www.wikidata.org/w/api.php?action=wbgetentities&ids={}&props=claims|labels&languages=en&format=json",
        urlencoding::encode(&ids_param)
    );
    let resp = client.get(&entities_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: serde_json::Value = resp.json().await.ok()?;
    let entities = data["entities"].as_object()?;

    // Iterate in the original search order so the most relevant match wins.
    for qid in &qids {
        let entity = match entities.get(qid) {
            Some(e) => e,
            None => continue,
        };

        let claims = match entity["claims"].as_object() {
            Some(c) => c,
            None => continue,
        };

        // Check P31 (instance of) — accept any of our whitelist.
        let p31 = match claims.get("P31").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => continue,
        };
        let is_video_game = p31.iter().any(|claim| {
            claim["mainsnak"]["datavalue"]["value"]["id"]
                .as_str()
                .map(|id| VIDEO_GAME_QIDS.contains(&id))
                .unwrap_or(false)
        });
        if !is_video_game {
            continue;
        }

        // Read P18 (image) — the value is a Commons file name.
        let image_filename = claims
            .get("P18")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|claim| claim["mainsnak"]["datavalue"]["value"].as_str())
            .map(String::from);

        if let Some(filename) = image_filename {
            // Skip vector logos — boxart is what we want.
            if filename.to_lowercase().ends_with(".svg") {
                continue;
            }
            let url = format!(
                "https://commons.wikimedia.org/wiki/Special:FilePath/{}?width=600",
                urlencoding::encode(&filename)
            );
            log::info!("Wikidata match for '{}': {} → {}", game_name, qid, filename);
            return Some(url);
        }
    }
    None
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
///
/// The album/folder names embedded in chiptune metadata are notoriously dirty
/// (`Lufia II - Rise of the Sinistrals [Estpolis Denki II] [Lufia] (1995)(Neverland)(Taito)`,
/// `ct-102a.spc`, etc), so this command:
/// 1. Builds a list of sanitised candidate names (album → parent folder → raw)
/// 2. For each candidate, tries multiple keyless free sources in priority order:
///      a. Libretro thumbnails (retro box art database — exact No-Intro names)
///      b. Wikidata (P31=video game filtered → P18 image on Commons)
///      c. Wikipedia article page image
///      d. iTunes soundtrack (last resort)
/// 3. Caches hits and misses on disk so we don't hammer the network.
#[tauri::command]
pub async fn fetch_vgm_cover_art(
    game_name: String,
    console_name: String,
    track_path: Option<String>,
    store_in_folder: Option<bool>,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    use sha2::{Sha256, Digest};
    use base64::Engine;
    use tauri::Manager;

    if game_name.is_empty() && track_path.is_none() {
        return Ok(None);
    }

    // 1. Build candidate names (sanitised album, sanitised parent folder, raw album).
    let candidates = build_game_candidates(&game_name, track_path.as_deref());
    if candidates.is_empty() {
        return Ok(None);
    }
    let primary = candidates[0].clone();

    // 2. Compute cache key from the primary sanitised name + console.
    //    Bumping the namespace ("vgm3") invalidates stale `.miss` files from
    //    the previous algorithm so users get a fresh chance.
    let cache_key = {
        let input = format!("vgm3\0{}\0{}", primary.to_lowercase(), console_name.to_lowercase());
        let hash = Sha256::digest(input.as_bytes());
        format!("{:x}", hash)[..16].to_string()
    };

    let cache_dir = app.path().app_data_dir()
        .map_err(|e| format!("Cannot get app data dir: {}", e))?
        .join("covers");
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Cannot create covers cache dir: {}", e))?;

    let cache_path = cache_dir.join(format!("{}.img", cache_key));
    let miss_path = cache_dir.join(format!("{}.miss", cache_key));

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

    // Helper closure to finalize a successful fetch: persist cache, optionally
    // mirror to the track's folder, return as base64 data URI.
    let finalize = |bytes: Vec<u8>| -> Result<Option<String>, String> {
        std::fs::write(&cache_path, &bytes)
            .map_err(|e| format!("Failed to cache cover art: {}", e))?;
        if store_in_folder.unwrap_or(false) {
            if let Some(ref tp) = track_path {
                save_cover_to_folder(tp, &bytes);
            }
        }
        let mime = mime_from_bytes(&bytes);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(Some(format!("data:{};base64,{}", mime, b64)))
    };

    log::info!("VGM cover lookup for '{}' [{}] — candidates: {:?}", game_name, console_name, candidates);

    for candidate in &candidates {
        // === SOURCE 1: Libretro thumbnails (retro game box art) ===
        if let Some(system) = libretro_system_name(&console_name) {
            let clean_name = libretro_game_name(candidate);
            let base = "https://thumbnails.libretro.com";
            let encoded_system = urlencoding::encode(system);
            let region_suffixes = ["", " (USA)", " (USA, Europe)", " (Europe)", " (Japan)", " (World)"];
            for suffix in &region_suffixes {
                let full_name = format!("{}{}", clean_name, suffix);
                let encoded_name = urlencoding::encode(&full_name);
                let url = format!("{}/{}/Named_Boxarts/{}.png", base, encoded_system, encoded_name);
                if let Some(bytes) = try_download_image(&client, &url).await {
                    if bytes.len() > 100 {
                        log::info!("VGM cover from Libretro: '{}' → '{}'", candidate, full_name);
                        return finalize(bytes);
                    }
                }
            }
        }

        // === SOURCE 2: Wikidata (entity-typed video game with P18 image) ===
        if let Some(artwork_url) = search_wikidata_cover(&client, candidate).await {
            if let Some(bytes) = try_download_image(&client, &artwork_url).await {
                if bytes.len() > 100 {
                    log::info!("VGM cover from Wikidata: '{}'", candidate);
                    return finalize(bytes);
                }
            }
        }

        // === SOURCE 3: Wikipedia article page image ===
        if let Some(artwork_url) = search_wikipedia_cover(&client, candidate, &console_name).await {
            if let Some(bytes) = try_download_image(&client, &artwork_url).await {
                if bytes.len() > 100 {
                    log::info!("VGM cover from Wikipedia: '{}'", candidate);
                    return finalize(bytes);
                }
            }
        }
    }

    // === SOURCE 4: iTunes soundtrack search (cross-candidate, last resort) ===
    {
        let query = format!("{} soundtrack", primary);
        let url = format!(
            "https://itunes.apple.com/search?term={}&media=music&entity=album&limit=3",
            urlencoding::encode(&query)
        );
        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                if let Ok(data) = response.json::<serde_json::Value>().await {
                    let primary_lower = primary.to_lowercase();
                    let artwork_url = data["results"].as_array()
                        .and_then(|arr| {
                            arr.iter()
                                .find(|r| {
                                    r["collectionName"].as_str()
                                        .map(|n| n.to_lowercase().contains(&primary_lower))
                                        .unwrap_or(false)
                                })
                                .or_else(|| arr.first())
                        })
                        .and_then(|r| r["artworkUrl100"].as_str())
                        .map(|url| url.replace("100x100bb", "600x600bb"));

                    if let Some(art_url) = artwork_url {
                        if let Some(bytes) = try_download_image(&client, &art_url).await {
                            if bytes.len() > 100 {
                                log::info!("VGM cover from iTunes: '{}'", primary);
                                return finalize(bytes);
                            }
                        }
                    }
                }
            }
        }
    }

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

/// Open a file or folder in the system file manager on Linux.
/// If `select_file` is true, tries to highlight the file in the file manager.
#[cfg(target_os = "linux")]
fn linux_open_path(path: &str, select_file: bool) -> Result<(), String> {
    let target = PathBuf::from(path);
    let folder = if select_file {
        target.parent().unwrap_or(&target)
    } else {
        &target
    };

    log::info!("linux_open_path: path={}, folder={}, select={}", path, folder.display(), select_file);

    // Try Dolphin first (KDE) — forces a new window so it's always visible.
    // xdg-open reuses the existing Dolphin instance which may open a tab
    // in a background window that the user never sees.
    {
        let mut cmd = std::process::Command::new("dolphin");
        cmd.arg("--new-window");
        if select_file {
            cmd.arg("--select").arg(&target);
        } else {
            cmd.arg(folder);
        }
        if let Ok(_) = cmd
            .env_remove("GDK_BACKEND")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            log::info!("Opened via dolphin --new-window: {}", folder.display());
            return Ok(());
        }
    }

    // Fallback: xdg-open (GNOME, XFCE, etc.)
    let result = std::process::Command::new("xdg-open")
        .arg(folder)
        .env_remove("GDK_BACKEND")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(_) => {
            log::info!("Opened via xdg-open: {}", folder.display());
            Ok(())
        }
        Err(e) => {
            log::error!("xdg-open failed: {}", e);
            Err(format!("Failed to open folder: {}", e))
        }
    }
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
        linux_open_path(actual_path, true)?;
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

/// Open a folder in the system file manager.
#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let folder = PathBuf::from(&path);
    if !folder.exists() {
        return Err(format!("Folder does not exist: {}", path));
    }

    #[cfg(target_os = "linux")]
    {
        linux_open_path(&path, false)?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
pub fn is_directory(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}
