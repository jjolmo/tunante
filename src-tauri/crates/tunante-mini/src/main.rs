//! tunante-mini — Tunante for the phone.
//!
//! Native Slint, no webview. The emulator cores are not linked into this program
//! at all: they live in the `tunante-decoder` helper, spawned per track and per
//! scanned file, so the tens of megabytes a console core allocates never land
//! here. Measured, one process per backend: an NDS core costs ~43 MB while it
//! plays and nothing a moment later.
//!
//! Three tabs, and a switcher that sits along the bottom edge in portrait and
//! moves to a side rail when the phone is turned.
//!
//! # Running it
//!
//! ```text
//! tunante-mini                     open the library it already knows
//! tunante-mini <fichero>           play that file, queueing its folder
//! tunante-mini --scan <carpeta>    scan a folder into the library first
//! ```
//!
//! # Instruments
//!
//! Two more flags exist, and they are not features — they are how the app gets
//! measured and driven from a shell when nobody can put a finger on the glass:
//!
//! ```text
//! tunante-mini --rows N            fake rows, to measure what the list costs
//! tunante-mini --focus-search      start on Library with the search focused
//! ```
//!
//! `--rows` fills the library tab with generated entries at a size no real
//! collection reaches; the real library never materialises every row. Read
//! **PSS** from `/proc/<pid>/smaps_rollup`, not RSS — RSS counts shared library
//! pages the session already has resident and overstates this several times.
//!
//! `--focus-search` exists because a compositor only raises the on-screen
//! keyboard when the last input came from touch, so focusing the field from a
//! shell proves the request is sent but not that the keyboard appears.

mod boost;
mod decoder;
mod library;
mod mpris;
mod picker;
mod player;
mod session;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use slint::{Model, ModelRc, SharedString, VecModel};

