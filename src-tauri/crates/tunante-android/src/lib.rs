//! The Rust side of the Android app.
//!
//! Everything below the pixels: the library database, the scan, and playback.
//! Java owns the screen, the service and the media session; it never sees a
//! decoder, a queue or a SQL statement.
//!
//! # Why the surface is JSON
//!
//! Marshalling structs across JNI by hand is where afternoons go. `serde_json`
//! is already in the tree, the decoder helper already speaks JSON, and a few
//! thousand rows of it is nothing on a phone — the screen pages anyway. When
//! that stops being true, the fix is to page in Rust, not to hand-roll
//! `jobject` conversions.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jint, jlong};
use jni::JNIEnv;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use tunante_core::db::Database;

mod player;
use player::Player;

/// The player, for the life of the process.
///
/// The device sink inside it is opened once and never handed back: reopening it
/// between tracks is audible as a click, and on a phone it also risks losing the
/// route to whatever the audio is going out of.
static ENGINE: Mutex<Option<Player>> = Mutex::new(None);

/// The library database.
///
/// Separate from `ENGINE` on purpose: a scan holds this for as long as it runs,
/// and holding the player's lock for a minute would freeze playback with it.
static DB: Mutex<Option<Database>> = Mutex::new(None);

/// Hand an error back to Java as JSON rather than by throwing.
///
/// A JNI exception has to be checked for after every single call, and one that
/// goes unchecked turns the next JNI call into an abort. One shape of answer for
/// both outcomes is far harder to get wrong.
fn fail(message: impl std::fmt::Display) -> String {
    log::error!("{message}");
    serde_json::json!({ "ok": false, "error": message.to_string() }).to_string()
}

/// `ndk_context` asserts if it is initialised twice, and a JNI entry point can
/// be called again after an activity is recreated.
static CONTEXT_READY: OnceLock<()> = OnceLock::new();

macro_rules! with_engine {
    ($body:expr) => {{
        let mut guard = ENGINE.lock().unwrap();
        match guard.as_mut() {
            Some(engine) => {
                let _ = engine;
                $body(engine)
            }
            None => {
                log::error!("player call before nativeInit");
            }
        }
    }};
}

fn jstring_to_string(env: &mut JNIEnv, s: &JString) -> Result<String, String> {
    env.get_string(s)
        .map(|v| v.into())
        .map_err(|e| format!("reading a Java string: {e}"))
}

/// Set up logging, hand cpal the JavaVM and Context, and open the audio device.
///
/// `decoder_path` comes from Kotlin as `ApplicationInfo.nativeLibraryDir` plus
/// the helper's name. Rust cannot work it out on its own.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    context: JObject,
    decoder_path: JString,
) -> jboolean {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("tunante"),
    );

    CONTEXT_READY.get_or_init(|| {
        let vm = match env.get_java_vm() {
            Ok(vm) => vm,
            Err(e) => {
                log::error!("no JavaVM: {e}");
                return;
            }
        };
        let ctx = match env.new_global_ref(&context) {
            Ok(c) => c,
            Err(e) => {
                log::error!("no global ref to the Context: {e}");
                return;
            }
        };
        unsafe {
            ndk_context::initialize_android_context(
                vm.get_java_vm_pointer() as *mut c_void,
                ctx.as_raw() as *mut c_void,
            );
        }
        // The context has to outlive every cpal call, which means the process.
        // Dropping the global ref here would leave cpal holding a stale handle.
        std::mem::forget(ctx);
        log::info!("android context handed to cpal");
    });

    let decoder = match jstring_to_string(&mut env, &decoder_path) {
        Ok(p) => PathBuf::from(p),
        Err(e) => {
            log::error!("{e}");
            return 0;
        }
    };

    if !decoder.is_file() {
        log::error!("no decoder at {}", decoder.display());
        return 0;
    }
    // Told once, and from here on the helper client finds it on its own. A
    // second call is ignored, which is what we want when an activity is
    // recreated: the path has not changed and a track may be playing.
    tunante_helper::set_decoder_path(&decoder);

    match Player::new() {
        Ok(player) => {
            log::info!("audio device open, decoder at {}", decoder.display());
            *ENGINE.lock().unwrap() = Some(player);
            1
        }
        Err(e) => {
            log::error!("{e}");
            0
        }
    }
}

