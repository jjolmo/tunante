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
//! A few more flags exist, and they are not features — they are how the app gets
//! measured and driven from a shell when nobody can put a finger on the glass:
//!
//! ```text
//! tunante-mini --rows N            fake rows, to measure what the list costs
//! tunante-mini --focus-search      start on Library with the search focused
//! tunante-mini --mode N            start on Library in view N (0..3)
//! tunante-mini --open-playlist X   start inside the playlist named X
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
//!
//! `--mode` and `--open-playlist` reach a view that otherwise takes two taps.
//! A desktop session will happily move the pointer over this window and then
//! drop the button event on the floor, so "does this screen draw correctly?" is
//! not answerable by clicking from a script — only by starting there.

mod boost;
// logind, reached over D-Bus, and only `mpris` uses it.
#[cfg(target_os = "linux")]
mod inhibit;
mod library;
mod mpris;
mod output;
mod picker;
mod player;
use tunante_core::session;

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
    let start_mode = arg_value("--mode").and_then(|s| s.parse::<i32>().ok());
    let open_playlist = arg_value("--open-playlist");
    let open_game = arg_value("--open-game");

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

    // The presentation override: auto picks by window width (>= 900px means
    // the desktop shell), and these force one or the other. The flag serves
    // development and scripts; the settings screen will write the same
    // property when it grows the control.
    if args.iter().any(|a| a == "--desktop") {
        // Before the window maps: AppWindow's preferred size keys off this, so
        // the window opens desktop-sized instead of phone-shaped.
        ui.set_ui_mode(2);
    } else if args.iter().any(|a| a == "--mini") {
        ui.set_ui_mode(1);
    }

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
    // Cloned rather than moved: the console view needs the roots again to answer
    // "everything of this console", which is a query across the whole library.
    let tree = Rc::new(RefCell::new(library::Tree::new(roots.clone())));
    let rows_model = Rc::new(VecModel::from(Vec::<LibraryRow>::new()));

    let db = Rc::new(db);

    if let Some(n) = fake_rows {
        rows_model.set_vec(generated_rows(n));
    } else {
        rows_model.set_vec(to_ui_rows(&tree.borrow().rows(&db)));
    }

    ui.set_library_total(rows_model.row_count() as i32);
    ui.set_library_rows(ModelRc::from(rows_model.clone()));

    let grid_model = Rc::new(VecModel::from(Vec::<GridLine>::new()));
    let art_cache: Rc<RefCell<Vec<(String, slint::Image)>>> = Rc::new(RefCell::new(Vec::new()));
    /// Set by the cover-download worker, acted on by the UI timer.
    ///
    /// The cache is an `Rc` owned by the UI thread and the download runs on its
    /// own, so this is the handover. It matters because the cache remembers
    /// *misses* too: without clearing it, every folder the run just gave a cover
    /// to keeps showing the placeholder until the app restarts.
    let art_dirty = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    ui.set_library_grid_lines(ModelRc::from(grid_model.clone()));

    // Two playlist models, not one. The picker that "add to a playlist" opens has
    // to show every playlist even while the search box is narrowing the Listas
    // view, and filtering a single shared model would hide playlists from it
    // without ever saying why.
    let playlists_model = Rc::new(VecModel::from(Vec::<PlaylistRow>::new()));
    let all_playlists_model = Rc::new(VecModel::from(Vec::<PlaylistRow>::new()));
    ui.set_playlists(ModelRc::from(playlists_model.clone()));
    ui.set_all_playlists(ModelRc::from(all_playlists_model.clone()));

    let views = Views {
        rows: rows_model.clone(),
        grid: grid_model.clone(),
        playlists: playlists_model.clone(),
        all_playlists: all_playlists_model.clone(),
        art: art_cache.clone(),
    };

    // El primer dibujado del árbol no pasa por `refresh_library`, así que el
    // selector de listas estaría vacío hasta el primer toque.
    refresh_playlists(&db, &views, "");

    // Instruments: land on a view directly instead of tapping to reach it.
    if start_mode.is_some() || open_playlist.is_some() || open_game.is_some() {
        let mode = if open_game.is_some() { 3 } else { start_mode.unwrap_or(3) };
        {
            let mut t = tree.borrow_mut();
            t.mode = library::Mode::from_index(mode);
            t.nav.clear();
            // Inside a game, which is the one level nothing could reach from a
            // script. Every other view is one `--mode` away, but a game is a
            // row you have to press, and this desktop's compositor refuses
            // synthetic clicks: XTEST will not move the pointer and winit
            // ignores XSendEvent. So the same door the playlist instrument
            // uses, for the same reason.
            //
            // The `juego:` prefix is not a detail the caller should know, so it
            // is added here rather than asked for.
            if let Some(name) = &open_game {
                t.mode = library::Mode::Games;
                t.nav.push(format!("juego:{name}"));
                // Says so when there is no such game, the way the playlist
                // instrument does. Landing on a silently empty level looks
                // exactly like the bug this exists to rule out.
                if t.grid_tracks(&db, library::Mode::Games).is_empty() {
                    eprintln!("no hay ningún juego llamado «{name}»");
                }
            }
            if let Some(name) = &open_playlist {
                if let Some(p) = db
                    .get_playlists()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|p| p.name == *name)
                {
                    t.mode = library::Mode::Playlists;
                    t.nav.push(p.id);
                } else {
                    eprintln!("no hay ninguna lista llamada «{name}»");
                }
            }
        }
        ui.set_library_mode(if open_playlist.is_some() { 4 } else { mode });
        ui.set_tab(2);
        refresh_library(&ui, &tree, &db, &views);
    }

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
            if let Ok(values) = tunante_helper::probe(
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
        let (tree, db, rows_model, player, queue_model, views) = (
            tree.clone(),
            db.clone(),
            rows_model.clone(),
            player.clone(),
            queue_model.clone(),
            views.clone(),
        );
        let weak = ui.as_weak();

        ui.on_library_activated(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(row) = rows_model.row_data(index as usize) else { return };
            let path = row.path.to_string();

            if row.is_folder {
                tree.borrow_mut().toggle(&path);
                refresh_library(&ui, &tree, &db, &views);
                return;
            }

            // Inside a playlist the context is the playlist, not the folder the
            // file happens to sit in: a playlist exists precisely to be an order
            // the disk does not have.
            let open_playlist = {
                let t = tree.borrow();
                if t.mode == library::Mode::Playlists {
                    t.nav.first().cloned()
                } else {
                    None
                }
            };

            let tracks = match &open_playlist {
                Some(id) => db.get_playlist_tracks(id).unwrap_or_default(),
                // Playing a track makes its folder the queue, which is what
                // anyone expects: tapping one song from an album queues the
                // album.
                None => {
                    let folder = std::path::Path::new(&path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    db.get_tracks_by_folder(&folder).unwrap_or_default()
                }
            };
            // By path and not by the row index: with the search box narrowing the
            // rows, index `i` addresses the filtered list while the context here
            // is the whole playlist.
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

    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_library_mode_changed(move |i| {
            let Some(ui) = weak_v.upgrade() else { return };
            tree_v.borrow_mut().mode = library::Mode::from_index(i);
            // Changing view starts at the top of it. Coming back to Consoles
            // three levels into where you were last time is disorienting, and
            // the crumb would be the only clue.
            tree_v.borrow_mut().nav.clear();
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_library_columns_changed(move |_| {
            let Some(ui) = weak_v.upgrade() else { return };
            // Turning the phone changes how many cards fit, and the lines are
            // cut in Rust, so they have to be cut again.
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_library_back(move || {
            let Some(ui) = weak_v.upgrade() else { return };
            tree_v.borrow_mut().nav.pop();
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_library_grid_tapped(move |path| {
            let Some(ui) = weak_v.upgrade() else { return };
            tree_v.borrow_mut().nav.push(path.to_string());
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }

    // --- Playlists -----------------------------------------------------------
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_playlist_open_requested(move |id| {
            let Some(ui) = weak_v.upgrade() else { return };
            {
                let mut t = tree_v.borrow_mut();
                // Same `nav` the grid views use, so the crumb strip and
                // `on_library_back` work here without a line of new navigation.
                t.nav.push(id.to_string());
                // Entering a playlist with the search box still narrowing the
                // list of playlists would hide most of what is inside it.
                t.filter.clear();
            }
            ui.set_search(SharedString::from(""));
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_playlist_create(move |name| {
            let Some(ui) = weak_v.upgrade() else { return };
            let name = name.trim().to_string();
            // Un nombre en blanco deja la lista sin migas, y la tira de volver
            // sólo existe si hay migas: entrar en ella sería quedarse dentro.
            // La interfaz ya lo impide; esto es la red de abajo.
            if name.is_empty() {
                return;
            }
            if let Err(e) = db_v.create_playlist_named(&name) {
                eprintln!("no se pudo crear la lista: {e}");
                return;
            }
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let roots_v = roots.clone();
        let weak_v = ui.as_weak();
        ui.on_add_path_to_playlist(move |path, deep, playlist_id| {
            let Some(ui) = weak_v.upgrade() else { return };
            add_to_playlist(
                &ui, &db_v, &tree_v, &views_v, &roots_v, &path, deep, &playlist_id,
            );
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let roots_v = roots.clone();
        let weak_v = ui.as_weak();
        ui.on_add_path_to_new_playlist(move |path, deep, name| {
            let Some(ui) = weak_v.upgrade() else { return };
            let name = name.trim().to_string();
            if name.is_empty() {
                return;
            }
            match db_v.create_playlist_named(&name) {
                Ok(id) => add_to_playlist(
                    &ui, &db_v, &tree_v, &views_v, &roots_v, &path, deep, &id,
                ),
                Err(e) => eprintln!("no se pudo crear la lista: {e}"),
            }
        });
    }
    {
        let (db_v, tree_v, player_v, queue_v) =
            (db.clone(), tree.clone(), player.clone(), queue_model.clone());
        ui.on_playlist_enqueue_all(move || {
            let Some(id) = tree_v.borrow().nav.first().cloned() else { return };
            enqueue_playlist(&db_v, &player_v, &queue_v, &id);
        });
    }
    {
        let (db_v, player_v, queue_v) = (db.clone(), player.clone(), queue_model.clone());
        ui.on_playlist_enqueue_id(move |id| {
            enqueue_playlist(&db_v, &player_v, &queue_v, &id);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_playlist_track_removed(move |path| {
            let Some(ui) = weak_v.upgrade() else { return };
            let Some(pid) = tree_v.borrow().nav.first().cloned() else { return };
            let Ok(Some(track)) = db_v.get_track_by_path(&path) else { return };
            if let Err(e) = db_v.remove_track_from_playlist(&pid, &track.id) {
                eprintln!("no se pudo quitar de la lista: {e}");
                return;
            }
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_playlist_reordered(move |from, to| {
            let Some(ui) = weak_v.upgrade() else { return };
            let Some(pid) = tree_v.borrow().nav.first().cloned() else { return };
            let mut tracks = db_v.get_playlist_tracks(&pid).unwrap_or_default();

            let from = from.max(0) as usize;
            if from >= tracks.len() {
                return;
            }
            // El destino se recorta en vez de descartarse: arrastrar más allá del
            // final quiere decir "al final", no "no me hagas caso".
            let to = to.clamp(0, tracks.len() as i32 - 1) as usize;
            if from == to {
                return;
            }

            let moved = tracks.remove(from);
            tracks.insert(to, moved);
            let ids: Vec<String> = tracks.into_iter().map(|t| t.id).collect();
            if let Err(e) = db_v.reorder_playlist_tracks(&pid, &ids) {
                eprintln!("no se pudo reordenar la lista: {e}");
                return;
            }
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_playlist_rename(move |id, name| {
            let Some(ui) = weak_v.upgrade() else { return };
            let name = name.trim().to_string();
            if name.is_empty() {
                return;
            }
            if let Err(e) = db_v.rename_playlist(&id, &name) {
                eprintln!("no se pudo renombrar la lista: {e}");
                return;
            }
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }
    {
        let (db_v, tree_v, views_v) = (db.clone(), tree.clone(), views.clone());
        let weak_v = ui.as_weak();
        ui.on_playlist_delete(move |id| {
            let Some(ui) = weak_v.upgrade() else { return };
            if let Err(e) = db_v.delete_playlist(&id) {
                eprintln!("no se pudo borrar la lista: {e}");
                return;
            }
            // Si estabas dentro de la que acaba de irse, salir. Quedarse sería
            // una vista sin nada y con unas migas que nombran a un fantasma.
            {
                let mut t = tree_v.borrow_mut();
                if t.nav.first().is_some_and(|open| *open == id.as_str()) {
                    t.nav.clear();
                }
            }
            refresh_library(&ui, &tree_v, &db_v, &views_v);
        });
    }

    // --- Swipes and the long-press menu: add to the queue, take out of it ---
    {
        let (db_folder, rows_folder, player_folder, queue_folder) =
            (db.clone(), rows_model.clone(), player.clone(), queue_model.clone());
        let roots_folder = roots.clone();
        let weak_folder = ui.as_weak();
        ui.on_library_enqueue_path(move |path, deep| {
            let Some(ui) = weak_folder.upgrade() else { return };
            let _ = &rows_folder;
            let tracks = tracks_for_path(&db_folder, &roots_folder, &path, deep);
            if tracks.is_empty() {
                return;
            }
            enqueue_all(&ui, &player_folder, &queue_folder, tracks);
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
        // Cover art, reusing the scan's status line and its channel. Same shape
        // as `on_rescan` above on purpose: one idiom in this app for "long job
        // with a message under it", not two.
        let (db, scan_tx) = (db.clone(), scan_tx.clone());
        let dirty_outer = std::sync::Arc::clone(&art_dirty);
        let weak = ui.as_weak();
        ui.on_descargar_caratulas(move || {
            let Some(ui) = weak.upgrade() else { return };
            let tracks = db.get_all_tracks().unwrap_or_default();
            if tracks.is_empty() {
                return;
            }
            ui.set_scan_status("Buscando carátulas…".into());
            let tx = scan_tx.clone();
            let dirty = std::sync::Arc::clone(&dirty_outer);
            std::thread::spawn(move || {
                let all: Vec<(String, String)> = tunante_core::console::CONSOLES
                    .iter()
                    .filter_map(|c| c.libretro.map(|s| (c.id.to_string(), s.to_string())))
                    .collect();
                // One request per game: a hundred tracks of one soundtrack want
                // one cover between them.
                let mut seen = std::collections::HashSet::new();
                let reqs: Vec<tunante_art::resolver::CoverRequest> = tracks
                    .iter()
                    .filter(|t| seen.insert((t.console_id.clone(), t.game.clone())))
                    .map(|t| {
                        let candidates = tunante_art::resolver::candidates_for(
                            &t.game, &t.album, &t.path,
                        );
                        let real = t.path.split('#').next().unwrap_or(&t.path);
                        tunante_art::resolver::CoverRequest {
                            libretro_system: tunante_core::console::by_id(&t.console_id)
                                .and_then(|c| c.libretro)
                                .map(str::to_string),
                            other_systems: all
                                .iter()
                                .filter(|(o, _)| *o != t.console_id)
                                .cloned()
                                .collect(),
                            console_id: t.console_id.clone(),
                            candidates,
                            dir: std::path::Path::new(real).parent().map(|p| p.to_path_buf()),
                        }
                    })
                    .collect();

                let resolver = std::sync::Arc::new(tunante_art::resolver::Resolver::new());
                let opts = tunante_art::resolver::BulkOptions::default();
                let plans = resolver.resolve_many(reqs, &opts, |p| {
                    let _ = tx.send(Some(format!(
                        "Carátulas {}/{}\n{} encontradas",
                        p.done, p.total, p.found
                    )));
                });
                let found = plans.iter().filter(|p| p.source != "none").count();
                dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = tx.send(Some(format!(
                    "{found} carátulas de {} juegos",
                    plans.len()
                )));
                std::thread::sleep(std::time::Duration::from_secs(3));
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
        let (db, rows_model, tree, views) =
            (db.clone(), rows_model.clone(), tree.clone(), views.clone());
        let weak = ui.as_weak();
        ui.on_search_changed(move |text| {
            let Some(ui) = weak.upgrade() else { return };
            let q = text.trim().to_string();

            // Two different things share one field, on purpose.
            //
            // In the tree it is a search: the whole library, through the FTS5
            // index, because that is the view you use when you do not know
            // where a thing is.
            //
            // In Discos and Consolas it is a filter over what is on screen.
            // Searching the whole library from a grid of albums would replace
            // the grid with a list of tracks, which is the one thing the grid
            // exists not to be — and inside a console it would silently leave
            // that console.
            if tree.borrow().mode != library::Mode::Tree {
                tree.borrow_mut().filter = q;
                refresh_library(&ui, &tree, &db, &views);
                return;
            }

            // Empty query returns to the tree rather than listing everything:
            // the whole point of not materialising the library is not to do that.
            if q.is_empty() {
                refresh_library(&ui, &tree, &db, &views);
                return;
            }

            // Straight to the FTS5 index that already exists in the schema,
            // capped: nobody reads past a few hundred hits, and building more
            // rows than that is memory spent on nothing.
            let hits = db.search_tracks(&q).unwrap_or_default();
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
            ui.set_library_grid(false);
            grid_model.set_vec(Vec::new());
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

    // Watches whether the sound has anywhere to go. Cheap and silent until it
    // has something to say; see output.rs for why this is not a rodio question.
    let output_watch = output::spawn();
    let was_silent = std::cell::Cell::new(false);

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
                // Covers arrived: drop what we remember about folder art —
                // including the misses — and redraw whatever is on screen.
                if art_dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    art_cache.borrow_mut().clear();
                    let path = ui.get_now_path().to_string();
                    refresh_artwork(&ui, (!path.is_empty()).then_some(path.as_str()), MAX_ART_SIDE);
                    let rows = tree.borrow().rows(&db);
                    rows_model.set_vec(to_ui_rows(&rows));
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
                        mpris::Command::SetRepeat(mode) => p.set_repeat(mode),
                        mpris::Command::SetShuffle(on) => p.set_shuffle(on),
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

                // The output can vanish without anything failing: PulseAudio
                // parks the stream on a null sink and the app plays into it
                // for as long as you let it. See output.rs.
                //
                // Pausing on the way in, once, rather than on every tick: it
                // saves your place and stops the phone decoding into nothing,
                // but if you press play anyway that is your business and the
                // banner is enough of an answer.
                output_watch.note_playing(p.is_playing());
                let silent = output_watch.is_silent();
                if silent != was_silent.get() {
                    was_silent.set(silent);
                    if silent && p.is_playing() {
                        p.toggle_play();
                    }
                    ui.set_output_warning(SharedString::from(if silent {
                        "Sin salida de audio"
                    } else {
                        ""
                    }));
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
                        // These already had their own settings keys, written
                        // from the cycle handlers. Going through Session too
                        // means one place writes them and one place reads them
                        // back, rather than two halves that can disagree.
                        ui.get_loop_count().max(0) as u32,
                        ui.get_fade_seconds().max(0) as u64,
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
                    volume: p.volume() as f64,
                    shuffle: p.shuffle(),
                    repeat: p.repeat(),
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
            tunante_helper::artwork(std::path::Path::new(&real), std::time::Duration::from_secs(5))
        })
        .and_then(|uri| decode_artwork(&uri, max_side));

    ui.set_now_art(art.unwrap_or_default());
}

/// Portadas de carpeta ya decodificadas y escaladas.
///
/// Con tope, y el tope es la razón de que exista: una rejilla construye todas
/// sus celdas de golpe, y sin límite una biblioteca grande dejaría una portada
/// residente por disco. A 224 px son 200 KB cada una; cuarenta son ocho megas
/// en el peor caso, y el caso normal es mucho menos porque la mayoría de las
/// carpetas de música de consola no traen imagen ninguna.
const ART_SIDE: u32 = 224;
const ART_CACHE: usize = 40;

fn folder_art(
    cache: &RefCell<Vec<(String, slint::Image)>>,
    dir: &str,
) -> slint::Image {
    if dir.is_empty() {
        return slint::Image::default();
    }
    if let Some((_, img)) = cache.borrow().iter().find(|(k, _)| k == dir) {
        return img.clone();
    }

    let img = library::folder_image(std::path::Path::new(dir))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| image::load_from_memory(&bytes).ok())
        .map(|d| d.thumbnail(ART_SIDE, ART_SIDE).to_rgba8())
        .map(|rgba| {
            let mut buf =
                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(rgba.width(), rgba.height());
            buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
            slint::Image::from_rgba8(buf)
        })
        // Se cachea también el "no hay portada": si no, cada refresco vuelve a
        // leer el directorio de todas las carpetas sin imagen, que son la
        // mayoría.
        .unwrap_or_default();

    let mut c = cache.borrow_mut();
    if c.len() >= ART_CACHE {
        c.remove(0);
    }
    c.push((dir.to_string(), img.clone()));
    img
}

/// Everything `refresh_library` writes into.
///
/// A bundle rather than five loose parameters: they always travel together, and
/// threading them one by one through every closure that can change the screen was
/// already five clones per closure before the playlists arrived.
#[derive(Clone)]
struct Views {
    rows: Rc<VecModel<LibraryRow>>,
    grid: Rc<VecModel<GridLine>>,
    playlists: Rc<VecModel<PlaylistRow>>,
    all_playlists: Rc<VecModel<PlaylistRow>>,
    art: Rc<RefCell<Vec<(String, slint::Image)>>>,
}

/// "1 pista" y no "1 pistas", igual que en la biblioteca.
fn playlist_subtitle(n: i64) -> String {
    if n == 1 { "1 pista".to_string() } else { format!("{n} pistas") }
}

/// Refill both playlist models from the database.
///
/// `all` is never filtered; `playlists` honours the search box.
fn refresh_playlists(db: &Database, views: &Views, filter: &str) {
    let all = db.get_playlists().unwrap_or_default();
    let row = |p: &tunante_core::db::models::Playlist| PlaylistRow {
        id: SharedString::from(p.id.as_str()),
        name: SharedString::from(p.name.as_str()),
        subtitle: SharedString::from(playlist_subtitle(p.track_count)),
    };

    views.all_playlists.set_vec(all.iter().map(row).collect::<Vec<_>>());

    let needle = library::plegar(filter);
    let visible: Vec<PlaylistRow> = all
        .iter()
        .filter(|p| needle.is_empty() || library::plegar(&p.name).contains(&needle))
        .map(row)
        .collect();
    views.playlists.set_vec(visible);
}

/// Rebuild whatever the library tab should be showing right now.
///
/// One place decides between the four views and, inside the two grid ones and
/// Listas, between a grid of cards, a list of playlists and a list of tracks.
/// Every path that can change what is on screen — switching view, tapping into a
/// console, opening a playlist, coming back, turning the phone — goes through
/// here, so none of them can disagree with the others.
fn refresh_library(
    ui: &AppWindow,
    tree: &Rc<RefCell<library::Tree>>,
    db: &Rc<Database>,
    views: &Views,
) {
    let (rows_model, grid_model, art_cache) = (&views.rows, &views.grid, &views.art);
    let t = tree.borrow();
    let mode = t.mode;

    // Siempre, no sólo en el modo Listas: el selector de «añadir a una lista» se
    // abre desde el árbol y desde las rejillas, y si el modelo sólo se llenase
    // al visitar Listas saldría vacío para quien no haya pasado por allí. Es una
    // consulta sobre una tabla de un puñado de filas, contra un toque que ya
    // hace consultas de carpeta.
    refresh_playlists(db, views, &t.filter);

    if mode == library::Mode::Playlists {
        ui.set_library_grid(false);
        grid_model.set_vec(Vec::new());

        match t.nav.first() {
            // El listado de listas.
            None => {
                ui.set_showing_playlists(true);
                ui.set_playlist_open(false);
                ui.set_library_crumb(SharedString::from(""));
                ui.set_playlist_count(0);
                rows_model.set_vec(Vec::new());
                ui.set_library_total(views.playlists.row_count() as i32);
            }
            // Dentro de una lista.
            Some(id) => {
                ui.set_showing_playlists(false);
                ui.set_playlist_open(true);
                // El nombre en las migas es lo que convierte la tira de volver
                // que ya existe en el botón "atrás" de esta vista.
                let name = db
                    .get_playlist(id)
                    .ok()
                    .flatten()
                    .map(|p| p.name)
                    .unwrap_or_default();
                ui.set_library_crumb(SharedString::from(name.as_str()));

                let tracks = db.get_playlist_tracks(id).unwrap_or_default();
                ui.set_playlist_count(tracks.len() as i32);

                let mut rows = library::playlist_rows(&tracks);
                let needle = library::plegar(&t.filter);
                if !needle.is_empty() {
                    rows.retain(|r| library::plegar(&r.label).contains(&needle));
                }
                rows_model.set_vec(to_ui_rows(&rows));
                ui.set_library_total(rows.len() as i32);
            }
        }
        return;
    }

    // Fuera del modo Listas ninguna de las dos puede quedar encendida: es lo que
    // mantiene excluyentes las ramas de lista del .slint.
    ui.set_showing_playlists(false);
    ui.set_playlist_open(false);

    ui.set_library_crumb(SharedString::from(t.crumb()));

    match t.grid(db, mode) {
        Some(cells) => {
            let columns = ui.get_library_columns().max(1) as usize;
            let total = cells.len();
            let lines: Vec<GridLine> = cells
                .chunks(columns)
                .map(|chunk| {
                    let mut fila: Vec<GridCell> = chunk
                        .iter()
                        .map(|c| GridCell {
                            title: SharedString::from(c.title.as_str()),
                            subtitle: SharedString::from(c.subtitle.as_str()),
                            path: SharedString::from(c.path.as_str()),
                            art: folder_art(art_cache, &c.art_dir),
                            console: SharedString::from(c.console.as_str()),
                            playing: false,
                        })
                        .collect();
                    // The last line is padded to full width. The cells are drawn
                    // as nothing and take no touches, and they are what lets the
                    // ListView keep every line the same height — which is the
                    // condition for it to virtualise at all.
                    while fila.len() < columns {
                        fila.push(GridCell::default());
                    }
                    GridLine { cells: ModelRc::from(Rc::new(VecModel::from(fila))) }
                })
                .collect();
            grid_model.set_vec(lines);
            rows_model.set_vec(Vec::new());
            ui.set_library_grid(true);
            ui.set_library_total(total as i32);
        }
        None => {
            let rows = match mode {
                library::Mode::Tree => t.rows_for(db, mode),
                _ => t.grid_tracks(db, mode),
            };
            rows_model.set_vec(to_ui_rows(&rows));
            grid_model.set_vec(Vec::new());
            ui.set_library_grid(false);
            ui.set_library_total(rows.len() as i32);
        }
    }
}

/// Everything of one console, across the whole library.
fn tracks_of_console(
    db: &Database,
    roots: &[PathBuf],
    console: &str,
) -> Vec<tunante_core::db::models::Track> {
    roots
        .iter()
        .flat_map(|r| db.get_tracks_by_folder(&r.to_string_lossy()).unwrap_or_default())
        .filter(|t| library::console_key(t) == console)
        .collect()
}

/// Every track of one game, wherever on disk it turned out to be.
///
/// Not `get_tracks_by_folder`: the whole point of the Games tab is that a game
/// is an album tag, so its tracks can be spread over several directories or
/// share one with another game.
fn tracks_of_game(
    db: &Database,
    roots: &[PathBuf],
    game: &str,
) -> Vec<tunante_core::db::models::Track> {
    let all: Vec<_> = roots
        .iter()
        .flat_map(|r| db.get_tracks_by_folder(&r.to_string_lossy()).unwrap_or_default())
        .collect();
    tunante_core::games::tracks_of(&all, game)
        .into_iter()
        .cloned()
        .collect()
}

/// Put a whole playlist at the end of the queue, without starting anything.
///
/// Deliberately not `enqueue_all`: that one starts playing when the player is
/// idle, and neither of the two buttons that land here promises more than to add.
fn enqueue_playlist(
    db: &Database,
    player: &Rc<RefCell<Option<player::Player>>>,
    queue_model: &Rc<VecModel<QueueRow>>,
    playlist_id: &str,
) {
    let tracks = db.get_playlist_tracks(playlist_id).unwrap_or_default();
    if tracks.is_empty() {
        return;
    }
    if let Some(p) = player.borrow_mut().as_mut() {
        p.enqueue_many(tracks);
        queue_model.set_vec(to_queue_rows(p.queue().tracks(), p.current_index()));
    }
}

/// Resolve a row's path and put whatever it holds into a playlist.
///
/// Shared by "add to that one" and "add to a new one", which differ only in
/// where the playlist id comes from.
#[allow(clippy::too_many_arguments)]
fn add_to_playlist(
    ui: &AppWindow,
    db: &Rc<Database>,
    tree: &Rc<RefCell<library::Tree>>,
    views: &Views,
    roots: &[PathBuf],
    path: &str,
    deep: bool,
    playlist_id: &str,
) {
    let tracks = tracks_for_path(db, roots, path, deep);
    if tracks.is_empty() {
        return;
    }
    let ids: Vec<String> = tracks.into_iter().map(|t| t.id).collect();
    if let Err(e) = db.add_tracks_to_playlist(playlist_id, &ids) {
        eprintln!("no se pudo añadir a la lista: {e}");
        return;
    }
    // Los recuentos del listado y del selector han cambiado, y si resulta que
    // estamos dentro de esa misma lista, también sus filas.
    refresh_library(ui, tree, db, views);
}

/// What a row's `path` actually stands for, as tracks.
///
/// Shared by "add to the queue" and "add to a playlist", which have to agree on
/// this to the letter: everything below is an encoding the views invented, and
/// two copies of it would drift the first time one of them was fixed.
///
/// `deep` only means anything for a real directory. Empty means nothing matched.
fn tracks_for_path(
    db: &Database,
    roots: &[PathBuf],
    path: &str,
    deep: bool,
) -> Vec<tunante_core::db::models::Track> {
    // The index views build rows whose `path` is not a path at all:
    // `consola:nes` for a console, `nes\u{1}/ruta/al/juego` for one of its games
    // — the console has to be in the key because a folder holding both .spc rips
    // and mp3s appears under two of them — and `juego:Nombre` for a game of the
    // Games tab, which is an album tag and may not correspond to any directory.
    // Those are resolved here rather than in the view, so this is the only place
    // that knows the encoding.
    if let Some(consola) = path.strip_prefix("consola:") {
        return tracks_of_console(db, roots, consola);
    }
    if let Some(juego) = path.strip_prefix("juego:") {
        return tracks_of_game(db, roots, juego);
    }
    if let Some((consola, dir)) = path.split_once('\u{1}') {
        return db
            .get_tracks_by_folder(dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| library::console_key(t) == consola)
            .collect();
    }

    // `is_folder` on a row does not mean "directory". A file with several
    // subsongs — an .nsf, a .gsflib — is shown as a folder too, because to
    // whoever is listening that is what it is. Its `path` is the file. So ask
    // the filesystem rather than trusting the flag.
    let on_disk = std::path::Path::new(path);
    let mut tracks = if on_disk.is_dir() {
        // Already the recursive answer: the query matches `path LIKE 'folder/%'`.
        let mut all = db.get_tracks_by_folder(path).unwrap_or_default();
        if !deep {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            all.retain(|t| {
                // On the real file: a subsong's `#n` suffix does not change
                // which directory it lives in.
                let real = tunante_core::vgm_path::parse_vgm_path(&t.path).0;
                real.strip_prefix(prefix.as_str())
                    .is_some_and(|rest| !rest.contains('/'))
            });
        }
        all
    } else {
        // A file, with or without subsongs. Take the whole thing: holding an
        // .nsf and getting one of its forty tunes would be a surprise. Its
        // siblings in the directory are filtered out by comparing real paths.
        let parent = on_disk
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut mine: Vec<_> = db
            .get_tracks_by_folder(&parent)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| tunante_core::vgm_path::parse_vgm_path(&t.path).0 == path)
            .collect();
        mine.sort_by_key(|t| tunante_core::vgm_path::parse_vgm_path(&t.path).1.unwrap_or(0));
        mine
    };

    if tracks.is_empty() {
        if let Ok(Some(t)) = db.get_track_by_path(path) {
            tracks.push(t);
        }
    }

    tracks
}

/// Put a batch at the end of the queue, and start it if nothing was playing.
///
/// Shared by every path that adds more than one track — a folder, a console,
/// one game of a console — so they all behave the same when the player is idle.
fn enqueue_all(
    ui: &AppWindow,
    player: &Rc<RefCell<Option<player::Player>>>,
    queue_model: &Rc<VecModel<QueueRow>>,
    tracks: Vec<tunante_core::db::models::Track>,
) {
    if tracks.is_empty() {
        return;
    }
    if let Some(p) = player.borrow_mut().as_mut() {
        p.enqueue_many(tracks);
        // Nothing was playing, so the first of them becomes the track.
        if p.current().is_none() {
            let _ = p.next();
            push_now_playing(ui, p);
        }
        queue_model.set_vec(to_queue_rows(p.queue().tracks(), p.current_index()));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tunante_core::db::models::Track;

    /// mini's first test, and it exists because of a bug that could not fail
    /// loudly: a row's `path` is not always a path, and everything downstream
    /// resolves it as one. When the encoding and the resolver disagree the only
    /// symptom is a long-press that does nothing at all.
    fn db_with(paths: &[(&str, &str)]) -> (std::path::PathBuf, Database) {
        // The counter matters: tests run in parallel threads of one process,
        // so the pid alone gives every test the same file to fight over.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let mut file = std::env::temp_dir();
        file.push(format!(
            "tunante-mini-test-{}-{}.db",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&file);
        let db = Database::new(&file).expect("open");
        for (path, album) in paths {
            // Written out rather than derived from a Default: if a field is
            // added to Track, this should stop compiling and be looked at.
            let t = Track {
                id: (*path).to_string(),
                path: (*path).to_string(),
                title: (*path).to_string(),
                artist: String::new(),
                album: (*album).to_string(),
                album_artist: String::new(),
                track_number: None,
                disc_number: None,
                duration_ms: 1000,
                sample_rate: None,
                channels: None,
                bitrate: None,
                codec: "test".into(),
                file_size: 0,
                has_artwork: false,
                rating: 0,
                modified_at: 0,
                ..Default::default()
            };
            db.insert_track(&t).expect("insert");
        }
        (file, db)
    }

    #[test]
    fn a_game_row_resolves_to_the_tracks_of_that_game() {
        let (file, db) = db_with(&[
            ("/m/FF7 Disco 1/a.psf", "Final Fantasy VII"),
            ("/m/FF7 Disco 2/b.psf", "Final Fantasy VII"),
            ("/m/otro/c.psf", "Chrono Trigger"),
        ]);
        let roots = vec![std::path::PathBuf::from("/m")];

        // What a long press on a game tile hands over.
        let got = tracks_for_path(&db, &roots, "juego:Final Fantasy VII", true);
        let mut paths: Vec<_> = got.iter().map(|t| t.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["/m/FF7 Disco 1/a.psf", "/m/FF7 Disco 2/b.psf"]);

        let _ = std::fs::remove_file(file);
    }

    /// The path the screen walks, which the resolver test above does not cover.
    ///
    /// Tapping a game pushes the grid cell's `path` onto `nav`, and from there
    /// two different things read it back: the breadcrumb and the track list.
    /// Both strip the prefix, and if either forgot to, entering a game would
    /// show an empty level or a crumb reading "juego:Xenogears" -- neither of
    /// which any test here would have noticed. mini has no way to be clicked
    /// from a script under Wayland, so this stands in for the click.
    #[test]
    fn entering_a_game_from_the_grid_shows_its_tracks_and_its_name() {
        let (file, db) = db_with(&[
            ("/m/disc1/a.psf", "Xenogears"),
            ("/m/disc2/b.psf", "Xenogears"),
            ("/m/otro/c.psf", "Chrono Cross"),
        ]);
        let mut tree = library::Tree::new(vec![std::path::PathBuf::from("/m")]);
        tree.mode = library::Mode::Games;

        // What the grid put on the cell, verbatim.
        tree.nav.push("juego:Xenogears".to_string());

        assert_eq!(tree.crumb(), "Xenogears");
        let rows = tree.grid_tracks(&db, library::Mode::Games);
        let mut paths: Vec<&str> = rows.iter().map(|r| r.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["/m/disc1/a.psf", "/m/disc2/b.psf"]);

        let _ = std::fs::remove_file(file);
    }

    /// A crumb is a name here, not a path, so it must not be cut at a slash.
    #[test]
    fn a_game_name_with_a_slash_survives_the_crumb() {
        let mut tree = library::Tree::new(vec![std::path::PathBuf::from("/m")]);
        tree.nav.push("juego:Hack//Sign".to_string());
        assert_eq!(tree.crumb(), "Hack//Sign");
    }

    /// The regression itself: without the prefix this returned nothing, because
    /// a game name is not a directory and never will be one.
    #[test]
    fn a_bare_game_name_is_not_mistaken_for_a_directory() {
        let (file, db) = db_with(&[("/m/x/a.psf", "Final Fantasy VII")]);
        let roots = vec![std::path::PathBuf::from("/m")];
        assert!(tracks_for_path(&db, &roots, "Final Fantasy VII", true).is_empty());
        let _ = std::fs::remove_file(file);
    }
}