use tunante_core::db::Database;

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Before anything else, and on this thread: `main` is where the Slint event
    // loop runs, and the clamp is per-thread. See boost.rs for the measurements
    // — this is the difference between 68 fps and 112 on the phone.
    if !boost::ask_for_ui_clock() {
        eprintln!("uclamp no disponible: la interfaz irá a la frecuencia que elija el gobernador");
    }

    let args: Vec<String> = std::env::args().collect();
    let arg_value = |name: &str| -> Option<String> {
        args.iter().skip_while(|a| *a != name).nth(1).cloned()
    };

    let fake_rows = arg_value("--rows").and_then(|s| s.parse::<usize>().ok());
    let scan_target = arg_value("--scan").map(PathBuf::from);
    let focus_search = args.iter().any(|a| a == "--focus-search");

    // A bare path means "play this". The .desktop file declares MIME types, so
    // a file manager or another app can hand us a track directly, and that has
    // to do the obvious thing.
    let open_target = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .filter(|p| p.is_file());

    let dbfile = db_path()?;
    let db = Database::new(&dbfile)?;

    if let Some(folder) = &scan_target {
        eprintln!("escaneando {}…", folder.display());
        let id = format!("{:x}", folder.to_string_lossy().len());
        let _ = db.add_monitored_folder(&id, &folder.to_string_lossy());

        let mut last_shown = 0usize;
        let added = library::scan_folder(&db, folder, |p| {
            // One line per 25 files: enough to see it moving, few enough that
            // the terminal is not the bottleneck on a collection of thousands.
            if p.scanned - last_shown >= 25 || p.scanned == p.total {
                last_shown = p.scanned;
                eprintln!(
                    "  {}/{}  añadidas {}  fallidas {}  — {}",
                    p.scanned, p.total, p.added, p.failed, p.current
                );
            }
        })?;
        eprintln!("escaneo terminado: {added} pistas");
    }

    let ui = AppWindow::new()?;

    if focus_search {
        ui.set_autofocus_search(true);
        ui.set_tab(2);
    }

    // --- First run: choose the folders --------------------------------------
    //
    // With nothing monitored there is no library to show, so the picker replaces
    // the whole shell rather than sitting behind three empty tabs.
    let picker = Rc::new(RefCell::new(picker::Picker::new()));
    let picker_model = Rc::new(VecModel::from(Vec::<FolderEntry>::new()));
    ui.set_picker_entries(ModelRc::from(picker_model.clone()));

    let refresh_picker = {
        let (picker, picker_model) = (picker.clone(), picker_model.clone());
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let p = picker.borrow();
            picker_model.set_vec(
                p.entries()
                    .into_iter()
                    .map(|e| FolderEntry {
                        name: SharedString::from(e.name),
                        path: SharedString::from(e.path.to_string_lossy().to_string()),
                        chosen: e.chosen,
                    })
                    .collect::<Vec<_>>(),
            );
            ui.set_picker_path(SharedString::from(p.cwd.to_string_lossy().to_string()));
            ui.set_picker_chosen(p.chosen.len() as i32);
        }
    };
    let refresh_picker = Rc::new(refresh_picker);

    // The library tab, either from the real database or from generated rows.
    let roots: Vec<PathBuf> = db
        .get_monitored_folders()?
        .into_iter()
        .map(|f| PathBuf::from(f.path))
        .collect();

    let first_run = roots.is_empty();
    let tree = Rc::new(RefCell::new(library::Tree::new(roots)));
    let rows_model = Rc::new(VecModel::from(Vec::<LibraryRow>::new()));

    let db = Rc::new(db);

    if let Some(n) = fake_rows {
        rows_model.set_vec(generated_rows(n));
    } else {
        rows_model.set_vec(to_ui_rows(&tree.borrow().rows(&db)));
    }

    ui.set_library_total(rows_model.row_count() as i32);
    ui.set_library_rows(ModelRc::from(rows_model.clone()));

    // Nothing monitored and nothing generated means a first run.
    if first_run && fake_rows.is_none() {
        ui.set_setup_mode(true);
        refresh_picker();
    }

    {
        let (picker, refresh) = (picker.clone(), refresh_picker.clone());
        let model = picker_model.clone();
        ui.on_picker_enter(move |i| {
            if let Some(e) = model.row_data(i as usize) {
                picker.borrow_mut().enter(std::path::Path::new(&e.path.to_string()));
                refresh();
            }
        });
    }
    {
        let (picker, refresh) = (picker.clone(), refresh_picker.clone());
        let model = picker_model.clone();
        ui.on_picker_toggle(move |i| {
            if let Some(e) = model.row_data(i as usize) {
                picker.borrow_mut().toggle(std::path::Path::new(&e.path.to_string()));
                refresh();
            }
        });
    }
    {
        let (picker, refresh) = (picker.clone(), refresh_picker.clone());
        ui.on_picker_up(move || {
            picker.borrow_mut().up();
            refresh();
        });
    }

    // Progress from the scanning thread. `None` means it finished.
    let (scan_tx, scan_rx) = std::sync::mpsc::channel::<Option<String>>();

    {
        let picker = picker.clone();
        let db = db.clone();
        let scan_tx = scan_tx.clone();
        let dbfile = dbfile.clone();
        let weak = ui.as_weak();

        ui.on_picker_done(move || {
            let Some(ui) = weak.upgrade() else { return };
            let chosen: Vec<PathBuf> = picker.borrow().chosen.iter().cloned().collect();
            if chosen.is_empty() {
                return;
            }

            for (i, folder) in chosen.iter().enumerate() {
                let _ = db.add_monitored_folder(
                    &format!("root-{i}"),
                    &folder.to_string_lossy(),
                );
            }

            ui.set_setup_mode(false);
            ui.set_scan_status("Analizando…".into());

            // Its own connection on its own thread. SQLite is in WAL mode, so a
            // second writer is fine, and the alternative — scanning on the UI
            // thread — freezes the app for the length of a real collection.
            let (tx, dbfile) = (scan_tx.clone(), dbfile.clone());
            std::thread::spawn(move || {
                let Ok(db) = Database::new(&dbfile) else {
                    let _ = tx.send(None);
                    return;
                };
                for folder in chosen {
                    let _ = library::scan_folder(&db, &folder, |p| {
                        let _ = tx.send(Some(format!(
                            "Analizando {}/{}\n{} pistas encontradas",
                            p.scanned, p.total, p.added
                        )));
                    });
                }
                let _ = tx.send(None);
            });
        });
    }

    // The player owns the audio output for the whole session: one ALSA client,
    // never handed back, because re-taking the device on a track change is what
    // makes the gap between tracks audible.
    let player = Rc::new(RefCell::new(player::Player::new().ok()));
    if player.borrow().is_none() {
        eprintln!("aviso: no hay salida de audio; la interfaz funciona, el sonido no");
    }

    let queue_model = Rc::new(VecModel::from(Vec::<QueueRow>::new()));
    ui.set_queue_rows(ModelRc::from(queue_model.clone()));

    // --- Restore the last session -------------------------------------------
    let saved = session::Session::load(&db);
    ui.set_volume(saved.volume);
    ui.set_shuffle(saved.shuffle);
    ui.set_repeat(saved.repeat as i32);
    ui.set_loop_count(
        db.get_setting("mini.loop_count")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2),
    );
    ui.set_fade_seconds(
        db.get_setting("mini.fade_seconds")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8),
    );
    ui.set_library_summary(SharedString::from(format!(
        "{}",
        db.get_monitored_folders().map(|f| f.len()).unwrap_or(0)
    )));

    if let Some(p) = player.borrow_mut().as_mut() {
        p.set_volume(saved.volume);
        p.set_loop_settings(
            ui.get_loop_count().max(1) as u32,
            ui.get_fade_seconds() as u64 * 1000,
        );
        p.set_shuffle(saved.shuffle);
        p.set_repeat(match saved.repeat {
            1 => tunante_core::RepeatMode::All,
            2 => tunante_core::RepeatMode::One,
            _ => tunante_core::RepeatMode::Off,
        });
    }

    // Restore where you were, paused. Resuming *playing* on launch would be a
    // phone suddenly making noise in a pocket, which is never what was meant.
    if open_target.is_none() {
        if let Some(path) = saved.track_path.clone() {
            let folder = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let tracks = db.get_tracks_by_folder(&folder).unwrap_or_default();
            if let Some(start) = tracks.iter().position(|t| t.path == path) {
                if let Some(p) = player.borrow_mut().as_mut() {
                    p.set_tracks(tracks.clone());
                    if p.play_index(start).is_ok() {
                        p.toggle_play();               // straight to paused
                        p.seek(saved.position_ms);
                        push_now_playing(&ui, p);
                    }
                }
                queue_model.set_vec(to_queue_rows(&tracks, Some(start)));
            }
        }
    }

    // Handed a file on the command line: play it, and queue its folder around
    // it, which is what anyone opening one track of an album expects.
    if let Some(path) = &open_target {
        let path = path.to_string_lossy().to_string();
        let folder = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut tracks = db.get_tracks_by_folder(&folder).unwrap_or_default();
        // Not in the library yet — ask the decoder about it directly, so the
        // app can play a file it has never scanned.
        if tracks.is_empty() {
            if let Ok(values) = decoder::probe(
                std::path::Path::new(&path),
                std::time::Duration::from_secs(20),
                false,
            ) {
                tracks = values
                    .into_iter()
                    .filter_map(|v| serde_json::from_value(v).ok())
                    .collect();
            }
        }

        let start = tracks.iter().position(|t| t.path == path).unwrap_or(0);
        if let Some(p) = player.borrow_mut().as_mut() {
            p.set_tracks(tracks.clone());
            match p.play_index(start) {
                Ok(()) => push_now_playing(&ui, p),
                Err(e) => eprintln!("no se pudo reproducir: {e}"),
            }
        }
        queue_model.set_vec(to_queue_rows(&tracks, Some(start)));
        ui.set_setup_mode(false);
    }

    // --- Library: open a folder, or play a track -----------------------------
    {
        let (tree, db, rows_model, player, queue_model) =
            (tree.clone(), db.clone(), rows_model.clone(), player.clone(), queue_model.clone());
        let weak = ui.as_weak();

        ui.on_library_activated(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(row) = rows_model.row_data(index as usize) else { return };
            let path = row.path.to_string();

            if row.is_folder {
                tree.borrow_mut().toggle(&path);
                let rows = tree.borrow().rows(&db);
                rows_model.set_vec(to_ui_rows(&rows));
                ui.set_library_total(rows.len() as i32);
                return;
            }

            // Playing a track makes its folder the queue, which is what anyone
            // expects: tapping one song from an album queues the album.
            let folder = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let tracks = db.get_tracks_by_folder(&folder).unwrap_or_default();
            let start = tracks.iter().position(|t| t.path == path).unwrap_or(0);

            if let Some(p) = player.borrow_mut().as_mut() {
                p.set_tracks(tracks.clone());
                if let Err(e) = p.play_index(start) {
                    eprintln!("no se pudo reproducir: {e}");
                    return;
                }
                push_now_playing(&ui, p);
            }
            queue_model.set_vec(to_queue_rows(&tracks, Some(start)));
        });
    }

    // --- Swipes and the long-press menu: add to the queue, take out of it ---
    {
        let (db_folder, rows_folder, player_folder, queue_folder) =
            (db.clone(), rows_model.clone(), player.clone(), queue_model.clone());
        let weak_folder = ui.as_weak();
        ui.on_library_enqueue_folder(move |index, deep| {
            let Some(ui) = weak_folder.upgrade() else { return };
            let Some(row) = rows_folder.row_data(index as usize) else { return };
            let path = row.path.to_string();

            // `is_folder` on a row does not mean "directory". A file with
            // several subsongs — an .nsf, a .gsflib — is shown as a folder too,
            // because to whoever is listening that is what it is. Its `path` is
            // the file. So ask the filesystem rather than trusting the flag.
            let on_disk = std::path::Path::new(&path);
            let mut tracks = if on_disk.is_dir() {
                // Already the recursive answer: the query matches
                // `path LIKE 'folder/%'`.
                let mut all = db_folder.get_tracks_by_folder(&path).unwrap_or_default();
                if !deep {
                    let prefix = format!("{}/", path.trim_end_matches('/'));
                    all.retain(|t| {
                        // On the real file: a subsong's `#n` suffix does not
                        // change which directory it lives in.
                        let real = tunante_core::vgm_path::parse_vgm_path(&t.path).0;
                        real.strip_prefix(prefix.as_str())
                            .is_some_and(|rest| !rest.contains('/'))
                    });
                }
                all
            } else {
                // A file, with or without subsongs. Take the whole thing:
                // holding an .nsf and getting one of its forty tunes would be a
                // surprise. Its siblings in the directory are filtered out by
                // comparing real paths.
                let parent = on_disk
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let mut mine: Vec<_> = db_folder
                    .get_tracks_by_folder(&parent)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|t| tunante_core::vgm_path::parse_vgm_path(&t.path).0 == path)
                    .collect();
                mine.sort_by_key(|t| tunante_core::vgm_path::parse_vgm_path(&t.path).1.unwrap_or(0));
                mine
            };

            if tracks.is_empty() {
                if let Ok(Some(t)) = db_folder.get_track_by_path(&path) {
                    tracks.push(t);
                } else {
                    return;
                }
            }

            if let Some(p) = player_folder.borrow_mut().as_mut() {
                for t in tracks {
                    p.enqueue(t);
                }
                // Nothing was playing, so the first of them becomes the track.
                if p.current().is_none() {
                    let _ = p.next();
                    push_now_playing(&ui, p);
                }
                queue_folder.set_vec(to_queue_rows(p.queue().tracks(), p.current_index()));
            }
        });
    }
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        let (db, rows_model) = (db.clone(), rows_model.clone());
        let weak = ui.as_weak();
        ui.on_library_enqueued(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(row) = rows_model.row_data(index as usize) else { return };
            if row.is_folder {
                return;
            }
            let path = row.path.to_string();
            let Ok(Some(track)) = db.get_track_by_path(&path) else { return };

            if let Some(p) = player.borrow_mut().as_mut() {
                p.enqueue(track);
                // Nothing was playing, so the queued track becomes the track.
                if p.current().is_none() {
                    let _ = p.next();
                    push_now_playing(&ui, p);
                }
                queue_model.set_vec(to_queue_rows(p.queue().tracks(), p.current_index()));
            }
        });
    }
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        ui.on_queue_reordered(move |from, to| {
            if let Some(p) = player.borrow_mut().as_mut() {
                p.reorder(from.max(0) as usize, to.max(0) as usize);
                queue_model.set_vec(to_queue_rows(p.queue().tracks(), p.current_index()));
            }
        });
    }
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_queue_cleared(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(p) = player.borrow_mut().as_mut() {
                p.clear_queue();
                queue_model.set_vec(Vec::new());
                // Emptying the queue stops the music, so the Playing tab and
                // the mini-player have to stop claiming a track as well.
                push_now_playing(&ui, p);
            }
        });
    }
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        ui.on_queue_removed(move |index| {
            if let Some(p) = player.borrow_mut().as_mut() {
                p.remove_from_queue(index as usize);
                queue_model.set_vec(to_queue_rows(p.queue().tracks(), p.current_index()));
            }
        });
    }

    // --- Queue: jump to a track ---------------------------------------------
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_queue_activated(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(p) = player.borrow_mut().as_mut() {
                if p.play_index(index as usize).is_ok() {
                    push_now_playing(&ui, p);
                    mark_playing(&queue_model, index as usize);
                }
            }
        });
    }

    // --- Transport -----------------------------------------------------------
    {
        let player = player.clone();
        let weak = ui.as_weak();
        ui.on_toggle_play(move || {
            if let Some(p) = player.borrow_mut().as_mut() {
                p.toggle_play();
                if let Some(ui) = weak.upgrade() {
                    ui.set_playing(p.is_playing());
                }
            }
        });
    }
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_next_track(move || {
            if let Some(p) = player.borrow_mut().as_mut() {
                let _ = p.next();
                if let Some(ui) = weak.upgrade() {
                    push_now_playing(&ui, p);
                }
                sync_queue_marker(p, &queue_model);
            }
        });
    }
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_prev_track(move || {
            if let Some(p) = player.borrow_mut().as_mut() {
                let _ = p.prev();
                if let Some(ui) = weak.upgrade() {
                    push_now_playing(&ui, p);
                }
                sync_queue_marker(p, &queue_model);
            }
        });
    }

    {
        let player = player.clone();
        let weak = ui.as_weak();
        ui.on_cycle_repeat(move || {
            use tunante_core::RepeatMode::*;
            if let Some(p) = player.borrow_mut().as_mut() {
                // Off → All → One → Off. The order most players use, and the one
                // where the destructive-looking mode is hardest to reach by
                // accident.
                p.set_repeat(match p.repeat() {
                    Off => All,
                    All => One,
                    One => Off,
                });
                if let Some(ui) = weak.upgrade() {
                    push_now_playing(&ui, p);
                }
            }
        });
    }
    {
        let player = player.clone();
        let weak = ui.as_weak();
        ui.on_toggle_shuffle(move || {
            if let Some(p) = player.borrow_mut().as_mut() {
                let on = !p.shuffle();
                p.set_shuffle(on);
                if let Some(ui) = weak.upgrade() {
                    push_now_playing(&ui, p);
                }
            }
        });
    }
    {
        let player = player.clone();
        let weak = ui.as_weak();
        ui.on_seek_to(move |ms| {
            if let Some(p) = player.borrow_mut().as_mut() {
                p.seek(ms.max(0) as u64);
                if let Some(ui) = weak.upgrade() {
                    ui.set_position_ms(p.position_ms() as i32);
                }
            }
        });
    }

    // --- Settings -------------------------------------------------------------
    let sleep = Rc::new(RefCell::new(session::SleepTimer::new()));

    {
        let player = player.clone();
        let weak = ui.as_weak();
        ui.on_set_volume(move |v| {
            if let Some(p) = player.borrow_mut().as_mut() {
                p.set_volume(v);
                if let Some(ui) = weak.upgrade() {
                    ui.set_volume(p.volume());
                }
            }
        });
    }
    {
        let sleep = sleep.clone();
        let weak = ui.as_weak();
        ui.on_cycle_sleep(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut t = sleep.borrow_mut();
            // off → 15 → 30 → 60 → off. Enough choices to be useful, few
            // enough to reach by tapping rather than by picking from a list.
            let next = if !t.is_running() {
                15
            } else {
                match t.remaining_minutes() {
                    0..=15 => 30,
                    16..=30 => 60,
                    _ => 0,
                }
            };
            if next == 0 {
                t.cancel();
            } else {
                t.start(next);
            }
            ui.set_sleep_running(t.is_running());
            ui.set_sleep_minutes(t.remaining_minutes() as i32);
        });
    }
    {
        let (db, dbfile, scan_tx) = (db.clone(), dbfile.clone(), scan_tx.clone());
        let weak = ui.as_weak();
        ui.on_rescan(move || {
            let Some(ui) = weak.upgrade() else { return };
            let folders: Vec<PathBuf> = db
                .get_monitored_folders()
                .unwrap_or_default()
                .into_iter()
                .map(|f| PathBuf::from(f.path))
                .collect();
            if folders.is_empty() {
                return;
            }
            ui.set_scan_status("Analizando…".into());
            let (tx, dbfile) = (scan_tx.clone(), dbfile.clone());
            std::thread::spawn(move || {
                let Ok(db) = Database::new(&dbfile) else {
                    let _ = tx.send(None);
                    return;
                };
                for folder in folders {
                    let _ = library::scan_folder(&db, &folder, |p| {
                        let _ = tx.send(Some(format!(
                            "Analizando {}/{}\n{} pistas encontradas",
                            p.scanned, p.total, p.added
                        )));
                    });
                }
                let _ = tx.send(None);
            });
        });
    }
    {
        // Adding a folder later reuses the first-run picker rather than being a
        // second, subtly different browser.
        let (picker, refresh) = (picker.clone(), refresh_picker.clone());
        let weak = ui.as_weak();
        ui.on_add_folder(move || {
            let Some(ui) = weak.upgrade() else { return };
            picker.borrow_mut().chosen.clear();
            refresh();
            ui.set_setup_mode(true);
        });
    }
    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        ui.on_cycle_loops(move || {
            let Some(ui) = weak.upgrade() else { return };
            // 1 → 2 → 3 → ∞(0) → 1. Two is the usual choice for a chiptune rip.
            let next = match ui.get_loop_count() {
                1 => 2,
                2 => 3,
                3 => 0,
                _ => 1,
            };
            ui.set_loop_count(next);
            let _ = db.set_setting("mini.loop_count", &next.to_string());
            if let Some(p) = player.borrow_mut().as_mut() {
                p.set_loop_settings(next.max(1) as u32, ui.get_fade_seconds() as u64 * 1000);
            }
        });
    }
    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        ui.on_cycle_fade(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = match ui.get_fade_seconds() {
                0 => 4,
                4 => 8,
                8 => 15,
                _ => 0,
            };
            ui.set_fade_seconds(next);
            let _ = db.set_setting("mini.fade_seconds", &next.to_string());
            if let Some(p) = player.borrow_mut().as_mut() {
                p.set_loop_settings(ui.get_loop_count().max(1) as u32, next as u64 * 1000);
            }
        });
    }

    // --- Search --------------------------------------------------------------
    {
        let (db, rows_model, tree) = (db.clone(), rows_model.clone(), tree.clone());
        let weak = ui.as_weak();
        ui.on_search_changed(move |text| {
            let Some(ui) = weak.upgrade() else { return };
            let q = text.trim();

            // Empty query returns to the tree rather than listing everything:
            // the whole point of not materialising the library is not to do that.
            if q.is_empty() {
                let rows = tree.borrow().rows(&db);
                ui.set_library_total(rows.len() as i32);
                rows_model.set_vec(to_ui_rows(&rows));
                return;
            }

            // Straight to the FTS5 index that already exists in the schema,
            // capped: nobody reads past a few hundred hits, and building more
            // rows than that is memory spent on nothing.
            let hits = db.search_tracks(q).unwrap_or_default();
            let rows: Vec<library::Row> = hits
                .iter()
                .take(300)
                .map(|t| library::Row {
                    label: if t.title.is_empty() { t.path.clone() } else { t.title.clone() },
                    detail: library::format_duration(t.duration_ms),
                    depth: 0,
                    is_folder: false,
                    expanded: false,
                    path: t.path.clone(),
                })
                .collect();
            ui.set_library_total(hits.len() as i32);
            rows_model.set_vec(to_ui_rows(&rows));
        });
    }

    // --- Lock screen and headset buttons -------------------------------------
    //
    // The D-Bus server runs on its own thread; commands come back over a
    // channel that the progress timer below drains. Going through a channel
    // rather than `invoke_from_event_loop` keeps the player's `Rc` on one
    // thread, where it belongs, instead of forcing the whole graph to be `Send`.
    let (mpris_tx, mpris_rx) = mpris::spawn();

    // --- Progress, and moving to the next track when one ends ---------------
    let timer = slint::Timer::default();
    {
        let player = player.clone();
        let queue_model = queue_model.clone();
        let (db, tree, rows_model) = (db.clone(), tree.clone(), rows_model.clone());
        let sleep = sleep.clone();
        let mut ticks: u64 = 0;
        let weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(500),
            move || {
                let Some(ui) = weak.upgrade() else { return };

                // Scan progress, and the handover when it finishes.
                let mut scan_done = false;
                while let Ok(msg) = scan_rx.try_recv() {
                    match msg {
                        Some(text) => ui.set_scan_status(SharedString::from(text)),
                        None => scan_done = true,
                    }
                }
                if scan_done {
                    ui.set_scan_status(SharedString::new());
                    // The roots only exist once the scan has been asked for, so
                    // the tree is rebuilt here rather than at startup.
                    let roots: Vec<PathBuf> = db
                        .get_monitored_folders()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|f| PathBuf::from(f.path))
                        .collect();
                    *tree.borrow_mut() = library::Tree::new(roots);
                    let rows = tree.borrow().rows(&db);
                    ui.set_library_total(rows.len() as i32);
                    rows_model.set_vec(to_ui_rows(&rows));
                    ui.set_tab(2);
                }

                let mut guard = player.borrow_mut();
                let Some(p) = guard.as_mut() else { return };

                // Anything the lock screen or a headset button asked for.
                while let Ok(cmd) = mpris_rx.try_recv() {
                    match cmd {
                        mpris::Command::PlayPause => p.toggle_play(),
                        mpris::Command::Play => {
                            if !p.is_playing() {
                                p.toggle_play()
                            }
                        }
                        mpris::Command::Pause => {
                            if p.is_playing() {
                                p.toggle_play()
                            }
                        }
                        mpris::Command::Stop => p.stop(),
                        mpris::Command::Next => {
                            let _ = p.next();
                        }
                        mpris::Command::Previous => {
                            let _ = p.prev();
                        }
                        mpris::Command::SetVolume(v) => p.set_volume(v as f32),
                        // Seeking needs the decoder protocol to grow a seek
                        // command first. Advertised as unsupported, so nothing
                        // well-behaved should be asking.
                        mpris::Command::Seek(_) => {}
                    }
                    push_now_playing(&ui, p);
                    sync_queue_marker(p, &queue_model);
                }

                if p.poll_track_end() {
                    push_now_playing(&ui, p);
                    sync_queue_marker(p, &queue_model);
                }
                ui.set_position_ms(p.position_ms() as i32);
                ui.set_duration_ms(p.duration_ms() as i32);
                ui.set_playing(p.is_playing());

                // Sleep timer. Only counts down while sound is actually coming
                // out: a timer that expires during a pause would be a promise
                // broken in the wrong direction.
                if p.is_playing() {
                    let mut t = sleep.borrow_mut();
                    if t.tick(500) {
                        p.stop();
                    }
                    ui.set_sleep_running(t.is_running());
                    ui.set_sleep_minutes(t.remaining_minutes() as i32);
                }

                // Save the session as we go, not only on exit: a phone app is
                // killed by the system far more often than it is closed, and a
                // resume that only survives a clean exit rarely fires.
                ticks += 1;
                if ticks % 10 == 0 {
                    session::Session::save(
                        &db,
                        p.current().map(|t| t.path.as_str()),
                        p.position_ms(),
                        p.volume(),
                        p.shuffle(),
                        match p.repeat() {
                            tunante_core::RepeatMode::All => 1,
                            tunante_core::RepeatMode::One => 2,
                            tunante_core::RepeatMode::Off => 0,
                        },
                    );
                }

                // The MPRIS side works out what actually changed; sending on
                // every tick would otherwise wake every listener on the bus
                // twice a second, which on a phone is battery.
                let (title, artist, album) = match p.current() {
                    Some(t) => (t.title.clone(), t.artist.clone(), t.album.clone()),
                    None => (String::new(), String::new(), String::new()),
                };
                let _ = mpris_tx.send(mpris::Update {
                    title,
                    artist,
                    album,
                    duration_ms: p.duration_ms(),
                    position_ms: p.position_ms(),
                    playing: p.is_playing(),
                    has_track: p.current().is_some(),
                });
            },
        );
    }

    ui.run()?;
    Ok(())
}

