//! Downloading cover art, for one track or for a whole library.
//!
//! The finding, matching and fetching all live in `tunante-art`, which the
//! phone and the postmarketOS build share. What is left here is what is
//! genuinely about *this* app: turning tracks in the database into requests,
//! and reporting progress to a window.
//!
//! # The bulk run is dry by default, and that is deliberate
//!
//! It writes `cover.jpg` into folders inside the user's own library, which is
//! very likely inside a sync client. A wrong cover then has to be deleted by
//! hand on every device it reached. So the flow is preview → review → apply,
//! only matches at [`Confidence::High`] or better are ever applied unattended,
//! and every run records what it created so it can be undone.

use crate::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, State};
use tunante_art::folder::{Manifest, Overwrite};
use tunante_art::resolver::{BulkOptions, CoverRequest, Plan, Resolver};
use tunante_art::{cache, Confidence};
use tunante_core::console::{self, CONSOLES};
use tunante_core::db::models::Track;
use tunante_core::vgm_path;

/// Set while a bulk run is going, so a second one cannot start and Cancel has
/// something to talk to.
fn running() -> &'static Arc<AtomicBool> {
    static R: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    R.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

fn cancel_flag() -> &'static Arc<AtomicBool> {
    static C: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    C.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

fn resolver() -> Arc<Resolver> {
    static R: OnceLock<Arc<Resolver>> = OnceLock::new();
    Arc::clone(R.get_or_init(|| Arc::new(Resolver::new())))
}

/// Every archive there is, so a multiplatform game can be found under the
/// platform it was actually released on.
fn all_systems() -> Vec<(String, String)> {
    CONSOLES
        .iter()
        .filter_map(|c| c.libretro.map(|s| (c.id.to_string(), s.to_string())))
        .collect()
}

/// Turn a track into a lookup.
///
/// The candidate order matters and is the reason abbreviated folders work: the
/// resolved `game` comes first, and for a rip that is the album tag — an SPC's
/// ID666 header names the game even when the folder is called `ct/`.
fn request_for(track: &Track, store_in_folder: bool) -> CoverRequest {
    let mut candidates = Vec::new();
    if !track.game.trim().is_empty() {
        candidates.push(track.game.clone());
    }
    if !track.album.trim().is_empty() && !track.album.eq_ignore_ascii_case(&track.game) {
        candidates.push(track.album.clone());
    }

    let (real, _) = vgm_path::parse_vgm_path(&track.path);
    let dir = store_in_folder
        .then(|| std::path::Path::new(real).parent().map(|p| p.to_path_buf()))
        .flatten();

    let all = all_systems();
    CoverRequest {
        libretro_system: console::by_id(&track.console_id).and_then(|c| c.libretro).map(str::to_string),
        other_systems: all.into_iter().filter(|(o, _)| *o != track.console_id).collect(),
        console_id: track.console_id.clone(),
        candidates,
        dir,
    }
}

