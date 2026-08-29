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

// The static extension list lives in tunante-core, shared with tunante-mini so
// the two scanners cannot drift apart.
pub use tunante_core::vgm_path::AUDIO_EXTENSIONS;

pub fn is_audio_file(path: &std::path::Path) -> bool {
    if tunante_core::vgm_path::is_audio_file(path) {
        return true;
    }

    // Also check vgmstream's dynamic extension list for formats not in our static list
    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
        return tunante_codec::vgmstream_accepts(filename);
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
        .unwrap_or(tunante_codec::DEFAULT_VGM_LOOP_COUNT)
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