/// Where the library database lives.
///
/// `$XDG_DATA_HOME/tunante-mini`, falling back to `~/.local/share`. Separate from
/// the desktop app's database on purpose: the two are independent libraries with
/// different paths on different machines.
fn db_path() -> Result<PathBuf, std::io::Error> {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".local/share")
        });
    let dir = base.join("tunante-mini");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("tunante-mini.db"))
}

/// Turn a `data:image/…;base64,…` URI into something Slint can draw.
///
/// Scaled down on the way in. A cover is often 1000² or larger, which is
/// 4 MB of RGBA for a square that is never shown bigger than the screen's
/// width — and holding the full-size decode is exactly the mistake that has
/// Amberol sitting at 3 GB with a thousand songs.
fn decode_artwork(data_uri: &str, max_side: u32) -> Option<slint::Image> {
    use base64::Engine;

    let b64 = data_uri.split(",").nth(1)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;

    let decoded = image::load_from_memory(&bytes).ok()?;
    let decoded = if decoded.width().max(decoded.height()) > max_side {
        decoded.thumbnail(max_side, max_side)
    } else {
        decoded
    };
    let rgba = decoded.to_rgba8();

    let mut buffer =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(rgba.width(), rgba.height());
    buffer.make_mut_bytes().copy_from_slice(rgba.as_raw());
    Some(slint::Image::from_rgba8(buffer))
}

