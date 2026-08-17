use crate::audio::vgm_path::parse_vgm_path;
use crate::db::models::Track;
use crate::metadata;
use crate::AppState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Max play time, in milliseconds, for tracks whose real length can't be
/// determined — in practice the ones that loop forever.
///
/// Only reached at the end of the duration cascade: a track with a length in
/// its `.m3u`, an internal `play_length`, or one that ends on its own keeps its
/// real duration and ignores this entirely.
pub(crate) fn loop_max_ms(state: &Arc<AppState>) -> i64 {
    let stored = {
        let db = state.db.lock();
        db.get_setting("loop_max_seconds").ok().flatten()
    };
    stored
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|secs| *secs > 0)
        .map(|secs| secs * 1000)
        .unwrap_or(metadata::gme_reader_default_duration_ms())
}

/// How many times a looping vgmstream stream should repeat, from settings.
pub(crate) fn vgm_loop_count(state: &Arc<AppState>) -> f64 {
    let stored = {
        let db = state.db.lock();
        db.get_setting("vgm_loop_count").ok().flatten()
    };
    stored
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|n| *n >= 0.0 && *n <= 20.0)
        .unwrap_or(vgmstream_rs::Vgmstream::DEFAULT_LOOP_COUNT)
}

/// All scan knobs the user controls, read in one place so every path that
/// reads metadata gets the same answer.
pub(crate) fn scan_opts(state: &Arc<AppState>) -> metadata::ScanOpts {
    let fast_scan = {
        let db = state.db.lock();
        db.get_setting("fast_scan")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false)
    };
    let loop_max_caps_all = {
        let db = state.db.lock();
        db.get_setting("loop_max_caps_all")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false)
    };
    metadata::ScanOpts {
        fast_scan,
        loop_max_ms: loop_max_ms(state),
        vgm_loop_count: vgm_loop_count(state),
        loop_max_caps_all,
    }
}

/// Read the user's rating-source priority from settings.
pub(crate) fn rating_order(state: &Arc<AppState>) -> Vec<metadata::rating_source::RatingSource> {
    let raw = {
        let db = state.db.lock();
        db.get_setting(metadata::rating_source::SETTING_KEY)
            .ok()
            .flatten()
    };
    metadata::rating_source::parse_order(raw.as_deref())
}

/// Resolve each track's rating following the user's priority order, and persist
/// whatever won back to the DB so later filtered reads (`get_faved_tracks`) agree.
///
/// When the order starts with `db` — the default — this is the cheap path: the
/// DB value wins outright and no file is touched. Only an order that puts
/// `file` or `folder` ahead of `db` makes this hit the disk.
pub(crate) fn augment_ratings(state: &Arc<AppState>, tracks: &mut [Track]) {
    let order = rating_order(state);

    // `db` first means the stored value always wins where it is set; the old
    // `_ratings.m3u` sync still fills in tracks the DB has at 0, which is what
    // keeps ratings synced from other machines showing up. This path is cheap:
    // one `.m3u` read per folder, cached.
    if order.first() == Some(&metadata::rating_source::RatingSource::Db) {
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
    // Any other order needs to read from disk per track. That is NOT done here:
    // see `spawn_rating_resolution`. Doing it inline blocked the window on
    // startup for seconds on a large library.
}

/// Whether the disk-backed rating resolution has already run this session.
static RATINGS_RESOLVED: AtomicBool = AtomicBool::new(false);

/// Allow the resolution to run again, e.g. after the user reorders the sources.
/// Without this, changing the priority would do nothing until the next start.
pub(crate) fn reset_rating_resolution() {
    RATINGS_RESOLVED.store(false, Ordering::SeqCst);
}

/// Resolve ratings from disk in the background, once per run.
///
/// ⚠️ This must never block a command. With `file` or `folder` ahead of `db`,
/// resolving means opening a file (or its folder's `_ratings.m3u`) for every
/// track: on a 30k-track library sitting on a synced folder that is seconds of
/// I/O. It used to run inline inside `get_all_tracks`, which is the first thing
/// the window waits for, so the app came up black until it finished.
///
/// The DB already holds usable ratings, so the list paints immediately and this
/// only refines it, emitting `library-updated` if anything actually changed.
fn spawn_rating_resolution(state: &Arc<AppState>, app: &tauri::AppHandle) {
    let order = rating_order(state);
    if order.first() == Some(&metadata::rating_source::RatingSource::Db) {
        return;
    }
    if RATINGS_RESOLVED.swap(true, Ordering::SeqCst) {
        return;
    }

    let state = state.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let tracks = {
            let db = state.db.lock();
            match db.get_all_tracks() {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("Rating resolution: could not read tracks: {}", e);
                    return;
                }
            }
        };

        let started = std::time::Instant::now();
        let mut updates: Vec<(String, i32)> = Vec::new();
        for track in &tracks {
            let resolved =
                metadata::rating_source::resolve_rating(&track.path, track.rating, &order);
            if resolved != track.rating {
                updates.push((track.id.clone(), resolved));
            }
        }

        if updates.is_empty() {
            log::info!(
                "Rating resolution: {} tracks checked in {:?}, nothing to change",
                tracks.len(),
                started.elapsed()
            );
            return;
        }

        let changed = updates.len();
        {
            let db = state.db.lock();
            for (id, rating) in updates {
                if let Err(e) = db.set_track_rating(&id, rating) {
                    log::warn!("Failed to persist resolved rating for {}: {}", id, e);
                }
            }
        }
        log::info!(
            "Rating resolution: {} of {} tracks updated in {:?}",
            changed,
            tracks.len(),
            started.elapsed()
        );
        let _ = app.emit("library-updated", ());
    });
}