/// The cover for one track, as a `data:` URI, downloading it if needed.
///
/// Replaces the old `fetch_vgm_cover_art` *and* `fetch_cover_art`. There is no
/// longer a fork in the frontend deciding which pipeline a track belongs to —
/// that decision was made with a console name minted in a `.ts` file, and
/// renaming a label there silently disabled box-art lookups.
#[tauri::command]
pub async fn resolve_cover(
    track_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    let store = state
        .db
        .lock()
        .get_setting("store_covers_in_folder")
        .ok()
        .flatten()
        .as_deref()
        == Some("true");
    let track = state
        .db
        .lock()
        .get_track_by_path(&track_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no such track: {track_path}"))?;
    let req = request_for(&track, store);

    // Never on the async runtime's thread: a lookup can take thirty seconds and
    // would park a tokio worker for all of it.
    let found = tauri::async_runtime::spawn_blocking(move || {
        resolver().resolve_and_store(&req, Confidence::High, Overwrite::Never)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(found.0.map(|f| {
        use base64::Engine;
        format!(
            "data:{};base64,{}",
            f.info.format.mime(),
            base64::engine::general_purpose::STANDARD.encode(&f.bytes)
        )
    }))
}

/// Fetch this track's cover again, ignoring everything remembered about it.
///
/// For "that cover is wrong". Three things have to give way, and missing any
/// one of them makes the button look broken:
///
/// - the cache, which is doing its job by returning the same answer;
/// - `Overwrite::Never`, since the file to replace is the one we wrote;
/// - and a `Low`-confidence result is accepted here, because the user asked
///   for this one specifically rather than letting a bulk run decide.
#[tauri::command]
pub async fn refetch_cover(
    track_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    let track = state
        .db
        .lock()
        .get_track_by_path(&track_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no such track: {track_path}"))?;
    let req = request_for(&track, true);

    let found = tauri::async_runtime::spawn_blocking(move || {
        let r = resolver();
        r.forget(&req);
        r.resolve_and_store(&req, Confidence::Low, Overwrite::Replace)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(found.0.map(|f| {
        use base64::Engine;
        format!(
            "data:{};base64,{}",
            f.info.format.mime(),
            base64::engine::general_purpose::STANDARD.encode(&f.bytes)
        )
    }))
}

/// Which tracks a scope covers, one per game.
fn tracks_for_scope(state: &AppState, scope: &str, target: &str) -> Result<Vec<Track>, String> {
    let db = state.db.lock();
    let all = match scope {
        "folder" => db.get_tracks_by_folder(target).map_err(|e| e.to_string())?,
        "console" => db
            .get_all_tracks()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|t| t.console_id == target)
            .collect(),
        "playlist" => db.get_playlist_tracks(target).map_err(|e| e.to_string())?,
        _ => db.get_all_tracks().map_err(|e| e.to_string())?,
    };

    // One request per game, not per track: a hundred tracks of one soundtrack
    // want one cover between them. Deduplicated on the *matcher's* notion of
    // sameness, so "Chrono Trigger" and "Chrono Trigger OST" are not looked up
    // twice — they would resolve to the same cover anyway.
    let mut seen = std::collections::HashSet::new();
    Ok(all
        .into_iter()
        .filter(|t| seen.insert((t.console_id.clone(), tunante_art::name::normalize(&t.game).key)))
        .collect())
}

/// What a bulk run would do, without doing any of it.
#[tauri::command]
pub async fn preview_cover_downloads(
    scope: String,
    target: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Plan>, String> {
    let tracks = tracks_for_scope(&state, &scope, &target)?;
    let reqs: Vec<CoverRequest> = tracks.iter().map(|t| request_for(t, false)).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let opts = BulkOptions { dry_run: true, ..Default::default() };
        resolver().resolve_many(reqs, &opts, |_| {})
    })
    .await
    .map_err(|e| e.to_string())
}

/// Run it for real. Progress arrives as `cover-progress`, the summary as
/// `cover-complete` — the same shape as the library scan's events.
#[tauri::command]
pub fn download_covers(
    scope: String,
    target: String,
    replace_existing: bool,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<u64, String> {
    if running().swap(true, Ordering::SeqCst) {
        return Err("a cover download is already running".into());
    }
    cancel_flag().store(false, Ordering::SeqCst);

    let tracks = match tracks_for_scope(&state, &scope, &target) {
        Ok(t) => t,
        Err(e) => {
            running().store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    let reqs: Vec<CoverRequest> = tracks.iter().map(|t| request_for(t, true)).collect();

    // A caller-supplied stamp: `tunante-art` keeps no clock of its own so its
    // behaviour stays reproducible under test.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let manifest = Manifest::new(&cache::cache_dir(), stamp).map_err(|e| e.to_string())?;
    let cancel = Arc::clone(cancel_flag());

    // A plain thread, like `scan_folder_sync`. This has no business on the
    // async runtime.
    std::thread::spawn(move || {
        let opts = BulkOptions {
            dry_run: false,
            min_confidence: Confidence::High,
            overwrite: if replace_existing { Overwrite::Replace } else { Overwrite::Never },
            cancel,
            ..Default::default()
        };
        let plans = resolver().resolve_many(reqs, &opts, |p| {
            let _ = app.emit("cover-progress", p);
        });
        // `written`, never `existing`. The second is the image that was already
        // in the folder and was deliberately left alone; recording it here would
        // make Undo delete the user's own artwork.
        for p in plans.iter().filter_map(|p| p.written.as_ref()) {
            let _ = manifest.record(std::path::Path::new(p));
        }
        let _ = app.emit("cover-complete", &plans);
        running().store(false, Ordering::SeqCst);
    });

    Ok(stamp)
}

#[tauri::command]
pub fn cancel_cover_download() {
    cancel_flag().store(true, Ordering::SeqCst);
}

/// Delete exactly the files one run created, and nothing else.
#[tauri::command]
pub fn undo_cover_run(stamp: u64) -> Result<usize, String> {
    Manifest::undo(&cache::cache_dir(), stamp).map_err(|e| e.to_string())
}

/// Empty the cover cache. Also clears the directory the desktop app used before
/// the cache moved to a platform cache dir the phone can reach too.
#[tauri::command]
pub fn clear_cover_cache(app: tauri::AppHandle) -> Result<u32, String> {
    use tauri::Manager;
    let mut n = cache::clear().map_err(|e| e.to_string())?;
    if let Ok(old) = app.path().app_data_dir() {
        n += cache::clear_legacy(&old.join("covers")).unwrap_or(0);
    }
    Ok(n)
}