/// Fetch and show the cover of whatever is playing, or clear it.
fn refresh_artwork(ui: &AppWindow, path: Option<&str>, max_side: u32) {
    let art = path
        .and_then(|p| {
            let real = tunante_core::vgm_path::parse_vgm_path(p).0.to_string();
            decoder::artwork(std::path::Path::new(&real), std::time::Duration::from_secs(5))
        })
        .and_then(|uri| decode_artwork(&uri, max_side));

    ui.set_now_art(art.unwrap_or_default());
}

fn to_ui_rows(rows: &[library::Row]) -> Vec<LibraryRow> {
    rows.iter()
        .map(|r| LibraryRow {
            title: SharedString::from(r.label.as_str()),
            subtitle: SharedString::from(r.detail.as_str()),
            depth: r.depth as i32,
            is_folder: r.is_folder,
            expanded: r.expanded,
            path: SharedString::from(r.path.as_str()),
        })
        .collect()
}

fn to_queue_rows(
    tracks: &[tunante_core::db::models::Track],
    playing: Option<usize>,
) -> Vec<QueueRow> {
    tracks
        .iter()
        .enumerate()
        .map(|(i, t)| QueueRow {
            title: SharedString::from(if t.title.is_empty() {
                t.path.as_str()
            } else {
                t.title.as_str()
            }),
            subtitle: SharedString::from(
                [t.artist.as_str(), t.album.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" — "),
            ),
            playing: Some(i) == playing,
        })
        .collect()
}