#[tauri::command]
pub fn get_all_tracks(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<Vec<Track>, String> {
    let mut tracks = state
        .db
        .lock()
        .get_all_tracks()
        .map_err(|e| e.to_string())?;
    augment_ratings(&state, &mut tracks);
    spawn_rating_resolution(state.inner(), &app);
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

    // Persist to disk following the user's priority order (no lock held).
    //
    // The DB was already updated above and always holds the value; this decides
    // which on-disk destination is authoritative. If the chosen one can't take
    // the rating — a NSF has no writable tag area — it falls through to the
    // next in the order instead of silently dropping it.
    if write_to_file.unwrap_or(true) {
        if let Some(path) = track_path {
            let order = rating_order(&state);
            let outcome = metadata::rating_source::write_rating(&path, rating, &order);
            match outcome.stored_in {
                Some(metadata::rating_source::RatingSource::Db) => {
                    log::debug!("Rating {} stored in the database only: {}", rating, path)
                }
                Some(src) => {
                    if outcome.skipped.is_empty() {
                        log::info!("Rating {} stored in {}: {}", rating, src, path);
                    } else {
                        let skipped: Vec<&str> =
                            outcome.skipped.iter().map(|s| s.as_key()).collect();
                        log::info!(
                            "Rating {} stored in {} (fell back from {}): {}",
                            rating,
                            src,
                            skipped.join(", "),
                            path
                        );
                    }
                }
                None => log::warn!(
                    "Could not store rating {} in any destination: {}",
                    rating,
                    path
                ),
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

    let opts = scan_opts(state);

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

        match metadata::read_metadata_all_with_opts(file_path, opts) {
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

    let pruned = prune_missing_tracks(state, path, total);

    let elapsed = start_time.elapsed();
    log::info!(
        "Scan complete: {} — {} tracks inserted, {} pruned, {} errors, took {:.1}s",
        path, inserted, pruned, errors, elapsed.as_secs_f64()
    );

    let _ = app.emit("scan-complete", ());
}

/// Drop tracks under `path` whose file is gone, so moved or deleted folders stop
/// leaving unplayable rows behind. Returns how many were removed.
///
/// `files_found` is the number of audio files the walk just turned up; when it is
/// zero the prune is skipped entirely. An unmounted drive looks exactly like a
/// folder whose files were all deleted, and wiping the library on a missing mount
/// would be far worse than leaving a few stale rows.
fn prune_missing_tracks(state: &Arc<AppState>, path: &str, files_found: usize) -> usize {
    if files_found == 0 {
        log::warn!(
            "Scan found no audio files in {} — skipping orphan prune (missing mount?)",
            path
        );
        return 0;
    }

    let known = {
        let db = state.db.lock();
        match db.get_track_paths_under(path) {
            Ok(paths) => paths,
            Err(e) => {
                log::error!("Failed to list tracks under {} for pruning: {}", path, e);
                return 0;
            }
        }
    };

    // Subtunes of one file share a base path ("foo.nsf#0", "foo.nsf#1"), so the
    // existence check is cached per base path — otherwise a chiptune-heavy
    // library would stat the same file hundreds of times.
    let mut exists: HashMap<&str, bool> = HashMap::new();
    let mut orphans: Vec<String> = Vec::new();

    for track_path in &known {
        let (base, _) = parse_vgm_path(track_path);
        let present = *exists
            .entry(base)
            .or_insert_with(|| Path::new(base).exists());
        if !present {
            orphans.push(track_path.clone());
        }
    }

    if orphans.is_empty() {
        return 0;
    }

    // A half-mounted or still-syncing network folder looks like a huge deletion.
    // Refusing to prune past half the folder keeps that from emptying the
    // library; a genuinely large deletion still clears via a full rescan.
    if orphans.len() * 2 > known.len() {
        log::warn!(
            "Skipping orphan prune under {}: {} of {} tracks appear missing, \
             which looks like an unavailable folder rather than a deletion",
            path,
            orphans.len(),
            known.len()
        );
        return 0;
    }

    let db = state.db.lock();
    match db.remove_tracks_by_paths(&orphans) {
        Ok(n) => {
            log::info!("Pruned {} tracks with missing files under {}", n, path);
            n
        }
        Err(e) => {
            log::error!("Failed to prune missing tracks under {}: {}", path, e);
            0
        }
    }
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
    // Read before locking: reading settings takes the DB lock itself, and
    // parking_lot mutexes are not reentrant.
    let opts = scan_opts(&state);
    let db = state.db.lock();

    for path_str in paths {
        let path = PathBuf::from(&path_str);
        if is_audio_file(&path) {
            match metadata::read_metadata_all_with_opts(&path, opts) {
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

/// Wipe the downloaded cover-art cache (`<app_data>/covers/`). Both successful
/// hits (`.img`/`.jpg`) and miss markers (`.miss`) are removed so subsequent
/// playback re-runs the full lookup pipeline. Returns the number of files deleted.
#[tauri::command]
pub fn clear_cover_cache(app: tauri::AppHandle) -> Result<u32, String> {
    use tauri::Manager;
    let cache_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot get app data dir: {}", e))?
        .join("covers");
    if !cache_dir.exists() {
        return Ok(0);
    }
    let mut count = 0u32;
    let entries = std::fs::read_dir(&cache_dir)
        .map_err(|e| format!("Cannot read covers cache dir: {}", e))?;
    for entry in entries.flatten() {
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            if std::fs::remove_file(entry.path()).is_ok() {
                count += 1;
            }
        }
    }
    log::info!("Cleared cover cache: {} files removed", count);
    Ok(count)
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

    // === SOURCE 1: Libretro thumbnails (most precise — exact box-art database) ===
    // Try every candidate against Libretro before falling back to fuzzier
    // sources, so an exact match on a sanitized folder name wins over a
    // best-effort Wikipedia hit on a dirty album field like "Lufia 2".
    if let Some(system) = libretro_system_name(&console_name) {
        let base = "https://thumbnails.libretro.com";
        let encoded_system = urlencoding::encode(system);
        let region_suffixes = ["", " (USA)", " (USA, Europe)", " (Europe)", " (Japan)", " (World)"];
        for candidate in &candidates {
            let clean_name = libretro_game_name(candidate);
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
    }

    // === SOURCE 2: Wikidata (entity-typed video game with P18 image) ===
    for candidate in &candidates {
        if let Some(artwork_url) = search_wikidata_cover(&client, candidate).await {
            if let Some(bytes) = try_download_image(&client, &artwork_url).await {
                if bytes.len() > 100 {
                    log::info!("VGM cover from Wikidata: '{}'", candidate);
                    return finalize(bytes);
                }
            }
        }
    }

    // === SOURCE 3: Wikipedia article page image (fuzzy — last keyless source) ===
    for candidate in &candidates {
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

/// Build a `file://` URI from an absolute path, percent-encoding each segment
/// (spaces, commas, etc.) while keeping the path separators intact.
#[cfg(target_os = "linux")]
fn path_to_file_uri(path: &str) -> String {
    let encoded: Vec<String> = path
        .split('/')
        .map(|seg| urlencoding::encode(seg).into_owned())
        .collect();
    format!("file://{}", encoded.join("/"))
}

/// Open a file or folder in the system file manager on Linux.
/// If `select_file` is true, tries to highlight the file in the file manager.
#[cfg(target_os = "linux")]
/// Lanza un programa del SISTEMA (xdg-open, dolphin, dbus-send, xdg-mime…) con el
/// entorno saneado. El runtime del AppImage inyecta LD_LIBRARY_PATH y rutas de
/// plugins (Qt/GTK) apuntando a las libs del propio AppImage; si el hijo las
/// hereda, carga librerias equivocadas y falla. Por eso "abrir carpeta" funciona
/// en dev/ejecutable pelado pero se rompe en el AppImage. Quitando esas variables
/// el hijo usa las del sistema. (En dev no estan puestas, asi que es no-op.)
#[cfg(target_os = "linux")]
fn system_command(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    for var in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "GDK_BACKEND",
        "GTK_PATH",
        "GIO_MODULE_DIR",
        "GDK_PIXBUF_MODULE_FILE",
        "GDK_PIXBUF_MODULEDIR",
        "QT_PLUGIN_PATH",
        "QT_QPA_PLATFORM_PLUGIN_PATH",
        "GSETTINGS_SCHEMA_DIR",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "PYTHONPATH",
        "PERLLIB",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

#[cfg(target_os = "linux")]
fn linux_open_path(path: &str, select_file: bool) -> Result<(), String> {
    let target = PathBuf::from(path);
    let folder = if select_file {
        target.parent().unwrap_or(&target)
    } else {
        &target
    };

    log::info!("linux_open_path: path={}, folder={}, select={}", path, folder.display(), select_file);

    // Which file manager owns directories? Reveal-with-selection via the freedesktop
    // `FileManager1.ShowItems` D-Bus interface is only reliable on Dolphin (KDE) and
    // Nautilus (GNOME). Others — notably **Nemo** — reply "success" over D-Bus without
    // ever opening a window, so trusting that success left "open containing folder"
    // doing nothing on KDE/Cinnamon setups that use Nemo. For those we skip ShowItems
    // and just open the containing folder (Nemo has no CLI file-select flag anyway).
    let default_fm = system_command("xdg-mime")
        .args(["query", "default", "inode/directory"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
        .unwrap_or_default();

    if select_file
        && (default_fm.contains("dolphin") || default_fm.contains("nautilus") || default_fm.is_empty())
    {
        let uri = path_to_file_uri(path);
        let status = system_command("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{}", uri))
            .arg("string:")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if let Ok(s) = status {
            if s.success() {
                log::info!("Opened via FileManager1.ShowItems: {}", uri);
                return Ok(());
            }
        }
        log::warn!("FileManager1.ShowItems unavailable — falling back to opening the folder");
    }

    // Open the containing folder with the user's default file manager. Dolphin gets an
    // explicit --new-window (xdg-open would reuse a background tab); everything else goes
    // through xdg-open, which respects the default handler (Nemo, Nautilus, Thunar, Caja…).
    if default_fm.contains("dolphin") {
        if system_command("dolphin")
            .arg("--new-window")
            .arg(folder)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            log::info!("Opened folder via dolphin --new-window: {}", folder.display());
            return Ok(());
        }
    }

    match system_command("xdg-open")
        .arg(folder)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {
            log::info!("Opened folder via xdg-open: {}", folder.display());
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
