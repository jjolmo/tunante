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
use tunante_art::resolver::{self, BulkOptions, CoverRequest, Plan, Resolver};
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
    let candidates = resolver::candidates_for(&track.game, &track.album, &track.path);

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
    // Default on, and that is deliberate. Writing the cover next to the track is
    // the whole storage strategy: it is what gets art onto the phone and onto
    // postmarketOS without either of them fetching anything, and it is what
    // survives a rescan and a new machine. A cache-only default would make this
    // feature invisible everywhere but the desktop that downloaded it.
    //
    // The safety is elsewhere and does not depend on this being off: an image
    // already in the folder is never replaced, the write is atomic, only
    // High-confidence matches are applied, and every bulk run can be undone.
    let store = state
        .db
        .lock()
        .get_setting("store_covers_in_folder")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(true);
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
///
/// Reports progress and can be cancelled, for the same reason the real run can:
/// over a whole library this is several hundred lookups and takes minutes. A
/// button that says "Looking…" for eight minutes with no way out is
/// indistinguishable from one that has hung.
#[tauri::command]
pub async fn preview_cover_downloads(
    scope: String,
    target: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<Vec<Plan>, String> {
    if running().swap(true, Ordering::SeqCst) {
        return Err("a cover run is already going".into());
    }
    cancel_flag().store(false, Ordering::SeqCst);

    let tracks = match tracks_for_scope(&state, &scope, &target) {
        Ok(t) => t,
        Err(e) => {
            running().store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    let reqs: Vec<CoverRequest> = tracks.iter().map(|t| request_for(t, false)).collect();
    let cancel = Arc::clone(cancel_flag());

    let out = tauri::async_runtime::spawn_blocking(move || {
        let opts = BulkOptions { dry_run: true, cancel, ..Default::default() };
        resolver().resolve_many(reqs, &opts, |p| {
            let _ = app.emit("cover-progress", p);
        })
    })
    .await;

    running().store(false, Ordering::SeqCst);
    out.map_err(|e| e.to_string())
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

    // Note: `running` is cleared inside the thread. If that thread ever unwinds
    // before reaching it, the flag would stay set and refuse every later run —
    // worth a scoped guard if this grows more early returns.

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

/// Game names to offer while someone is correcting a classification.
///
/// Three sources, in the order they are worth trusting:
///
/// 1. **The Libretro archive for that console.** These are No-Intro names —
///    canonical, and the exact strings the cover downloader will later try to
///    match. Picking one here means the artwork step cannot then fail to find
///    it. Cached after the first fetch, so this is usually local.
/// 2. **Names already in the library**, so a correction lands on the spelling
///    the rest of the collection uses instead of a near-duplicate.
/// 3. **Steam**, for everything the console archives have never heard of —
///    PC games, and anything released after the archive stopped caring.
///
/// Region tags are stripped from the Libretro names: `(USA)` and `(Europe)` are
/// two files for one game, and offering both as separate choices is offering a
/// decision that does not exist.
#[tauri::command]
pub async fn suggest_game_names(
    console_id: String,
    query: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, String> {
    let q = query.trim().to_lowercase();
    if q.len() < 2 {
        return Ok(Vec::new());
    }

    let library: Vec<String> = {
        let db = state.db.lock();
        db.get_all_tracks()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|t| (!t.game.is_empty()).then_some(t.game))
            .collect()
    };

    tauri::async_runtime::spawn_blocking(move || {
        let mut out: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = Default::default();
        let mut push = |name: String, out: &mut Vec<String>| {
            let key = name.to_lowercase();
            if seen.insert(key) {
                out.push(name);
            }
        };

        for g in library {
            if g.to_lowercase().contains(&q) {
                push(g, &mut out);
            }
        }

        let http = tunante_art::http::UreqHttp::default();
        if let Some(system) = tunante_core::console::by_id(&console_id).and_then(|c| c.libretro) {
            if let Ok(index) = tunante_art::archive::index_for(&http, system) {
                for e in &index.entries {
                    // The stem before the first region group, which is the game.
                    let base = e.file.split(" (").next().unwrap_or(&e.file).trim();
                    if base.to_lowercase().contains(&q) {
                        push(base.to_string(), &mut out);
                    }
                    if out.len() > 40 {
                        break;
                    }
                }
            }
        }

        // `suggest_names`, not the cover-matching sources.
        //
        // Those demand the name match, because what they choose gets written
        // into somebody's library — `nintendo()` will not answer "The Legend of
        // Zelda: Link's Awakening" to a folder called `Zelda Link's awakening
        // remake`, and it is right not to. Here a person reads the list and
        // picks, so the strictness belongs to them and the useful answer is
        // every title the catalogue thought was close.
        //
        // It also covers the gap the archives leave: Switch and PC have no
        // Libretro thumbnails at all, so for those this is the only source
        // there is.
        if out.len() < 8 {
            for name in tunante_art::sources::suggest_names(&http, &query) {
                push(name, &mut out);
                if out.len() >= 12 {
                    break;
                }
            }
        }

        out.sort_by_key(|n| {
            let l = n.to_lowercase();
            // Prefixes first, then the rest: typing "chrono" should not bury
            // "Chrono Trigger" under "Radical Dreamers - Le Trésor…".
            (if l.starts_with(&q) { 0 } else { 1 }, n.len(), l)
        });
        out.truncate(12);
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A proposed set of track names for a multi-subsong file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrackNames {
    /// The file every one of these belongs to.
    pub file: String,
    /// How many subsongs it actually holds.
    pub subsongs: usize,
    /// The names, in order. Empty when nothing usable was found.
    pub titles: Vec<String>,
    /// Their lengths as published, positionally aligned with `titles`.
    ///
    /// Carried separately rather than folded into the title so the caller can
    /// show them apart, and so the apply path keeps them: these formats loop,
    /// nothing in the file says how long a track is, and without a published
    /// length the reader emulates each subsong and watches for silence.
    pub lengths: Vec<String>,
    /// Why there is nothing, in words a person can act on.
    pub problem: Option<String>,
}

/// Look up the track names for the file a track lives in.
///
/// Only for the formats that pack a whole game into one file — GBS, NSF, HES,
/// KSS, AY, SAP. Everything else is one song per file and has its title
/// already; asking would be asking for nothing.
///
/// Fetches and counts, and refuses on any mismatch. It never writes: the caller
/// shows the list and decides, because position is the entire mapping and a
/// listing of the wrong length would rename every track to the wrong song.
#[tauri::command]
pub async fn suggest_track_names(
    track_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<TrackNames, String> {
    let (file, _) = tunante_core::vgm_path::parse_vgm_path(&track_path);
    let file = file.to_string();

    let (console_id, game) = {
        let db = state.db.lock();
        let t = db
            .get_track_by_path(&track_path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "track not in the library".to_string())?;
        (t.console_id.clone(), t.game.clone())
    };

    let none = |why: &str| TrackNames {
        file: file.clone(),
        subsongs: 0,
        titles: Vec::new(),
        lengths: Vec::new(),
        problem: Some(why.to_string()),
    };

    let Some(system) = tunante_core::console::by_id(&console_id).and_then(|c| c.zophar) else {
        return Ok(none(
            "This console's format holds one song per file, so there is no listing to fetch.",
        ));
    };
    if game.trim().is_empty() {
        return Ok(none("Name the game first — the listing is looked up by it."));
    }

    // How many songs the file really has, counted from the library rather than
    // by opening it again: the scan already expanded every subsong into a row,
    // and re-reading a GBS to count them costs an emulator boot.
    let subsongs = {
        let db = state.db.lock();
        db.get_all_tracks()
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|t| tunante_core::vgm_path::parse_vgm_path(&t.path).0 == file)
            .count()
    };

    let file_for_thread = file.clone();
    let game_for_thread = game.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if subsongs <= 1 {
            return Ok(TrackNames {
                file: file_for_thread,
                subsongs,
                titles: Vec::new(),
                lengths: Vec::new(),
                problem: Some("This file holds a single song.".into()),
            });
        }

        let http = tunante_art::http::UreqHttp::default();
        let entries = tunante_art::tracklist::fetch(&http, system, &game_for_thread);
        if entries.is_empty() {
            return Ok(TrackNames {
                file: file_for_thread,
                subsongs,
                titles: Vec::new(),
                lengths: Vec::new(),
                problem: Some(format!("No listing for \"{game_for_thread}\" in the archive.")),
            });
        }
        if !tunante_art::tracklist::matches_subsongs(&entries, subsongs) {
            return Ok(TrackNames {
                file: file_for_thread,
                subsongs,
                titles: Vec::new(),
                lengths: Vec::new(),
                problem: Some(format!(
                    "The archive lists {} tracks and this file has {subsongs}. \
                     Position is the whole mapping, so a different count is a different rip \
                     and applying it would name every track wrongly.",
                    entries.len()
                )),
            });
        }
        Ok(TrackNames {
            file: file_for_thread,
            subsongs,
            titles: entries.iter().map(|e| e.title.clone()).collect(),
            lengths: entries.iter().map(|e| e.length.clone()).collect(),
            problem: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Write the names as an `.m3u` beside the file, and restamp the library.
///
/// An `.m3u` rather than rows in a table: every player of these formats reads
/// one, this one included, so the names outlive Tunante and the reader needs no
/// new path. Never overwrites an existing playlist — one already there was put
/// there by somebody.
#[tauri::command]
pub async fn apply_track_names(
    file: String,
    titles: Vec<String>,
    lengths: Vec<String>,
    only_index: Option<usize>,
    replace: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<usize, String> {
    let path = std::path::PathBuf::from(&file);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "not a file".to_string())?;
    let m3u = path.with_extension("m3u");

    let entries: Vec<tunante_art::tracklist::Entry> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| tunante_art::tracklist::Entry {
            number: i as u32 + 1,
            title: t.clone(),
            length: lengths.get(i).cloned().unwrap_or_default(),
        })
        .collect();

    // Never silently. A playlist already there was put there by somebody —
    // possibly by a previous run of this, possibly by hand thirty years ago —
    // and replacing it is a decision, not a default.
    //
    // The name only, not the path: the full one is three lines of a dialog and
    // the reader already knows which folder they are in.
    if m3u.exists() && !replace {
        let shown = m3u.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        return Err(format!("A playlist called {shown} is already next to this file."));
    }
    // The whole listing, even when one track was asked for. An .m3u with a
    // single line in it would leave every other subsong worse off than before,
    // and the file describes the rip, not the selection.
    let body = tunante_art::tracklist::to_m3u(&name, &entries);
    std::fs::write(&m3u, body).map_err(|e| format!("could not write {}: {e}", m3u.display()))?;

    // Then read the file again through the ordinary path, which now finds the
    // playlist beside it. No second implementation of "what is this track
    // called" — the readers already prefer an .m3u title over everything else.
    let opts = crate::commands::library::scan_opts(&state);
    let read = tunante_codec::metadata::read_metadata_all_with_opts(&path, opts)
        .map_err(|e| format!("could not re-read {}: {e}", path.display()))?;

    let db = state.db.lock();
    let mut n = 0usize;
    for track in read {
        let idx = tunante_core::vgm_path::parse_vgm_path(&track.path).1;
        if only_index.is_some() && idx != only_index {
            continue;
        }
        if db.insert_track(&track).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}