fn mark_playing(model: &Rc<VecModel<QueueRow>>, index: usize) {
    for i in 0..model.row_count() {
        if let Some(mut row) = model.row_data(i) {
            let should = i == index;
            if row.playing != should {
                row.playing = should;
                model.set_row_data(i, row);
            }
        }
    }
}

/// Move the "now playing" marker to wherever the queue says we are.
///
/// Asks the queue for the index rather than hunting for a matching title: two
/// tracks in an album can share a title, and a set of subsongs from one file
/// routinely do.
fn sync_queue_marker(p: &player::Player, model: &Rc<VecModel<QueueRow>>) {
    if let Some(i) = p.current_index() {
        mark_playing(model, i);
    }
}

/// How big a cover is ever drawn. Anything larger is memory nobody sees.
const MAX_ART_SIDE: u32 = 720;

fn push_now_playing(ui: &AppWindow, p: &player::Player) {
    let art_path = p.current().map(|t| t.path.clone());
    refresh_artwork(ui, art_path.as_deref(), MAX_ART_SIDE);

    ui.set_shuffle(p.shuffle());
    ui.set_repeat(match p.repeat() {
        tunante_core::RepeatMode::Off => 0,
        tunante_core::RepeatMode::All => 1,
        tunante_core::RepeatMode::One => 2,
    });

    match p.current() {
        Some(t) => {
            ui.set_now_title(SharedString::from(if t.title.is_empty() {
                t.path.as_str()
            } else {
                t.title.as_str()
            }));
            ui.set_now_artist(SharedString::from(t.artist.as_str()));
            ui.set_now_album(SharedString::from(t.album.as_str()));
            // The library marks its own row from this, rather than the rows
            // carrying a flag: they are rebuilt in five places and none of them
            // would hear about a track change.
            ui.set_now_path(SharedString::from(t.path.as_str()));
        }
        None => {
            ui.set_now_title("Nada sonando".into());
            ui.set_now_artist(SharedString::new());
            ui.set_now_album(SharedString::new());
            ui.set_now_path(SharedString::new());
        }
    }
    ui.set_playing(p.is_playing());
}

/// Generated rows for `--rows`, to measure what the list costs.
///
/// Note what this is *not*: a stand-in for the real library. That one never
/// materialises every row — the tree only builds what is expanded, and a search
/// is capped. Holding all of these at once is the worst case, on purpose.
fn generated_rows(n: usize) -> Vec<LibraryRow> {
    (0..n)
        .map(|i| {
            let is_folder = i % 12 == 0;
            LibraryRow {
                title: SharedString::from(if is_folder {
                    format!("Folder {}", i / 12)
                } else {
                    format!("Track {i:06} — a title long enough to need eliding")
                }),
                subtitle: SharedString::from(if is_folder {
                    "11 items".to_string()
                } else {
                    format!("{}:{:02}", i % 5 + 1, i % 60)
                }),
                depth: (i % 3) as i32,
                is_folder,
                expanded: is_folder && i % 24 == 0,
                path: SharedString::new(),
            }
        })
        .collect()
}