/// Open (and migrate) the library database under the app's private directory.
///
/// `dir` is `Context.getFilesDir()`. Not shared storage, deliberately: that is
/// behind FUSE, where SQLite's WAL sidecar files are a bad bet. This directory
/// is plain POSIX, needs no permission, and is wiped with the app — which is
/// correct for a cache of what was scanned.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeOpenDb<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    dir: JString,
) -> jni::objects::JString<'a> {
    let out = match jstring_to_string(&mut env, &dir) {
        Ok(d) => {
            let path = Path::new(&d).join("tunante-android.db");
            match Database::new(&path) {
                Ok(db) => {
                    let tracks = db.get_all_tracks().map(|t| t.len()).unwrap_or(0);
                    *DB.lock().unwrap() = Some(db);
                    log::info!("db at {} ({tracks} tracks)", path.display());
                    serde_json::json!({ "ok": true, "path": path.display().to_string(),
                                        "tracks": tracks })
                    .to_string()
                }
                Err(e) => fail(format!("opening {}: {e}", path.display())),
            }
        }
        Err(e) => fail(e),
    };
    env.new_string(out).expect("new_string")
}

/// Walk a folder, probe every audio file in it, and write what comes back.
///
/// Blocking, and long: call it from a thread, never from the main looper.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeScan<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    root: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let asked = jstring_to_string(&mut env, &root)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeScan before nativeOpenDb")?;

        // An empty root means "everything the library is built from", which is
        // what a Rescan button wants. Hardcoding one folder was only ever a
        // stopgap: where the music is, is the user's answer.
        let roots: Vec<String> = if asked.is_empty() {
            db.get_monitored_folders()
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|f| f.path)
                .collect()
        } else {
            vec![asked]
        };
        if roots.is_empty() {
            return Err("no folders to scan — add one first".into());
        }

        let started = Instant::now();
        let (mut files, mut added, mut failed, mut gone) = (0usize, 0usize, 0usize, 0usize);

        for r in &roots {
            // Prune before adding: a folder the user deleted should stop being
            // in the tree, and the tree is built from what was scanned rather
            // than from what is on disk right now.
            //
            // Per root, never across them: an SD card that is not mounted is a
            // root full of files that all look missing, and pruning the whole
            // library over it would be unrecoverable.
            gone += tunante_helper::scan::prune_missing(db, Path::new(r)).unwrap_or(0);

            let (mut t, mut f) = (0usize, 0usize);
            match tunante_helper::scan::scan_folder(db, Path::new(r), |p| {
                t = p.total;
                f = p.failed;
            }) {
                Ok(n) => added += n,
                // One unreadable root does not abandon the others.
                Err(e) => log::warn!("scanning {r}: {e}"),
            }
            files += t;
            failed += f;
        }

        let ms = started.elapsed().as_millis() as u64;
        let per = if files > 0 { ms as f64 / files as f64 } else { 0.0 };
        log::info!(
            "scanned {files} files in {ms} ms ({per:.1} ms/file) across {} roots, \
             {added} tracks, {failed} failed, {gone} forgotten",
            roots.len()
        );
        Ok(serde_json::json!({ "ok": true, "added": added, "files": files, "failed": failed,
                               "removed": gone, "ms": ms, "roots": roots.len() })
        .to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// One level of the library tree: the folders under `parent`, and the tracks
/// that sit in it.
///
/// An empty `parent` asks for the roots. The shape of the answer, and every
/// awkward case in it, lives in `tunante_core::tree` — where it can be tested
/// without a phone attached.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeBrowse<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    parent: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let parent = jstring_to_string(&mut env, &parent)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeBrowse before nativeOpenDb")?;
        let all = db.get_all_tracks().map_err(|e| e.to_string())?;

        let paths: Vec<String> = all.iter().map(|t| t.path.clone()).collect();
        let level = tunante_core::tree::level(&paths, &parent);

        // Back to whole tracks for the ones on this level: the screen wants
        // titles and durations, and the tree only deals in paths.
        let by_path: std::collections::HashMap<&str, &tunante_core::db::models::Track> =
            all.iter().map(|t| (t.path.as_str(), t)).collect();
        let here: Vec<_> = level.here.iter().filter_map(|p| by_path.get(p.as_str())).collect();

        let folders: Vec<_> = level
            .folders
            .iter()
            .map(|f| {
                serde_json::json!({ "path": f.path, "name": f.name, "count": f.count,
                                    "cover": f.first_track })
            })
            .collect();

        Ok(serde_json::json!({ "ok": true, "folders": folders, "tracks": here }).to_string())
    })();
    let out = out.unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Put back what was playing when the app last stopped, paused.
///
/// Called after the database is open. Deliberately lands paused: a phone that
/// starts making noise in a pocket because it was rebooted is worse than one
/// that forgot where it was.
///
/// The queue is rebuilt as the saved track's whole folder, which is the same
/// approximation `tunante-mini` makes — the queue itself is not persisted, only
/// the track.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeRestoreSession<'a>(
    env: JNIEnv<'a>,
    _class: JClass,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeRestoreSession before nativeOpenDb")?;
        let saved = tunante_core::Session::load(db);

        let mut engine = ENGINE.lock().unwrap();
        let engine = engine.as_mut().ok_or("nativeRestoreSession before nativeInit")?;
        engine.set_volume(saved.volume);
        engine.set_loop_settings(saved.loops, saved.fade_seconds * 1000);
        engine.set_shuffle(saved.shuffle);
        engine.set_repeat(match saved.repeat {
            1 => tunante_core::RepeatMode::All,
            2 => tunante_core::RepeatMode::One,
            _ => tunante_core::RepeatMode::Off,
        });

        let Some(path) = saved.track_path else {
            return Ok(serde_json::json!({ "ok": true, "restored": false }).to_string());
        };
        let file = path.split('#').next().unwrap_or(&path);
        let Some(folder) = Path::new(file).parent().map(|p| p.to_string_lossy().to_string()) else {
            return Ok(serde_json::json!({ "ok": true, "restored": false }).to_string());
        };
        let tracks = db.get_tracks_by_folder(&folder).map_err(|e| e.to_string())?;
        let Some(index) = tracks.iter().position(|t| t.path == path) else {
            // The file is gone, or was never rescanned. Silently not resuming is
            // right here: there is nothing to tell the user to do about it.
            return Ok(serde_json::json!({ "ok": true, "restored": false }).to_string());
        };

        // A track that will not open must not poison the launch.
        //
        // The saved track can stop being playable between one run and the next
        // — deleted, on a card that is out, or simply a format the decoder
        // cannot handle, which is not hypothetical: a Dolby m4a scans fine
        // through lofty and fails through symphonia (see docs/TODO-upstream.md).
        // Propagating that error meant every single launch tried it, failed,
        // and left the queue holding a track that could never play. Forever,
        // because nothing ever overwrote the saved position.
        if let Err(e) = engine.restore(tracks, index, saved.position_ms) {
            log::warn!("not resuming {path}: {e}");
            engine.stop();
            engine.set_tracks(Vec::new());
            return Ok(serde_json::json!({ "ok": true, "restored": false,
                                          "reason": e })
            .to_string());
        }
        log::info!("resumed {path} at {} ms, paused", saved.position_ms);
        Ok(serde_json::json!({ "ok": true, "restored": true }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Write the session out.
///
/// Called from the service tick every few seconds and again on `onPause`, not
/// only on exit: a phone app is killed by the system far more often than it is
/// closed by the user, and a resume that only works on a clean exit is a resume
/// that rarely works.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeSaveSession(
    _env: JNIEnv,
    _class: JClass,
) {
    let engine = ENGINE.lock().unwrap();
    let Some(engine) = engine.as_ref() else { return };
    let db = DB.lock().unwrap();
    let Some(db) = db.as_ref() else { return };

    tunante_core::Session::save(
        db,
        engine.current_path().as_deref(),
        engine.position_ms(),
        engine.volume(),
        engine.shuffle(),
        engine.repeat() as u8,
        engine.loops(),
        engine.fade_ms() / 1000,
    );
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeSetSleepTimer(
    _env: JNIEnv,
    _class: JClass,
    minutes: jint,
) {
    with_engine!(|e: &mut Player| {
        if minutes > 0 {
            e.start_sleep_timer(minutes as u64);
        } else {
            e.cancel_sleep_timer();
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeSetShuffle(
    _env: JNIEnv,
    _class: JClass,
    on: jboolean,
) {
    with_engine!(|e: &mut Player| e.set_shuffle(on != 0))
}

/// 0 off, 1 all, 2 one — the order `RepeatMode` already has.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeSetRepeat(
    _env: JNIEnv,
    _class: JClass,
    mode: jint,
) {
    with_engine!(|e: &mut Player| e.set_repeat(match mode {
        1 => tunante_core::RepeatMode::All,
        2 => tunante_core::RepeatMode::One,
        _ => tunante_core::RepeatMode::Off,
    }))
}

/// Put a track next in line, without touching what is playing.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeEnqueue<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    path: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let path = jstring_to_string(&mut env, &path)?;
        let track = {
            let db = DB.lock().unwrap();
            db.as_ref()
                .and_then(|db| db.get_track_by_path(&path).ok().flatten())
                .unwrap_or_else(|| bare_track(&path))
        };
        let title = track.title.clone();
        let mut guard = ENGINE.lock().unwrap();
        let engine = guard.as_mut().ok_or("nativeEnqueue before nativeInit")?;
        engine.enqueue(track);
        Ok(serde_json::json!({ "ok": true, "title": title }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Take a track out of a playlist, named by path.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeRemoveFromPlaylist<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    id: JString,
    path: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let id = jstring_to_string(&mut env, &id)?;
        let path = jstring_to_string(&mut env, &path)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeRemoveFromPlaylist before nativeOpenDb")?;
        let track = db
            .get_track_by_path(&path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no track at {path}"))?;
        db.remove_track_from_playlist(&id, &track.id).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// The folders the library is built from.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeRoots<'a>(
    env: JNIEnv<'a>,
    _class: JClass,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeRoots before nativeOpenDb")?;
        let folders = db.get_monitored_folders().map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true, "roots": folders }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Add a folder to scan, or take one away.
///
/// Removing a root forgets its tracks too. Leaving them would put rows in the
/// library that no scan will ever refresh or prune, which is worse than losing
/// them: they cannot be got rid of at all.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeSetRoot<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    path: JString,
    add: jboolean,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let path = jstring_to_string(&mut env, &path)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeSetRoot before nativeOpenDb")?;

        if add != 0 {
            if !Path::new(&path).is_dir() {
                return Err(format!("not a folder: {path}"));
            }
            let id = format!("{:x}", md5ish(&path));
            db.add_monitored_folder(&id, &path).map_err(|e| e.to_string())?;
        } else {
            let existing = db.get_monitored_folders().map_err(|e| e.to_string())?;
            let Some(f) = existing.into_iter().find(|f| f.path == path) else {
                return Err(format!("not a root: {path}"));
            };
            db.remove_monitored_folder(&f.id).map_err(|e| e.to_string())?;
            db.remove_tracks_by_folder_path(&path).map_err(|e| e.to_string())?;
        }
        Ok(serde_json::json!({ "ok": true }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// A stable id for a path, without pulling in a hash crate for it.
fn md5ish(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// The directories directly inside `path`, for the folder picker.
///
/// Straight off the disk rather than out of the library: the whole point is to
/// choose somewhere that has not been scanned yet.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeListDirs<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    path: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let path = jstring_to_string(&mut env, &path)?;
        let path = if path.is_empty() { "/storage/emulated/0".to_string() } else { path };
        let mut dirs: Vec<String> = std::fs::read_dir(&path)
            .map_err(|e| format!("reading {path}: {e}"))?
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            // Dotfiles are configuration, not music.
            .filter(|n| !n.starts_with('.'))
            .collect();
        dirs.sort_by_key(|n| n.to_lowercase());

        let parent = Path::new(&path).parent().map(|p| p.to_string_lossy().to_string());
        Ok(serde_json::json!({ "ok": true, "here": path, "parent": parent, "dirs": dirs })
            .to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// One row per folder that directly holds music.
///
/// An album, in practice: this collection puts one game per directory. An index
/// over the same rows the tree shows, for when you know the name and do not want
/// to walk down to it.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeAlbums<'a>(
    env: JNIEnv<'a>,
    _class: JClass,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeAlbums before nativeOpenDb")?;
        let all = db.get_all_tracks().map_err(|e| e.to_string())?;

        let mut by_dir: std::collections::BTreeMap<String, (usize, String)> = Default::default();
        for t in &all {
            let file = t.path.split('#').next().unwrap_or(&t.path);
            let Some(dir) = Path::new(file).parent().map(|p| p.to_string_lossy().to_string())
            else {
                continue;
            };
            let e = by_dir.entry(dir).or_insert((0, t.path.clone()));
            e.0 += 1;
        }

        let folders: Vec<_> = by_dir
            .into_iter()
            .map(|(path, (count, cover))| {
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                serde_json::json!({ "path": path, "name": name, "count": count, "cover": cover })
            })
            .collect();
        Ok(serde_json::json!({ "ok": true, "folders": folders, "tracks": [] }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// One row per game, from the album tag.
///
/// Empty `game` lists them; naming one lists its tracks. Not the same index as
/// the albums tab: that one is the disk's opinion (a directory that holds
/// music) and this is the tags'. They differ for a rip split across discs, for
/// a folder holding several games, and for anything tagged but filed loose.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeGames<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    game: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let want = jstring_to_string(&mut env, &game)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeGames before nativeOpenDb")?;
        let all = db.get_all_tracks().map_err(|e| e.to_string())?;

        if want.is_empty() {
            let folders: Vec<_> = tunante_core::games::index(&all)
                .into_iter()
                .map(|g| {
                    serde_json::json!({ "path": g.name, "name": g.name, "count": g.count,
                                        "cover": g.first_track, "by": g.by })
                })
                .collect();
            return Ok(
                serde_json::json!({ "ok": true, "folders": folders, "tracks": [] }).to_string()
            );
        }

        let tracks = tunante_core::games::tracks_of(&all, &want);
        Ok(serde_json::json!({ "ok": true, "folders": [], "tracks": tracks }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// One row per console, from the format of the files.
///
/// Three levels, the same as tunante-mini's: empty lists the consoles, a
/// console name lists its games, and `console\u{1}/dir` lists that game's
/// tracks. The middle level is what makes the tab usable at all -- a console
/// with a real collection under it is thousands of tracks, and a flat list of
/// them is not something anybody scrolls.
///
/// The console has to be in the key of the third level rather than the folder
/// alone: a directory holding both .spc rips and mp3s appears under two
/// consoles, and only the pair says which of the two was opened.
///
/// The mapping is `tunante_core::console`, shared with tunante-mini so the two
/// group the library the same way.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeConsoles<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    console: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let want = jstring_to_string(&mut env, &console)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeConsoles before nativeOpenDb")?;
        let all = db.get_all_tracks().map_err(|e| e.to_string())?;

        if want.is_empty() {
            let mut by_console: std::collections::BTreeMap<&str, (usize, String)> =
                Default::default();
            for t in &all {
                let c = tunante_core::console::console_of(&t.path);
                let e = by_console.entry(c).or_insert((0, t.path.clone()));
                e.0 += 1;
            }
            let folders: Vec<_> = by_console
                .into_iter()
                .map(|(name, (count, cover))| {
                    serde_json::json!({ "path": name, "name": name, "count": count,
                                        "cover": cover })
                })
                .collect();
            return Ok(
                serde_json::json!({ "ok": true, "folders": folders, "tracks": [] }).to_string()
            );
        }

        // Third level: one game of one console.
        if let Some((console, dir)) = want.split_once('\u{1}') {
            let prefix = format!("{}/", dir.trim_end_matches('/'));
            let tracks: Vec<_> = all
                .iter()
                .filter(|t| tunante_core::console::console_of(&t.path) == console)
                .filter(|t| {
                    // On the real file: a subsong's `#n` suffix does not change
                    // which directory it lives in. Direct children only, so a
                    // game does not swallow the one filed inside it.
                    let file = t.path.split('#').next().unwrap_or(&t.path);
                    file.strip_prefix(prefix.as_str())
                        .is_some_and(|rest| !rest.contains('/'))
                })
                .collect();
            return Ok(
                serde_json::json!({ "ok": true, "folders": [], "tracks": tracks }).to_string()
            );
        }

        // Second level: the games of one console, which are its directories.
        let mut by_dir: std::collections::BTreeMap<String, (usize, String)> = Default::default();
        for t in &all {
            if tunante_core::console::console_of(&t.path) != want {
                continue;
            }
            let file = t.path.split('#').next().unwrap_or(&t.path);
            let Some(dir) = Path::new(file).parent().map(|p| p.to_string_lossy().to_string())
            else {
                continue;
            };
            let e = by_dir.entry(dir).or_insert((0, t.path.clone()));
            e.0 += 1;
        }
        let folders: Vec<_> = by_dir
            .into_iter()
            .map(|(path, (count, cover))| {
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                serde_json::json!({ "path": path, "name": name, "count": count, "cover": cover })
            })
            .collect();
        Ok(serde_json::json!({ "ok": true, "folders": folders, "tracks": [] }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Empty the waiting list, leaving what is playing alone.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeClearQueue(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| e.clear_user_queue())
}

/// Cycle how many times a looping track plays: 1, 2, 3, then forever.
///
/// The same four steps `tunante-mini` offers, stored under the same settings
/// key, so a shared library does not change how it sounds when you change which
/// program is reading it.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeCycleLoops(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| {
        let next = match e.loops() {
            1 => 2,
            2 => 3,
            3 => 0,
            _ => 1,
        };
        e.set_loop_settings(next, e.fade_ms());
    })
}

/// Cycle the fade at the end of a looping track: none, 4, 8, 15 seconds.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeCycleFade(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| {
        let next = match e.fade_ms() / 1000 {
            0 => 4,
            4 => 8,
            8 => 15,
            _ => 0,
        };
        e.set_loop_settings(e.loops(), next * 1000);
    })
}

/// Everything waiting, in order.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeQueue<'a>(
    env: JNIEnv<'a>,
    _class: JClass,
) -> jni::objects::JString<'a> {
    let out = match ENGINE.lock().unwrap().as_ref() {
        Some(e) => serde_json::json!({ "ok": true, "tracks": e.user_queue() }).to_string(),
        None => fail("nativeQueue before nativeInit"),
    };
    env.new_string(out).expect("new_string")
}

/// Play something that was waiting, now.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePlayQueued<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    path: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let path = jstring_to_string(&mut env, &path)?;
        let mut guard = ENGINE.lock().unwrap();
        let engine = guard.as_mut().ok_or("nativePlayQueued before nativeInit")?;
        let id = engine
            .user_queue()
            .iter()
            .find(|t| t.path == path)
            .map(|t| t.id.clone())
            .ok_or_else(|| format!("{path} is not waiting"))?;
        engine.play_queued(&id)?;
        Ok(engine.state().to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Take one track out of the waiting list, by path.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeDequeue(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    let Ok(path) = jstring_to_string(&mut env, &path) else { return };
    let mut guard = ENGINE.lock().unwrap();
    let Some(engine) = guard.as_mut() else { return };
    // By path, like everything else the screen hands back; the id is a UUID
    // nobody sees. A track queued twice loses the copy that matched first,
    // which is the one the row showed.
    let Some(id) = engine.user_queue().iter().find(|t| t.path == path).map(|t| t.id.clone())
    else {
        return;
    };
    engine.dequeue(&id);
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeMoveInQueue(
    _env: JNIEnv,
    _class: JClass,
    from: jint,
    to: jint,
) {
    with_engine!(|e: &mut Player| e.move_in_queue(from.max(0) as usize, to.max(0) as usize))
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeRenamePlaylist<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    id: JString,
    name: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let id = jstring_to_string(&mut env, &id)?;
        let name = jstring_to_string(&mut env, &name)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("a playlist needs a name".into());
        }
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeRenamePlaylist before nativeOpenDb")?;
        db.rename_playlist(&id, name).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Store the playlists in this order.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeReorderPlaylists<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    ids_json: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let raw = jstring_to_string(&mut env, &ids_json)?;
        let ids: Vec<String> =
            serde_json::from_str(&raw).map_err(|e| format!("the id list was not JSON: {e}"))?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeReorderPlaylists before nativeOpenDb")?;
        db.reorder_playlists(&ids).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Put a whole playlist in the waiting list, in its stored order.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeEnqueuePlaylist<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    id: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let id = jstring_to_string(&mut env, &id)?;
        let tracks = {
            let guard = DB.lock().unwrap();
            let db = guard.as_ref().ok_or("nativeEnqueuePlaylist before nativeOpenDb")?;
            db.get_playlist_tracks(&id).map_err(|e| e.to_string())?
        };
        let n = tracks.len();
        let mut guard = ENGINE.lock().unwrap();
        let engine = guard.as_mut().ok_or("nativeEnqueuePlaylist before nativeInit")?;
        for t in tracks {
            engine.enqueue(t);
        }
        Ok(serde_json::json!({ "ok": true, "added": n }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Every playlist, with its track count.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePlaylists<'a>(
    env: JNIEnv<'a>,
    _class: JClass,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativePlaylists before nativeOpenDb")?;
        let lists = db.get_playlists().map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true, "playlists": lists }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// The tracks in a playlist, in their stored order.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePlaylistTracks<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    id: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let id = jstring_to_string(&mut env, &id)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativePlaylistTracks before nativeOpenDb")?;
        let tracks = db.get_playlist_tracks(&id).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true, "tracks": tracks }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Make a playlist and hand back its id.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeCreatePlaylist<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    name: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let name = jstring_to_string(&mut env, &name)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("a playlist needs a name".into());
        }
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeCreatePlaylist before nativeOpenDb")?;
        let id = db.create_playlist_named(name).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true, "id": id }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeDeletePlaylist<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    id: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let id = jstring_to_string(&mut env, &id)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeDeletePlaylist before nativeOpenDb")?;
        db.delete_playlist(&id).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Append tracks, named by path, to a playlist.
///
/// By path rather than by id because that is what the screen has: the library
/// list is built from paths, and a track's id is a UUID the user never sees.
/// Files the library has never been told about are skipped rather than invented
/// — a playlist entry pointing at nothing is worse than a missing one.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeAddToPlaylist<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    id: JString,
    paths_json: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let id = jstring_to_string(&mut env, &id)?;
        let raw = jstring_to_string(&mut env, &paths_json)?;
        let paths: Vec<String> =
            serde_json::from_str(&raw).map_err(|e| format!("the path list was not JSON: {e}"))?;

        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeAddToPlaylist before nativeOpenDb")?;
        let ids: Vec<String> = paths
            .iter()
            .filter_map(|p| db.get_track_by_path(p).ok().flatten().map(|t| t.id))
            .collect();
        let skipped = paths.len() - ids.len();
        let added = db.add_tracks_to_playlist(&id, &ids).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true, "added": added, "skipped": skipped }).to_string())
    })()
    .unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Cover art for a track, as a `data:` URI, or empty if there is none.
///
/// Two sources, in order: the art embedded in the file's own tags, which costs
/// a decoder process, and then a `cover.jpg` sitting beside it, which costs a
/// `read_dir`. Most console rips have the second and not the first.
///
/// Blocking — it can spawn a helper — so call it off the main looper. Java
/// caches the result; nothing is cached here, because the only sensible cache
/// key is the one the screen already has.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeArtwork<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    path: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let path = jstring_to_string(&mut env, &path)?;
        // The subsong suffix is part of the track's identity, not of any file
        // name, and neither the helper nor read_dir wants to see it.
        let real = path.split('#').next().unwrap_or(&path).to_string();

        if let Some(uri) = tunante_helper::artwork(Path::new(&real), Duration::from_secs(5)) {
            if !uri.is_empty() {
                return Ok(uri);
            }
        }

        let dir = Path::new(&real).parent().ok_or("no parent directory")?;
        let Some(image) = tunante_helper::art::folder_image(dir) else {
            return Ok(String::new());
        };
        let bytes = std::fs::read(&image).map_err(|e| format!("reading {}: {e}", image.display()))?;
        let mime = match image
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            Some("bmp") => "image/bmp",
            // jpg, jpeg, and anything folder_image let through.
            _ => "image/jpeg",
        };
        Ok(format!("data:{mime};base64,{}", B64.encode(bytes)))
    })();
    // An empty string rather than an error object: "this track has no cover" is
    // the common case, not a failure, and the caller should not have to parse
    // JSON to learn it.
    let out = out.unwrap_or_else(|e| {
        log::warn!("artwork: {e}");
        String::new()
    });
    env.new_string(out).expect("new_string")
}

/// Tracks matching `query`, across the whole library.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeSearch<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    query: JString,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let q = jstring_to_string(&mut env, &query)?;
        let guard = DB.lock().unwrap();
        let db = guard.as_ref().ok_or("nativeSearch before nativeOpenDb")?;
        let tracks = db.search_tracks(&q).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true, "tracks": tracks }).to_string())
    })();
    let out = out.unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Load a folder's tracks as the queue and start at `index`.
///
/// Empty `folder` means the whole library, which is what a "play everything"
/// button wants.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePlayFolder<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    folder: JString,
    index: jint,
) -> jni::objects::JString<'a> {
    let out = (|| {
        let f = jstring_to_string(&mut env, &folder)?;
        let tracks = {
            let guard = DB.lock().unwrap();
            let db = guard.as_ref().ok_or("nativePlayFolder before nativeOpenDb")?;
            if f.is_empty() { db.get_all_tracks() } else { db.get_tracks_by_folder(&f) }
                .map_err(|e| e.to_string())?
        };
        if tracks.is_empty() {
            return Err(format!("no tracks under {f:?}"));
        }
        let mut guard = ENGINE.lock().unwrap();
        let engine = guard.as_mut().ok_or("nativePlayFolder before nativeInit")?;
        engine.set_tracks(tracks);
        engine.play_index(index.max(0) as usize)?;
        Ok(engine.state().to_string())
    })();
    let out = out.unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Queue exactly these paths, in this order, and start at `index`.
///
/// The general primitive, and the one the browser uses. `nativePlayFolder`
/// cannot serve it: `get_tracks_by_folder` matches `path LIKE 'folder/%'`, so it
/// also returns everything in the subfolders, and its indices would not line up
/// with a list showing only what is directly in the folder.
///
/// A JSON array rather than a `String[]`: same reason as everything else here —
/// one marshalling story, not two.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePlayList<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    paths_json: JString,
    index: jint,
) -> jni::objects::JString<'a> {
    let out = (|| -> Result<String, String> {
        let raw = jstring_to_string(&mut env, &paths_json)?;
        let paths: Vec<String> =
            serde_json::from_str(&raw).map_err(|e| format!("the path list was not JSON: {e}"))?;
        if paths.is_empty() {
            return Err("an empty queue".into());
        }

        let tracks: Vec<_> = {
            let db = DB.lock().unwrap();
            paths
                .iter()
                .map(|p| {
                    db.as_ref()
                        .and_then(|db| db.get_track_by_path(p).ok().flatten())
                        .unwrap_or_else(|| bare_track(p))
                })
                .collect()
        };

        let mut guard = ENGINE.lock().unwrap();
        let engine = guard.as_mut().ok_or("nativePlayList before nativeInit")?;
        engine.set_tracks(tracks);
        engine.play_index((index.max(0) as usize).min(paths.len() - 1))?;
        Ok(engine.state().to_string())
    })();
    let out = out.unwrap_or_else(fail);
    env.new_string(out).expect("new_string")
}

/// Play one file directly, with a queue of just it.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePlay(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jboolean {
    let Ok(path) = jstring_to_string(&mut env, &path) else {
        return 0;
    };
    let mut guard = ENGINE.lock().unwrap();
    let Some(engine) = guard.as_mut() else {
        log::error!("nativePlay before nativeInit");
        return 0;
    };

    // A one-track queue, so the media session and the notification have
    // something to name even when nothing was scanned.
    let track = {
        let db = DB.lock().unwrap();
        db.as_ref()
            .and_then(|db| db.get_track_by_path(&path).ok().flatten())
            .unwrap_or_else(|| bare_track(&path))
    };
    engine.set_tracks(vec![track]);
    match engine.play_index(0) {
        Ok(()) => {
            log::info!("playing {path}");
            1
        }
        Err(e) => {
            log::error!("{e}");
            0
        }
    }
}

/// A `Track` for a file the library has never seen.
///
/// The decoder would tell us the real metadata, but that is another process
/// spawn on the way to pressing play; the file name is enough for a
/// notification, and the scan fills the rest in properly.
fn bare_track(path: &str) -> tunante_core::db::models::Track {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    // Written out rather than `..Default::default()` on purpose: Track has no
    // Default, and if a field is ever added the compiler says so here instead of
    // quietly filling it with a zero that means something.
    tunante_core::db::models::Track {
        id: String::new(),
        path: path.to_string(),
        title: name,
        artist: String::new(),
        album: String::new(),
        album_artist: String::new(),
        track_number: None,
        disc_number: None,
        duration_ms: 0,
        sample_rate: None,
        channels: None,
        bitrate: None,
        codec: String::new(),
        file_size: 0,
        has_artwork: false,
        rating: 0,
        modified_at: 0,
    }
}


#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeTogglePlay(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| e.toggle_play())
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePause(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| e.pause())
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeResume(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| e.resume())
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeNext(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| {
        e.next();
    })
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativePrev(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| {
        e.prev();
    })
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeSeek(
    _env: JNIEnv,
    _class: JClass,
    ms: jlong,
) {
    with_engine!(|e: &mut Player| e.seek(ms.max(0) as u64))
}

#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeStop(
    _env: JNIEnv,
    _class: JClass,
) {
    with_engine!(|e: &mut Player| e.stop())
}

/// The heartbeat, called by the foreground service.
///
/// This is the clock that in `tunante-mini` lives in a `slint::Timer` on the UI
/// thread — which is exactly why it stops there when the window does. Driving it
/// from the service is the whole reason the queue keeps advancing with the
/// screen off.
///
/// Returns the state, with `trackChanged` set on the tick where the queue moved
/// on, so the caller can refresh the notification then rather than twice a
/// second forever.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeTick<'a>(
    env: JNIEnv<'a>,
    _class: JClass,
) -> jni::objects::JString<'a> {
    let out = match ENGINE.lock().unwrap().as_mut() {
        Some(engine) => {
            let changed = engine.tick();
            let mut state = engine.state();
            state["trackChanged"] = serde_json::Value::Bool(changed);
            state.to_string()
        }
        None => fail("nativeTick before nativeInit"),
    };
    env.new_string(out).expect("new_string")
}

/// The current state, without ticking the clock.
#[no_mangle]
pub extern "system" fn Java_com_tunante_android_NativeBridge_nativeState<'a>(
    env: JNIEnv<'a>,
    _class: JClass,
) -> jni::objects::JString<'a> {
    let out = match ENGINE.lock().unwrap().as_ref() {
        Some(engine) => engine.state().to_string(),
        None => fail("nativeState before nativeInit"),
    };
    env.new_string(out).expect("new_string")
}
