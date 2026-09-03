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
mod debuglog;
mod theme_watch;
mod integrate;
mod buttons;
mod shortcuts;
// logind, reached over D-Bus, and only `mpris` uses it.
#[cfg(target_os = "linux")]
mod inhibit;
mod library;
mod mpris;
mod output;
mod picker;
mod player;
mod single;
mod store;
mod tray;
mod update;
use tunante_core::session;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use slint::{Model, ModelRc, SharedString, VecModel};

use tunante_core::db::Database;

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    debuglog::install();

    // The old desktop's crash courtesy: a panic writes crash.log next to the
    // database and says so out loud, instead of a window that just vanishes.
    {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let msg = format!("{info}");
            let log = std::env::var_os("XDG_DATA_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|h| std::path::PathBuf::from(h).join(".local/share"))
                })
                .unwrap_or_else(std::env::temp_dir)
                .join("tunante-crash.log");
            let _ = std::fs::write(&log, &msg);
            #[cfg(target_os = "linux")]
            {
                let text = format!("Tunante se ha caído.\n\n{msg}\n\nDetalles en {}", log.display());
                let _ = std::process::Command::new("zenity")
                    .args(["--error", "--no-markup", "--text", &text])
                    .spawn()
                    .or_else(|_| {
                        std::process::Command::new("kdialog")
                            .args(["--error", &text])
                            .spawn()
                    });
            }
            default_hook(info);
        }));
    }

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

    // Headless self-update: check, install, exit. No window, no instance
    // claim — a cron job or an ssh session can keep a player fresh with one
    // order, and it doubles as the way to exercise the whole update path
    // without a compositor.
    if args.iter().any(|a| a == "--update") {
        let (tx, rx) = std::sync::mpsc::channel::<update::UpdateMsg>();
        eprintln!("comprobando (v{} local)…", update::CURRENT_VERSION);
        update::spawn_check(tx.clone());
        match rx.recv() {
            Ok(update::UpdateMsg::UpToDate) => eprintln!("al día"),
            Ok(update::UpdateMsg::Available { version, url }) => {
                eprintln!("v{version} disponible; descargando…");
                update::spawn_install(tx, version, url);
                match rx.recv() {
                    Ok(update::UpdateMsg::Installed(v)) => eprintln!("v{v} instalada"),
                    Ok(update::UpdateMsg::Error(e)) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                    _ => {}
                }
            }
            Ok(update::UpdateMsg::Error(e)) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
            _ => {}
        }
        return Ok(());
    }

    // One instance. A second launch delivers its intent to the first — the
    // file it was handed, or just "come to the front" — and exits.
    let instance = match single::claim(&match &open_target {
        Some(p) => format!("play {}\n", p.display()),
        None => "raise\n".to_string(),
    }) {
        single::Start::Primary(i) => i,
        single::Start::Secondary => {
            eprintln!("ya hay un Tunante abierto; le paso el encargo");
            return Ok(());
        }
    };

    let dbfile = store::resolve()?;
    let db = Database::new(&dbfile)?;

    if let Some(folder) = &scan_target {
        eprintln!("escaneando {}…", folder.display());
        let id = format!("{:x}", folder.to_string_lossy().len());
        let _ = db.add_monitored_folder(&id, &folder.to_string_lossy());

        let mut last_shown = 0usize;
        let added = tunante_helper::scan::scan_folder_with(&db, folder, &probe_opts(&db), |p| {
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
    // the desktop shell), the setting remembers a forced choice, and the
    // flags win over both for development and scripts. All of it before the
    // window maps: AppWindow's preferred size keys off `ui-mode`, so a
    // remembered desktop mode opens desktop-sized.
    let saved_ui_mode = db
        .get_setting("mini.ui_mode")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|m| (0..=2).contains(m))
        .unwrap_or(0);
    ui.set_ui_mode(saved_ui_mode);
    if args.iter().any(|a| a == "--desktop") {
        ui.set_ui_mode(2);
    } else if args.iter().any(|a| a == "--mini") {
        ui.set_ui_mode(1);
    }
    ui.set_ui_mode_label(SharedString::from(ui_mode_label(ui.get_ui_mode())));

    // The palette always had both values; this is just the switch. Persisted
    // under the desktop's key, so the unified database keeps one opinion.
    // Three states now: dark, light, or follow the system through the portal.
    theme_watch::start();
    let theme_mode = Rc::new(std::cell::Cell::new(
        match db.get_setting("theme").ok().flatten().as_deref() {
            Some("light") => 1u8,
            Some("system") => 2,
            _ => 0,
        },
    ));
    let dark = match theme_mode.get() {
        1 => false,
        2 => theme_watch::prefers_dark().unwrap_or(true),
        _ => true,
    };
    ui.global::<Theme>().set_dark(dark);
    ui.set_theme_label(SharedString::from(theme_mode_label(theme_mode.get())));

    // The tray runs its own GTK thread; clicks come back through tray::poll()
    // in the UI timer, beside the MPRIS commands they resemble.
    let tray_style = match db
        .get_setting("tray_icon_style")
        .ok()
        .flatten()
        .as_deref()
    {
        Some("symbolic") => 1u8,
        Some("logo") => 2,
        _ => 0,
    };
    tray::spawn(tray_style);
    ui.set_tray_style_label(SharedString::from(tray_style_label(tray_style)));

    if focus_search {
        ui.set_autofocus_search(true);
        ui.set_tab(2);
    }

    // Same reason --mode exists: this compositor refuses synthetic clicks, so
    // "does the settings screen draw right?" is only answerable by starting on
    // it. 0 Sonando · 1 Cola · 2 Biblioteca · 3 Ajustes.
    if let Some(tab) = arg_value("--tab").and_then(|s| s.parse::<i32>().ok()) {
        ui.set_tab(tab.clamp(0, 3));
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
    // The expanded folders survive the restart, under the old key.
    if let Ok(Some(saved)) = db.get_setting("files_expanded_folders") {
        tree.borrow_mut()
            .set_expanded_paths(saved.split('\n').map(str::to_string));
    }
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
    // Set by the cover-download worker, acted on by the UI timer.
    //
    // The cache is an `Rc` owned by the UI thread and the download runs on its
    // own, so this is the handover. It matters because the cache remembers
    // *misses* too: without clearing it, every folder the run just gave a cover
    // to keeps showing the placeholder until the app restarts.
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

    // --- The watcher: the scan that never ends -------------------------------
    //
    // The machinery lives in `tunante_helper::watch`; this app plugs in its
    // two opinions: the shared static extension list, and a probe-based
    // re-read — out of process, like everything else it decodes. The handler
    // runs on the watcher's thread with its own connection (WAL lets the two
    // coexist); the UI hears about it through a flag the timer drains.
    let library_dirty = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let dirty = library_dirty.clone();
        let dbfile_w = dbfile.clone();
        let mut wdb: Option<Database> = None;
        Rc::new(RefCell::new(tunante_helper::watch::FolderWatcher::new(
            |p| tunante_core::vgm_path::is_audio_file(p),
            move |change, path| {
                if wdb.is_none() {
                    wdb = Database::new(&dbfile_w).ok();
                }
                let Some(db) = wdb.as_ref() else { return };
                let path_str = path.to_string_lossy().to_string();
                match change {
                    tunante_helper::watch::FileChange::Modified => {
                        // Probe first: a read that fails must not cost the
                        // rows we already had. Same knobs as the scanner, read
                        // from this thread's own connection.
                        let Ok(values) = tunante_helper::probe_with(
                            path,
                            tunante_helper::scan::PROBE_TIMEOUT,
                            &probe_opts(db),
                        ) else {
                            return;
                        };
                        let _ = db.remove_tracks_by_base_path(&path_str);
                        for v in values {
                            if let Ok(track) =
                                serde_json::from_value::<tunante_core::db::models::Track>(v)
                            {
                                let _ = db.insert_track(&track);
                            }
                        }
                    }
                    tunante_helper::watch::FileChange::Removed => {
                        let _ = db.remove_tracks_by_base_path(&path_str);
                    }
                }
                dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            },
        )))
    };
    // Watch every monitored folder that asks for it. Re-run after any scan:
    // start_watching an already-watched path is a cheap re-watch, and a scan
    // is exactly when the folder list can have grown.
    let sync_watches = {
        let (db, watcher) = (db.clone(), watcher.clone());
        Rc::new(move || {
            let mut w = watcher.borrow_mut();
            let folders = db.get_monitored_folders().unwrap_or_default();
            // Both directions: a folder removed or un-watched in Ajustes
            // stops being listened to, not just new ones starting.
            for path in w.watched() {
                let keep = folders
                    .iter()
                    .any(|f| f.path == path && f.watching_enabled);
                if !keep {
                    let _ = w.stop_watching(&path);
                }
            }
            for f in &folders {
                if f.watching_enabled {
                    if let Err(e) = w.start_watching(&f.path) {
                        eprintln!("no se pudo vigilar {}: {e}", f.path);
                    }
                }
            }
        })
    };
    sync_watches();

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
                    let _ = tunante_helper::scan::scan_folder_with(&db, &folder, &probe_opts(&db), |p| {
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

    // Re-apply the remembered output device. Best-effort on purpose: if the
    // device is gone the engine already falls back to the system default, and
    // the label still shows what was asked for so the user can see why.
    {
        let stored = db
            .get_setting("audio_output_device")
            .ok()
            .flatten()
            .unwrap_or_else(|| "system".to_string());
        if stored != "system" {
            let sel = tunante_audio::OutputSelection::from_setting(&stored);
            if let Some(p) = player.borrow_mut().as_mut() {
                if let Err(e) = p.engine_mut().set_output_selection(sel) {
                    eprintln!("no se pudo abrir la salida guardada ({stored}): {e}");
                }
            }
        }
        ui.set_output_label(SharedString::from(output_label(&stored)));
    }

    // The crossfade: same keys the desktop persisted. The timer only exists
    // while a fade is in flight — the kick starts it, Idle stops it, so the
    // phone never pays 40 wakeups a second for a feature at rest.
    let crossfade_secs: i32 = db
        .get_setting("fade_seconds")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v.round() as i32)
        .unwrap_or(2)
        .clamp(0, 10);
    let crossfade_on = db
        .get_setting("fade_on_track_change")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    ui.set_crossfade_secs(if crossfade_on { crossfade_secs } else { 0 });
    {
        let fade_timer = Rc::new(slint::Timer::default());
        if let Some(p) = player.borrow_mut().as_mut() {
            let engine = p.engine_mut();
            engine.set_fade_on_track_change(crossfade_on);
            engine.set_fade_seconds(crossfade_secs as f32);
        }
        let (timer, player_k) = (fade_timer.clone(), player.clone());
        let kick = move || {
            let (timer_inner, player) = (timer.clone(), player_k.clone());
            timer.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_millis(25),
                move || {
                    let active = player
                        .borrow_mut()
                        .as_mut()
                        .map(|p| p.tick_fade())
                        .unwrap_or(false);
                    if !active {
                        timer_inner.stop();
                    }
                },
            );
        };
        if let Some(p) = player.borrow_mut().as_mut() {
            p.set_fade_kick(kick);
        }
    }

    // The DSP chain: same JSON, same `dsp_config` key the desktop persists,
    // so when the two databases become one the equalizer simply carries over.
    let dsp_config = Rc::new(RefCell::new(
        db.get_setting("dsp_config")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<tunante_core::dsp::DspConfig>(&s).ok())
            .unwrap_or_default(),
    ));
    let push_dsp_ui = |ui: &AppWindow, c: &tunante_core::dsp::DspConfig| {
        ui.set_eq_enabled(c.eq_enabled);
        ui.set_eq_low(c.eq_low_db);
        ui.set_eq_mid(c.eq_mid_db);
        ui.set_eq_high(c.eq_high_db);
        ui.set_preamp_db(c.preamp_db);
        ui.set_dsp_mono(c.mono);
        ui.set_dsp_mono_compensate(c.mono_compensate);
        ui.set_dsp_mono_phase(c.mono_phase_safe);
        ui.set_dsp_limiter(c.limiter);
        ui.set_dsp_balance(c.balance);
        ui.set_dsp_width(c.width);
    };
    {
        let c = dsp_config.borrow();
        if let Some(p) = player.borrow_mut().as_mut() {
            c.apply_to(p.engine_mut().dsp());
        }
        push_dsp_ui(&ui, &c);
    }
    ui.set_rating_priority_label(SharedString::from(rating_priority_label(
        db.get_setting("rating_source_priority")
            .ok()
            .flatten()
            .as_deref(),
    )));

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
    if let Some(p) = player.borrow_mut().as_mut() {
        p.set_continue_from_queue(
            db.get_setting("continue_from_queue")
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(false),
        );
    }
    ui.set_continue_queue(
        db.get_setting("continue_from_queue")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false),
    );
    ui.set_loop_max_mins(
        db.get_setting("loop_max_seconds")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i32>().ok())
            .map(|s| s / 60)
            .unwrap_or(0),
    );
    ui.set_slow_scan(
        db.get_setting("fast_scan")
            .ok()
            .flatten()
            .map(|v| v == "false")
            .unwrap_or(false),
    );
    ui.set_short_filter_secs({
        let secs: i64 = db
            .get_setting("mini.short_filter_secs")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if let Some(p) = player.borrow_mut().as_mut() {
            p.set_short_filter(secs * 1000);
        }
        secs as i32
    });
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
        if let Some(v) = db
            .get_setting("vgm_loop_count")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<f64>().ok())
        {
            p.engine_mut().set_vgm_loop_count(v);
        }
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
                // Opt-in resume: only if the user asked, the app was playing
                // when it went away, and that was less than five minutes ago.
                let resume = db
                    .get_setting("resume_playback_on_open")
                    .ok()
                    .flatten()
                    .map(|v| v == "true")
                    .unwrap_or(false)
                    && db.get_setting("mini.was_playing").ok().flatten().as_deref() == Some("true")
                    && db
                        .get_setting("mini.closed_at")
                        .ok()
                        .flatten()
                        .and_then(|v| v.parse::<u64>().ok())
                        .zip(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .ok()
                                .map(|d| d.as_secs()),
                        )
                        .is_some_and(|(closed, now)| now.saturating_sub(closed) < 300);
                if let Some(p) = player.borrow_mut().as_mut() {
                    p.set_tracks(tracks.clone());
                    if p.play_index(start).is_ok() {
                        if !resume {
                            p.toggle_play();           // straight to paused
                        }
                        p.seek(saved.position_ms);
                        push_now_playing(&ui, p);
                    }
                }
                queue_model.set_vec(to_queue_rows(&tracks, Some(start)));
            }
        }
    }

    // Handed a file on the command line: play it, and queue its folder around
    // it, which is what anyone opening one track of an album expects. The
    // same door a second launch's `play` message comes through.
    if let Some(path) = &open_target {
        play_from_path(&ui, &db, &player, &queue_model, &path.to_string_lossy());
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
                // A folder picked out of a search gets revealed — ancestors
                // and all — and the search clears so the tree can show it.
                if !ui.get_search().is_empty() {
                    tree.borrow_mut().reveal(&path);
                    ui.set_search(SharedString::new());
                } else {
                    tree.borrow_mut().toggle(&path);
                }
                let _ = db.set_setting(
                    "files_expanded_folders",
                    &tree.borrow().expanded_list().join("\n"),
                );
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
                    show_play_error(&ui, &e);
                    return;
                }
                push_now_playing(&ui, p);
            }
            queue_model.set_vec(to_queue_rows(&tracks, Some(start)));
        });
    }

    // --- The desktop shell's track table -------------------------------------
    //
    // Built lazily on `table-needed`: the phone never instantiates the pane, so
    // it never pays for a whole-library model. Rust owns sort and filter — the
    // UI only reports which header or row was touched — and the full
    // sort-and-rebuild was measured at 11–21 ms over 30k rows in the spike.
    let table_model = Rc::new(VecModel::from(Vec::<TableRow>::new()));
    ui.set_table_rows(ModelRc::from(table_model.clone()));
    let table_state = Rc::new(RefCell::new(TableState::default()));
    // The search box's latest text, flushed to the database by the timer —
    // a write per keystroke would be noise, the old app debounced too.
    let pending_search: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let columns_model = Rc::new(VecModel::from(Vec::<TableColumn>::new()));
    let choices_model = Rc::new(VecModel::from(Vec::<ColumnChoice>::new()));
    ui.set_table_columns(ModelRc::from(columns_model.clone()));
    ui.set_table_column_choices(ModelRc::from(choices_model.clone()));
    {
        // Which columns, remembered. Unknown keys (an old build's) drop out.
        let mut st = table_state.borrow_mut();
        if let Ok(Some(saved)) = db.get_setting("mini.table_columns") {
            // Each entry is `key` or `key:weight` — the saved order IS the
            // display order, and a weight is a hand-resized width.
            let mut keys = Vec::new();
            for item in saved.split(',') {
                let (key, weight) = match item.split_once(':') {
                    Some((k, w)) => (k, w.parse::<f32>().ok()),
                    None => (item, None),
                };
                if !TABLE_COLUMNS.iter().any(|d| d.key == key) {
                    continue;
                }
                keys.push(key.to_string());
                if let Some(w) = weight.filter(|w| *w > 0.0) {
                    st.widths.insert(key.to_string(), w);
                }
            }
            if !keys.is_empty() {
                st.visible = keys;
            }
        }
        st.album_game_prefers_game = db
            .get_setting("album_game_prefers")
            .ok()
            .flatten()
            .map(|v| v == "game")
            .unwrap_or(false);
        // The search text survives too, under the desktop's key.
        if let Ok(Some(q)) = db.get_setting("search_query") {
            if !q.is_empty() {
                st.filter = q.clone();
                ui.set_table_filter(SharedString::from(q));
            }
        }
        // The sort survives the restart too — the desktop's session keys.
        if let Ok(Some(k)) = db.get_setting("session_sort_column") {
            if TABLE_COLUMNS.iter().any(|d| d.key == k) {
                st.sort_key = k;
            }
        }
        if let Ok(Some(d)) = db.get_setting("session_sort_direction") {
            st.asc = d != "desc";
        }
        ui.set_table_sort_asc(st.asc);
        rebuild_columns(&ui, &st, &columns_model, &choices_model);
        ui.set_table_sort_col(
            st.visible.iter().position(|k| k == &st.sort_key).map(|i| i as i32).unwrap_or(-1),
        );
    }

    {
        let (db_t, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_table_needed(move || {
            let mut st = st.borrow_mut();
            if st.built {
                return;
            }
            st.built = true;
            st.all = db_t.get_all_tracks().unwrap_or_default();
            rebuild_table(&mut st, &model);
            if let Some(ui) = weak.upgrade() {
                ui.set_table_sort_col(
                    st.visible
                        .iter()
                        .position(|k| k == &st.sort_key)
                        .map(|i| i as i32)
                        .unwrap_or(-1),
                );
                ui.set_table_sort_asc(st.asc);
            }
        });
    }
    {
        let (st, model) = (table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        let db_s = db.clone();
        ui.on_table_sorted(move |col| {
            let mut st = st.borrow_mut();
            let Some(key) = st.visible.get(col as usize).cloned() else {
                return;
            };
            st.asc = if st.sort_key == key { !st.asc } else { true };
            st.sort_key = key;
            let _ = db_s.set_setting("session_sort_column", &st.sort_key);
            let _ = db_s.set_setting(
                "session_sort_direction",
                if st.asc { "asc" } else { "desc" },
            );
            rebuild_table(&mut st, &model);
            if let Some(ui) = weak.upgrade() {
                ui.set_table_sort_col(col);
                ui.set_table_sort_asc(st.asc);
            }
        });
    }
    {
        let (st, model) = (table_state.clone(), table_model.clone());
        let (cols, choices, db_c) = (columns_model.clone(), choices_model.clone(), db.clone());
        let weak = ui.as_weak();
        ui.on_table_column_toggled(move |key| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            let key = key.to_string();
            if st.visible.iter().any(|k| *k == key) {
                // Never down to zero: a table with no columns is a mistake,
                // not a preference.
                if st.visible.len() > 1 {
                    st.visible.retain(|k| *k != key);
                }
            } else {
                // Arrives at the end; the user can drag it where they want.
                st.visible.push(key.clone());
            }
            let _ = db_c.set_setting("mini.table_columns", &persist_columns(&st));
            // A hidden sort column falls back to the title rather than
            // pointing at nothing.
            if !st.visible.iter().any(|k| k == &st.sort_key) {
                st.sort_key = "title".to_string();
            }
            rebuild_columns(&ui, &st, &cols, &choices);
            rebuild_table(&mut st, &model);
            ui.set_table_sort_col(
                st.visible
                    .iter()
                    .position(|k| k == &st.sort_key)
                    .map(|i| i as i32)
                    .unwrap_or(-1),
            );
        });
    }
    {
        let (db_c, st) = (db.clone(), table_state.clone());
        let (cols, choices, model) = (
            columns_model.clone(),
            choices_model.clone(),
            table_model.clone(),
        );
        let weak = ui.as_weak();
        ui.on_table_column_moved(move |from, to| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            let (from, to) = (from as usize, to as usize);
            if from >= st.visible.len() || to >= st.visible.len() {
                return;
            }
            let key = st.visible.remove(from);
            st.visible.insert(to, key);
            let _ = db_c.set_setting("mini.table_columns", &persist_columns(&st));
            rebuild_columns(&ui, &st, &cols, &choices);
            // The cells travel with their headers.
            rebuild_table(&mut st, &model);
            ui.set_table_sort_col(
                st.visible
                    .iter()
                    .position(|k| k == &st.sort_key)
                    .map(|i| i as i32)
                    .unwrap_or(-1),
            );
        });
    }

    // Resize: the drag reports a cumulative offset from the press, so the
    // maths always starts from the weights snapshotted at the press — never
    // incremental, which is the difference between a steady column edge and
    // one that drifts under the pointer.
    let resize_base: Rc<RefCell<Option<(usize, f32, f32)>>> = Rc::new(RefCell::new(None));
    {
        let (st, base) = (table_state.clone(), resize_base.clone());
        ui.on_table_column_resize_started(move |ci| {
            let st = st.borrow();
            let ci = ci as usize;
            if ci + 1 >= st.visible.len() {
                // The last column has no right neighbour to trade width
                // with; its edge is the table's edge.
                *base.borrow_mut() = None;
                return;
            }
            let weight = |k: &str| {
                st.widths.get(k).copied().unwrap_or_else(|| {
                    TABLE_COLUMNS
                        .iter()
                        .find(|d| d.key == k)
                        .map(|d| d.fraction)
                        .unwrap_or(1.0)
                })
            };
            *base.borrow_mut() =
                Some((ci, weight(&st.visible[ci]), weight(&st.visible[ci + 1])));
        });
    }
    {
        let (st, base, cols) = (table_state.clone(), resize_base.clone(), columns_model.clone());
        ui.on_table_column_resized(move |_ci, dx_px, total_px| {
            let Some((ci, w_a, w_b)) = *base.borrow() else { return };
            if total_px <= 0.0 {
                return;
            }
            let mut st = st.borrow_mut();
            // Weights and shares are proportional: the sum of all weights
            // maps onto total_px, so a pixel delta converts through it.
            let total_weight: f32 = st
                .visible
                .iter()
                .map(|k| {
                    st.widths.get(k).copied().unwrap_or_else(|| {
                        TABLE_COLUMNS
                            .iter()
                            .find(|d| d.key == k)
                            .map(|d| d.fraction)
                            .unwrap_or(1.0)
                    })
                })
                .sum();
            // 40px minimum, the old desktop's floor.
            let min_w = 40.0 / total_px * total_weight;
            let pair = w_a + w_b;
            let new_a = (w_a + dx_px / total_px * total_weight)
                .clamp(min_w, (pair - min_w).max(min_w));
            let new_b = pair - new_a;
            let (key_a, key_b) = (st.visible[ci].clone(), st.visible[ci + 1].clone());
            st.widths.insert(key_a, new_a);
            st.widths.insert(key_b, new_b);
            // Live: only the two touched fractions move, in place.
            for (i, w) in [(ci, new_a), (ci + 1, new_b)] {
                if let Some(mut c) = cols.row_data(i) {
                    c.fraction = w / total_weight.max(0.001);
                    cols.set_row_data(i, c);
                }
            }
        });
    }
    {
        let (db_c, st, base) = (db.clone(), table_state.clone(), resize_base.clone());
        ui.on_table_column_resize_done(move || {
            if base.borrow_mut().take().is_some() {
                let st = st.borrow();
                let _ = db_c.set_setting("mini.table_columns", &persist_columns(&st));
            }
        });
    }
    {
        let (st, model) = (table_state.clone(), table_model.clone());
        let pending = pending_search.clone();
        ui.on_table_filter_changed(move |s| {
            *pending.borrow_mut() = Some(s.to_string());
            let mut st = st.borrow_mut();
            st.filter = s.to_string();
            rebuild_table(&mut st, &model);
        });
    }
    {
        let (st, player_t, queue_model_t) = (
            table_state.clone(),
            player.clone(),
            queue_model.clone(),
        );
        let weak = ui.as_weak();
        ui.on_table_activated(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let tracks = st.borrow().tracks.clone();
            let index = index as usize;
            if index >= tracks.len() {
                return;
            }
            // The table's visible order — filtered, sorted — becomes the queue,
            // which is what double-clicking a row in any desktop player means.
            if let Some(p) = player_t.borrow_mut().as_mut() {
                p.set_tracks(tracks.clone());
                if let Err(e) = p.play_index(index) {
                    show_play_error(&ui, &e);
                    return;
                }
                push_now_playing(&ui, p);
            }
            queue_model_t.set_vec(to_queue_rows(&tracks, Some(index)));
        });
    }

    {
        let (db_t, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_table_rated(move |index, stars| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            let Some(track) = st.tracks.get(index as usize) else { return };
            // Clicking the star it already has clears it — the second tap on a
            // toggle, not a way to be stuck at one star forever.
            let new = if track.rating == stars { 0 } else { stars };
            let (id, path) = (track.id.clone(), track.path.clone());
            if let Err(e) = db_t.set_track_rating(&id, new) {
                eprintln!("no se pudo guardar la puntuación: {e}");
                return;
            }
            // The disk half, through the helper — the same priority order the
            // desktop honours, read from the same setting. Off this thread:
            // writing a tag can mean rewriting the file.
            let order = db_t
                .get_setting("rating_source_priority")
                .ok()
                .flatten();
            {
                let path = path.clone();
                std::thread::spawn(move || {
                    if let Err(e) = tunante_helper::rate(
                        std::path::Path::new(&path),
                        new,
                        order.as_deref(),
                        std::time::Duration::from_secs(20),
                    ) {
                        eprintln!("no se pudo escribir la puntuación en disco: {e}");
                    }
                });
            }
            for t in st.all.iter_mut().filter(|t| t.path == path) {
                t.rating = new;
            }
            rebuild_table(&mut st, &model);
            // The transport's stars follow when the rated row is the one
            // playing, and Favoritos' number moves either way.
            if ui.get_now_path() == path.as_str() {
                ui.set_now_rating(new);
                ui.set_now_stars(SharedString::from(stars_for(new)));
            }
            refresh_counts(&db_t, &ui);
        });
    }
    {
        let (st, player_t) = (table_state.clone(), player.clone());
        ui.on_table_enqueued(move |index| {
            let batch = {
                let st = st.borrow();
                let i = index as usize;
                if st.selected.len() > 1 && st.selected.contains(&i) {
                    // The visible order, not click order: "and then these".
                    let mut idx: Vec<usize> = st.selected.iter().copied().collect();
                    idx.sort_unstable();
                    idx.iter().filter_map(|&j| st.tracks.get(j).cloned()).collect()
                } else {
                    st.tracks.get(i).cloned().into_iter().collect::<Vec<_>>()
                }
            };
            if batch.is_empty() {
                return;
            }
            if let Some(p) = player_t.borrow_mut().as_mut() {
                p.enqueue_many(batch);
            }
        });
    }
    {
        let (st, player_t, queue_model_t) = (
            table_state.clone(),
            player.clone(),
            queue_model.clone(),
        );
        ui.on_table_play_next(move |index| {
            let batch = {
                let st = st.borrow();
                let i = index as usize;
                if st.selected.len() > 1 && st.selected.contains(&i) {
                    let mut idx: Vec<usize> = st.selected.iter().copied().collect();
                    idx.sort_unstable();
                    idx.iter().filter_map(|&j| st.tracks.get(j).cloned()).collect()
                } else {
                    st.tracks.get(i).cloned().into_iter().collect::<Vec<_>>()
                }
            };
            if batch.is_empty() {
                return;
            }
            if let Some(p) = player_t.borrow_mut().as_mut() {
                p.play_next(batch);
                refresh_queue(p, &queue_model_t);
            }
        });
    }
    {
        let (st, player_m) = (table_state.clone(), player.clone());
        let queue_model_m = queue_model.clone();
        ui.on_table_row_middle_clicked(move |index| {
            let track = {
                let st = st.borrow();
                st.tracks.get(index as usize).cloned()
            };
            let Some(track) = track else { return };
            if let Some(p) = player_m.borrow_mut().as_mut() {
                // Toggle: already waiting in the user queue → out; not
                // there → in. The old desktop's middle-click.
                let pos = p.user_queue().iter().position(|t| t.path == track.path);
                match pos {
                    Some(i) => {
                        let _ = p.dequeue_user(i);
                    }
                    None => p.play_next(vec![track]),
                }
                refresh_queue(p, &queue_model_m);
            }
        });
    }
    // Double-click on a sidebar collection plays it — random start when
    // shuffle is on, like the old desktop.
    fn play_collection(
        ui: &AppWindow,
        player: &Rc<RefCell<Option<player::Player>>>,
        queue_model: &VecModel<QueueRow>,
        tracks: Vec<tunante_core::db::models::Track>,
    ) {
        if tracks.is_empty() {
            return;
        }
        if let Some(p) = player.borrow_mut().as_mut() {
            let start = if p.shuffle() {
                (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as usize)
                    .unwrap_or(0))
                    % tracks.len()
            } else {
                0
            };
            p.set_tracks(tracks.clone());
            match p.play_index(start) {
                Ok(()) => push_now_playing(ui, p),
                Err(e) => show_play_error(ui, &e),
            }
            queue_model.set_vec(to_queue_rows(&tracks, Some(start)));
        }
    }
    {
        let (db_p, st, player_p) = (db.clone(), table_state.clone(), player.clone());
        let queue_model_p = queue_model.clone();
        let weak = ui.as_weak();
        ui.on_sidebar_play_all(move || {
            let Some(ui) = weak.upgrade() else { return };
            let tracks = {
                let mut st = st.borrow_mut();
                if !st.built {
                    st.built = true;
                    st.all = db_p.get_all_tracks().unwrap_or_default();
                }
                if st.tracks.is_empty() { st.all.clone() } else { st.tracks.clone() }
            };
            play_collection(&ui, &player_p, &queue_model_p, tracks);
        });
    }
    {
        let (db_p, player_p, queue_model_p) = (db.clone(), player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_playlist_played(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let tracks = db_p.get_playlist_tracks(&id).unwrap_or_default();
            play_collection(&ui, &player_p, &queue_model_p, tracks);
        });
    }
    {
        let (db_p, player_p, queue_model_p) = (db.clone(), player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_folder_played(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let folder = sidebar_folder_path(&db_p, &id);
            let Some(folder) = folder else { return };
            let tracks: Vec<_> = db_p
                .get_all_tracks()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| {
                    let (real, _) = tunante_core::vgm_path::parse_vgm_path(&t.path);
                    real.strip_prefix(folder.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                })
                .collect();
            play_collection(&ui, &player_p, &queue_model_p, tracks);
        });
    }
    {
        let st = table_state.clone();
        ui.on_table_open_folder(move |index| {
            let path = {
                let st = st.borrow();
                st.tracks.get(index as usize).map(|t| t.path.clone())
            };
            let Some(path) = path else { return };
            let (real, _) = tunante_core::vgm_path::parse_vgm_path(&path);
            // FileManager1 selects the file itself; the xdg-open fallback
            // inside only opens the folder when nobody answers the bus.
            integrate::reveal(std::path::Path::new(real));
        });
    }
    // The one channel every art/metadata worker reports through; the timer
    // drains it. Created here because reclassification's suggestions worker
    // is the earliest sender.
    let (cover_tx, cover_rx) = std::sync::mpsc::channel::<CoverMsg>();

    // --- Reclassification --------------------------------------------------
    //
    // The catalog, "(automática)" first: an empty id means "let the rules
    // decide", which set_override turns into clearing the correction.
    {
        ui.set_consoles(ModelRc::new(VecModel::from(consoles_for_filter(""))));
    }
    {
        let weak = ui.as_weak();
        ui.on_reclass_console_filter_changed(move |q| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_consoles(ModelRc::new(VecModel::from(consoles_for_filter(&q))));
        });
    }
    let sugg_model = Rc::new(VecModel::from(Vec::<SharedString>::new()));
    ui.set_reclass_suggestions(ModelRc::from(sugg_model.clone()));
    // (folder target, track target), captured when the sheet opens so a
    // re-sorted table cannot change what Guardar means.
    let reclass_target: Rc<RefCell<Option<(String, String)>>> = Rc::new(RefCell::new(None));

    {
        let (st, target, sugg) = (
            table_state.clone(),
            reclass_target.clone(),
            sugg_model.clone(),
        );
        let weak = ui.as_weak();
        ui.on_table_reclassify_requested(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let st = st.borrow();
            let Some(t) = st.tracks.get(index as usize) else { return };
            let (real, _) = tunante_core::vgm_path::parse_vgm_path(&t.path);
            let folder = std::path::Path::new(real)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            *target.borrow_mut() = Some((folder.clone(), t.path.clone()));
            let folder_name = std::path::Path::new(&folder)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(folder);
            ui.set_reclass_heading(SharedString::from(folder_name));
            ui.set_reclass_scope_folder(true);
            ui.set_reclass_console_filter(SharedString::new());
            ui.set_consoles(ModelRc::new(VecModel::from(consoles_for_filter(""))));
            ui.set_reclass_console(SharedString::from(t.console_id.as_str()));
            ui.set_reclass_game(SharedString::from(t.game.as_str()));
            sugg.set_vec(Vec::new());
            ui.set_reclassifying(true);
        });
    }
    let reclass_gen = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    {
        let (st, sugg, tx, gen) = (
            table_state.clone(),
            sugg_model.clone(),
            cover_tx.clone(),
            reclass_gen.clone(),
        );
        let weak = ui.as_weak();
        ui.on_reclass_game_edited(move |raw| {
            let Some(ui) = weak.upgrade() else { return };
            // The library answers instantly, so a correction lands on the
            // spelling the collection uses; the archive and Steam answer over
            // the channel, generation-stamped so typing outruns them safely.
            let q = library::plegar(&raw);
            let mut out: Vec<SharedString> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let mut library_names: Vec<String> = Vec::new();
            if q.len() >= 2 {
                for t in st.borrow().all.iter() {
                    if t.game.is_empty() {
                        continue;
                    }
                    if seen.insert(t.game.to_lowercase()) {
                        library_names.push(t.game.clone());
                        if library::plegar(&t.game).contains(&q) && out.len() < 8 {
                            out.push(SharedString::from(t.game.as_str()));
                        }
                    }
                }
            }
            sugg.set_vec(out);
            if q.len() >= 2 {
                let generation =
                    gen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                spawn_reclass_suggest(
                    tx.clone(),
                    generation,
                    ui.get_reclass_console().to_string(),
                    raw.to_string(),
                    library_names,
                );
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_reclass_cancelled(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_reclassifying(false);
            }
        });
    }
    {
        let (db_r, st, model, target) = (
            db.clone(),
            table_state.clone(),
            table_model.clone(),
            reclass_target.clone(),
        );
        let (tree_r, views_r) = (tree.clone(), views.clone());
        let weak = ui.as_weak();
        ui.on_reclass_accepted(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some((folder, track)) = target.borrow_mut().take() else {
                ui.set_reclassifying(false);
                return;
            };
            let (scope, target_path) = if ui.get_reclass_scope_folder() {
                ("folder", folder)
            } else {
                ("track", track)
            };
            let console = ui.get_reclass_console().to_string();
            let game = ui.get_reclass_game().to_string();

            // set_override re-derives every affected row itself, and an
            // all-empty correction becomes a clear — core's rules, reused.
            if let Err(e) = db_r.set_override(
                &uuid::Uuid::new_v4().to_string(),
                scope,
                &target_path,
                Some(&console),
                Some(&game),
            ) {
                eprintln!("no se pudo guardar la corrección: {e}");
                ui.set_reclassifying(false);
                return;
            }

            // Derived columns changed under the caches: re-read, re-cut.
            {
                let mut st = st.borrow_mut();
                st.all = db_r.get_all_tracks().unwrap_or_default();
                rebuild_table(&mut st, &model);
            }
            refresh_library(&ui, &tree_r, &db_r, &views_r);
            ui.set_reclassifying(false);
        });
    }

    // --- The cover picker ----------------------------------------------------
    //
    // Network on worker threads, results through a channel the timer drains —
    // the same shape as the scanner. Slint cannot show a URL, so the worker
    // downloads the thumbnails too; the UI thread only decodes.
    let cover_target: Rc<RefCell<Option<tunante_core::db::models::Track>>> =
        Rc::new(RefCell::new(None));
    let cover_urls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let cover_model = Rc::new(VecModel::from(Vec::<CoverCandidate>::new()));
    ui.set_cover_candidates(ModelRc::from(cover_model.clone()));

    {
        let (st, target, tx) = (table_state.clone(), cover_target.clone(), cover_tx.clone());
        let (urls, model) = (cover_urls.clone(), cover_model.clone());
        let weak = ui.as_weak();
        ui.on_table_cover_requested(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let t = st.borrow().tracks.get(index as usize).cloned();
            let Some(t) = t else { return };
            ui.set_cover_heading(SharedString::from(if t.game.is_empty() {
                t.album.clone()
            } else {
                t.game.clone()
            }));
            ui.set_cover_query(SharedString::new());
            ui.set_cover_status(SharedString::from("Buscando…"));
            urls.borrow_mut().clear();
            model.set_vec(Vec::new());
            *target.borrow_mut() = Some(t.clone());
            ui.set_covering(true);
            spawn_cover_search(tx.clone(), t, None);
        });
    }
    {
        let (target, tx) = (cover_target.clone(), cover_tx.clone());
        let weak = ui.as_weak();
        ui.on_cover_search(move |q| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(t) = target.borrow().clone() else { return };
            ui.set_cover_status(SharedString::from("Buscando…"));
            let q = q.trim().to_string();
            spawn_cover_search(tx.clone(), t, (!q.is_empty()).then_some(q));
        });
    }
    {
        let (target, dirty) = (cover_target.clone(), std::sync::Arc::clone(&art_dirty));
        let (db_c, tx) = (db.clone(), cover_tx.clone());
        let weak = ui.as_weak();
        ui.on_cover_auto(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(t) = target.borrow().clone() else { return };
            ui.set_cover_status(SharedString::from("Descargando la mejor…"));
            let store = db_c
                .get_setting("store_covers_in_folder")
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(false);
            let (tx, dirty) = (tx.clone(), std::sync::Arc::clone(&dirty));
            std::thread::spawn(move || {
                // The old refetch_cover: forget what the cache thinks, take
                // the archive's best answer, apply it over what exists.
                let req = cover_request_for(&t, store);
                let resolver = cover_resolver();
                resolver.forget(&req);
                let opts = tunante_art::resolver::BulkOptions {
                    min_confidence: tunante_art::Confidence::High,
                    overwrite: tunante_art::folder::Overwrite::Replace,
                    ..Default::default()
                };
                let plans = resolver.resolve_many(vec![req], &opts, |_| {});
                let ok = plans
                    .first()
                    .is_some_and(|p| p.written.is_some() || p.existing.is_some());
                dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = tx.send(CoverMsg::Status(
                    if ok {
                        "Aplicada la mejor del archivo.".to_string()
                    } else {
                        "Nada con confianza suficiente; elige a mano.".to_string()
                    },
                ));
            });
        });
    }
    {
        let (target, urls, tx) = (cover_target.clone(), cover_urls.clone(), cover_tx.clone());
        let weak = ui.as_weak();
        ui.on_cover_chosen(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(t) = target.borrow().clone() else { return };
            let Some(url) = urls.borrow().get(index as usize).cloned() else {
                return;
            };
            ui.set_cover_status(SharedString::from("Descargando…"));
            let tx = tx.clone();
            std::thread::spawn(move || {
                // With the folder as destination: choosing is the user
                // deciding, so this write replaces — same rules as the
                // desktop's choose_cover.
                let req = cover_request_for(&t, true);
                match cover_resolver().fetch_chosen(&req, &url) {
                    Ok(_) => {
                        let _ = tx.send(CoverMsg::Saved);
                    }
                    Err(e) => {
                        let _ = tx.send(CoverMsg::Status(format!("No se pudo guardar: {e}")));
                    }
                }
            });
        });
    }

    // --- Track names from the archive ----------------------------------------
    let names_pending: Rc<RefCell<Option<(String, Vec<String>, Vec<String>)>>> =
        Rc::new(RefCell::new(None));
    let names_rows_model = Rc::new(VecModel::from(Vec::<SharedString>::new()));
    ui.set_names_rows(ModelRc::from(names_rows_model.clone()));
    let names_file: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    {
        let (st, tx) = (table_state.clone(), cover_tx.clone());
        let (rows, file_cell) = (names_rows_model.clone(), names_file.clone());
        let weak = ui.as_weak();
        ui.on_table_names_requested(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let st = st.borrow();
            let Some(t) = st.tracks.get(index as usize) else { return };
            let (real, _) = tunante_core::vgm_path::parse_vgm_path(&t.path);
            let file = real.to_string();
            let subsongs = st
                .all
                .iter()
                .filter(|x| tunante_core::vgm_path::parse_vgm_path(&x.path).0 == file)
                .count();

            *file_cell.borrow_mut() = file.clone();
            rows.set_vec(Vec::new());
            ui.set_names_can_apply(false);
            ui.set_names_heading(SharedString::from(if t.game.is_empty() {
                std::path::Path::new(&file)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                t.game.clone()
            }));
            ui.set_naming(true);

            let system = tunante_core::console::by_id(&t.console_id).and_then(|c| c.zophar);
            let Some(system) = system else {
                ui.set_names_status(SharedString::from(
                    "El formato de esta consola lleva una canción por fichero: \
                     no hay listado que consultar.",
                ));
                return;
            };
            if t.game.trim().is_empty() {
                ui.set_names_status(SharedString::from(
                    "Ponle nombre al juego primero — el listado se busca por él.",
                ));
                return;
            }
            if subsongs <= 1 {
                ui.set_names_status(SharedString::from("Este fichero lleva una sola canción."));
                return;
            }
            ui.set_names_status(SharedString::from("Consultando el archivo…"));
            spawn_names_fetch(tx.clone(), system, t.game.clone(), subsongs);
        });
    }
    {
        let (pending, tx, file_cell) =
            (names_pending.clone(), cover_tx.clone(), names_file.clone());
        let (db_file,) = (dbfile.clone(),);
        let weak = ui.as_weak();
        ui.on_names_apply(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some((_, titles, lengths)) = pending.borrow_mut().take() else {
                return;
            };
            ui.set_names_can_apply(false);
            ui.set_names_status(SharedString::from("Escribiendo…"));
            // "Fix the lengths too" off: keep whatever is already timed, so
            // the .m3u carries only the titles the user came for.
            let lengths = if ui.get_names_fix_lengths() {
                lengths
            } else {
                Vec::new()
            };
            spawn_names_apply(
                tx.clone(),
                db_file.clone(),
                file_cell.borrow().clone(),
                titles,
                lengths,
            );
        });
    }

    // --- Add to playlist from the table --------------------------------------
    let add_targets: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let (st, targets) = (table_state.clone(), add_targets.clone());
        let weak = ui.as_weak();
        ui.on_table_add_to_playlist(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let st = st.borrow();
            let i = index as usize;
            let (ids, heading) = if st.selected.len() > 1 && st.selected.contains(&i) {
                let mut idx: Vec<usize> = st.selected.iter().copied().collect();
                idx.sort_unstable();
                let ids: Vec<String> =
                    idx.iter().filter_map(|&j| st.tracks.get(j).map(|t| t.id.clone())).collect();
                let heading = format!("Añadir {} pistas a…", ids.len());
                (ids, heading)
            } else {
                let Some(t) = st.tracks.get(i) else { return };
                let name = if t.title.is_empty() { t.path.clone() } else { t.title.clone() };
                (vec![t.id.clone()], format!("Añadir «{}» a…", name))
            };
            if ids.is_empty() {
                return;
            }
            *targets.borrow_mut() = ids;
            ui.set_pick_heading(SharedString::from(heading));
            ui.set_picking_playlist(true);
        });
    }
    {
        let (db_p, targets, views_p) = (db.clone(), add_targets.clone(), views.clone());
        let weak = ui.as_weak();
        ui.on_playlist_picked(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let ids = std::mem::take(&mut *targets.borrow_mut());
            if !ids.is_empty() {
                if let Err(e) = db_p.add_tracks_to_playlist(&id, &ids) {
                    eprintln!("no se pudo añadir a la lista: {e}");
                }
                refresh_playlists(&db_p, &views_p, "");
            }
            ui.set_picking_playlist(false);
        });
    }
    {
        let (db_p, targets, views_p) = (db.clone(), add_targets.clone(), views.clone());
        let weak = ui.as_weak();
        ui.on_playlist_created_for_add(move |name| {
            let Some(ui) = weak.upgrade() else { return };
            let ids = std::mem::take(&mut *targets.borrow_mut());
            let id = uuid::Uuid::new_v4().to_string();
            if let Err(e) = db_p.create_playlist(&id, &name) {
                eprintln!("no se pudo crear la lista: {e}");
            } else if !ids.is_empty() {
                if let Err(e) = db_p.add_tracks_to_playlist(&id, &ids) {
                    eprintln!("no se pudo añadir a la lista: {e}");
                }
            }
            refresh_playlists(&db_p, &views_p, "");
            ui.set_picking_playlist(false);
        });
    }

    // --- Selection and keyboard in the table --------------------------------
    {
        let (st, model) = (table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_table_row_clicked(move |i, ctrl, shift| {
            let Some(ui) = weak.upgrade() else { return };
            let i = i as usize;
            let mut st = st.borrow_mut();
            if i >= st.tracks.len() {
                return;
            }
            if shift && ctrl {
                // The range JOINS the selection instead of replacing it —
                // the old desktop's Ctrl+Shift.
                let (a, b) = (st.anchor.min(i), st.anchor.max(i));
                st.selected.extend(a..=b);
            } else if shift {
                let (a, b) = (st.anchor.min(i), st.anchor.max(i));
                st.selected = (a..=b).collect();
            } else if ctrl {
                if !st.selected.insert(i) {
                    st.selected.remove(&i);
                }
                st.anchor = i;
            } else {
                st.selected = std::iter::once(i).collect();
                st.anchor = i;
            }
            repaint_selection(&st, &model);
            ui.set_table_cursor(i as i32);
        });
    }
    {
        let (st, model) = (table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_table_cursor_moved(move |delta, shift| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            if st.tracks.is_empty() {
                return;
            }
            let last = st.tracks.len() as i32 - 1;
            let cur = ui.get_table_cursor();
            let next = if cur < 0 {
                if delta > 0 { 0 } else { last }
            } else {
                (cur + delta).clamp(0, last)
            } as usize;
            if shift {
                let (a, b) = (st.anchor.min(next), st.anchor.max(next));
                st.selected = (a..=b).collect();
            } else {
                st.selected = std::iter::once(next).collect();
                st.anchor = next;
            }
            repaint_selection(&st, &model);
            ui.set_table_cursor(next as i32);
        });
    }
    {
        let (st, model) = (table_state.clone(), table_model.clone());
        ui.on_table_select_all(move || {
            let mut st = st.borrow_mut();
            st.selected = (0..st.tracks.len()).collect();
            repaint_selection(&st, &model);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_table_activate_cursor(move || {
            let Some(ui) = weak.upgrade() else { return };
            let cursor = ui.get_table_cursor();
            if cursor >= 0 {
                ui.invoke_table_activated(cursor);
            }
        });
    }

    // --- The metadata editor ---------------------------------------------
    // Which tracks the open sheet is about. Ids and not row indices: the
    // table can be re-sorted or re-filtered underneath a dialog. One id is
    // the single editor, several is the batch one.
    let edit_target: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let (st, target) = (table_state.clone(), edit_target.clone());
        let weak = ui.as_weak();
        ui.on_table_edit_requested(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let st = st.borrow();
            let i = index as usize;
            let Some(t) = st.tracks.get(i) else { return };

            if st.selected.len() > 1 && st.selected.contains(&i) {
                // The batch editor: per-track fields hide, an empty field
                // leaves that column alone on every selected track.
                let mut idx: Vec<usize> = st.selected.iter().copied().collect();
                idx.sort_unstable();
                let picked: Vec<&tunante_core::db::models::Track> =
                    idx.iter().filter_map(|&j| st.tracks.get(j)).collect();
                *target.borrow_mut() = picked.iter().map(|t| t.id.clone()).collect();

                let uniform = |get: fn(&tunante_core::db::models::Track) -> &str| {
                    let first = get(picked[0]);
                    picked.iter().all(|t| get(t) == first).then(|| first.to_string())
                };
                ui.set_meta_batch(true);
                ui.set_meta_heading(SharedString::from(format!(
                    "{} pistas seleccionadas",
                    picked.len()
                )));
                ui.set_meta_detail(SharedString::from(
                    "Un campo vacío no toca nada.",
                ));
                ui.set_meta_title(SharedString::new());
                ui.set_meta_artist(SharedString::from(
                    uniform(|t| t.artist.as_str()).unwrap_or_default(),
                ));
                ui.set_meta_album(SharedString::from(
                    uniform(|t| t.album.as_str()).unwrap_or_default(),
                ));
                ui.set_meta_track(SharedString::new());
                ui.set_meta_tech(SharedString::new());
                ui.set_editing_metadata(true);
                return;
            }

            *target.borrow_mut() = vec![t.id.clone()];
            let name = std::path::Path::new(
                tunante_core::vgm_path::parse_vgm_path(&t.path).0,
            )
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
            ui.set_meta_batch(false);
            ui.set_meta_heading(SharedString::from(name));
            ui.set_meta_detail(SharedString::from(format!(
                "{} · {}",
                table_console_label(t),
                t.path
            )));
            ui.set_meta_title(SharedString::from(t.title.as_str()));
            ui.set_meta_artist(SharedString::from(t.artist.as_str()));
            ui.set_meta_album(SharedString::from(t.album.as_str()));
            ui.set_meta_track(SharedString::from(
                t.track_number.map(|n| n.to_string()).unwrap_or_default(),
            ));
            // The read-only half of the old Properties dialog, one line.
            let mut tech: Vec<String> = vec![
                cell_for(t, "duration", false),
                t.codec.to_uppercase(),
            ];
            for extra in [
                cell_for(t, "samplerate", false),
                cell_for(t, "channels", false),
                cell_for(t, "bitrate", false),
                cell_for(t, "size", false),
            ] {
                if !extra.is_empty() {
                    tech.push(extra);
                }
            }
            if t.rating > 0 {
                tech.push(stars_for(t.rating));
            }
            ui.set_meta_tech(SharedString::from(tech.join(" · ")));
            ui.set_editing_metadata(true);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_metadata_cancelled(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_editing_metadata(false);
            }
        });
    }
    {
        let (db_m, st, model, target) = (
            db.clone(),
            table_state.clone(),
            table_model.clone(),
            edit_target.clone(),
        );
        let (tree_m, views_m) = (tree.clone(), views.clone());
        let weak = ui.as_weak();
        ui.on_metadata_accepted(move || {
            let Some(ui) = weak.upgrade() else { return };
            let ids = std::mem::take(&mut *target.borrow_mut());
            if ids.is_empty() {
                ui.set_editing_metadata(false);
                return;
            }
            let batch = ids.len() > 1;
            let title = ui.get_meta_title().to_string();
            let artist = ui.get_meta_artist().to_string();
            let album = ui.get_meta_album().to_string();
            let track_raw = ui.get_meta_track().to_string();
            // Single editor: empty clears the number, unparseable leaves it.
            // Batch: numbers are per-track and never touched.
            let track_number = if batch || !track_raw.trim().is_empty() {
                track_raw.trim().parse::<i32>().ok().map(Some)
            } else {
                Some(None)
            };

            // In batch, None means "this column stays as it is" — exactly
            // what update_track_metadata's Options already say.
            let (title_arg, artist_arg, album_arg) = if batch {
                (
                    None,
                    (!artist.trim().is_empty()).then_some(artist.as_str()),
                    (!album.trim().is_empty()).then_some(album.as_str()),
                )
            } else {
                (
                    Some(title.as_str()),
                    Some(artist.as_str()),
                    Some(album.as_str()),
                )
            };

            for id in &ids {
                if let Err(e) = db_m.update_track_metadata(
                    id,
                    title_arg,
                    artist_arg,
                    album_arg,
                    None,
                    if batch { None } else { track_number },
                    None,
                ) {
                    eprintln!("no se pudieron guardar los metadatos: {e}");
                }
            }

            // The caches the table sorts and filters from, kept in step so
            // the rows repaint without a database round trip.
            {
                let mut st = st.borrow_mut();
                let apply = |t: &mut tunante_core::db::models::Track| {
                    if let Some(v) = title_arg {
                        t.title = v.to_string();
                    }
                    if let Some(v) = artist_arg {
                        t.artist = v.to_string();
                    }
                    if let Some(v) = album_arg {
                        t.album = v.to_string();
                    }
                    if !batch {
                        if let Some(tn) = track_number {
                            t.track_number = tn;
                        }
                    }
                };
                st.all
                    .iter_mut()
                    .filter(|t| ids.contains(&t.id))
                    .for_each(apply);
                st.tracks
                    .iter_mut()
                    .filter(|t| ids.contains(&t.id))
                    .for_each(apply);
                rebuild_table(&mut st, &model);
            }
            // The tree and grids read the database, so they just re-read.
            refresh_library(&ui, &tree_m, &db_m, &views_m);
            ui.set_editing_metadata(false);
        });
    }

    {
        let (db_t, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_table_faved_changed(move |faved| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            // Favoritos can be the first thing clicked in a session, before
            // the pane's lazy init ever fired.
            if !st.built {
                st.built = true;
                st.all = db_t.get_all_tracks().unwrap_or_default();
            }
            st.scope = if faved { Scope::Faved } else { Scope::Library };
            rebuild_table(&mut st, &model);
            ui.set_table_faved(faved);
            ui.set_table_scope_kind(SharedString::from(""));
            ui.set_table_folder_id(SharedString::from(""));
            ui.set_table_scope_label(SharedString::from(""));
        });
    }

    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_stop_playback(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(p) = player.borrow_mut().as_mut() {
                p.stop();
                push_now_playing(&ui, p);
                sync_queue_marker(p, &queue_model);
            }
        });
    }
    {
        let (db_r, st, model, player) = (
            db.clone(),
            table_state.clone(),
            table_model.clone(),
            player.clone(),
        );
        let weak = ui.as_weak();
        ui.on_rate_now(move |stars| {
            let Some(ui) = weak.upgrade() else { return };
            let track = player
                .borrow()
                .as_ref()
                .and_then(|p| p.current().cloned());
            let Some(track) = track else { return };
            // Same toggle rule as the table: the star it already has clears.
            let current = ui.get_now_rating();
            let new = if current == stars { 0 } else { stars };
            if let Err(e) = db_r.set_track_rating(&track.id, new) {
                eprintln!("no se pudo guardar la puntuación: {e}");
                return;
            }
            let order = db_r
                .get_setting("rating_source_priority")
                .ok()
                .flatten();
            {
                let path = track.path.clone();
                std::thread::spawn(move || {
                    if let Err(e) = tunante_helper::rate(
                        std::path::Path::new(&path),
                        new,
                        order.as_deref(),
                        std::time::Duration::from_secs(20),
                    ) {
                        eprintln!("no se pudo escribir la puntuación en disco: {e}");
                    }
                });
            }
            ui.set_now_rating(new);
            ui.set_now_stars(SharedString::from(stars_for(new)));
            refresh_counts(&db_r, &ui);
            let mut st = st.borrow_mut();
            if st.built {
                for t in st.all.iter_mut().filter(|t| t.path == track.path) {
                    t.rating = new;
                }
                rebuild_table(&mut st, &model);
            }
        });
    }

    {
        let (player, weak) = (player.clone(), ui.as_weak());
        let last_vol = Rc::new(std::cell::Cell::new(0.8f32));
        ui.on_toggle_mute(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(p) = player.borrow_mut().as_mut() {
                if p.volume() > 0.001 {
                    last_vol.set(p.volume());
                    p.set_volume(0.0);
                } else {
                    // Back to where it was — or the old desktop's 0.8 when
                    // nobody remembers.
                    p.set_volume(last_vol.get().max(0.05));
                }
                ui.set_volume(p.volume());
            }
        });
    }
    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        let last_fade = Rc::new(std::cell::Cell::new(4i32));
        ui.on_toggle_crossfade(move || {
            let Some(ui) = weak.upgrade() else { return };
            let current = ui.get_crossfade_secs();
            let next = if current > 0 {
                last_fade.set(current);
                0
            } else {
                last_fade.get()
            };
            ui.set_crossfade_secs(next);
            let _ = db.set_setting(
                "fade_on_track_change",
                if next > 0 { "true" } else { "false" },
            );
            if next > 0 {
                let _ = db.set_setting("fade_seconds", &next.to_string());
            }
            if let Some(p) = player.borrow_mut().as_mut() {
                let engine = p.engine_mut();
                engine.set_fade_on_track_change(next > 0);
                if next > 0 {
                    engine.set_fade_seconds(next as f32);
                }
            }
        });
    }
    {
        let (st, player, model) = (table_state.clone(), player.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_now_clicked(move || {
            let Some(ui) = weak.upgrade() else { return };
            let path = player
                .borrow()
                .as_ref()
                .and_then(|p| p.current().map(|t| t.path.clone()));
            let Some(path) = path else { return };
            let mut st = st.borrow_mut();
            // The row scrolls into view through the cursor — the table's
            // changed-cursor handler already does the maths.
            if let Some(i) = st.tracks.iter().position(|t| t.path == path) {
                st.selected = std::iter::once(i).collect();
                st.anchor = i;
                repaint_selection(&st, &model);
                ui.set_table_cursor(-1);
                ui.set_table_cursor(i as i32);
            } else if !st.filter.is_empty() || st.scope != Scope::Library {
                // Not in the current narrowing: widen to the whole library
                // and try again — the old app's fallback chain, first rung.
                st.filter.clear();
                st.scope = Scope::Library;
                ui.set_table_filter(SharedString::new());
                ui.set_table_faved(false);
                ui.set_table_scope_kind(SharedString::from(""));
                ui.set_table_folder_id(SharedString::from(""));
                ui.set_table_scope_label(SharedString::from(""));
                rebuild_table(&mut st, &model);
                if let Some(i) = st.tracks.iter().position(|t| t.path == path) {
                    st.selected = std::iter::once(i).collect();
                    st.anchor = i;
                    repaint_selection(&st, &model);
                    ui.set_table_cursor(-1);
                    ui.set_table_cursor(i as i32);
                }
            }
        });
    }

    // --- Pinned folders ------------------------------------------------------
    //
    // The database supported them from day one; this is the sidebar finally
    // asking. Pin from the table's context menu, open from the sidebar
    // (narrows the table to that subtree), unpin from the hover ✕.
    let pinned_model = Rc::new(VecModel::from(Vec::<PinnedRow>::new()));
    ui.set_pinned_folders(ModelRc::from(pinned_model.clone()));
    refresh_pinned(&db, &pinned_model);
    refresh_counts(&db, &ui);
    let folders_model = Rc::new(VecModel::from(Vec::<FolderRow>::new()));
    ui.set_library_folders(ModelRc::from(folders_model.clone()));
    refresh_library_folders(&db, &folders_model);
    let consoles_side = Rc::new(VecModel::from(Vec::<PlaylistRow>::new()));
    ui.set_sidebar_consoles(ModelRc::from(consoles_side.clone()));
    {
        let (db_c, tree_c, model) = (db.clone(), tree.clone(), consoles_side.clone());
        refresh_sidebar_consoles(&db_c, &tree_c, &model);
    }
    {
        let (tree_c, db_c, views_c) = (tree.clone(), db.clone(), views.clone());
        let weak = ui.as_weak();
        ui.on_console_opened(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            tree_c.borrow_mut().mode = library::Mode::Consoles;
            tree_c.borrow_mut().nav.clear();
            tree_c.borrow_mut().nav.push(format!("consola:{id}"));
            ui.set_library_mode(1);
            refresh_library(&ui, &tree_c, &db_c, &views_c);
        });
    }
    {
        let (db_c, player_c, queue_c) = (db.clone(), player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_console_played(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let tracks: Vec<_> = db_c
                .get_all_tracks()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| tunante_core::console::key_of(t) == id.as_str())
                .collect();
            play_collection(&ui, &player_c, &queue_c, tracks);
        });
    }
    // The grande shell shows a console or a playlist in the powerful table,
    // not the phone grid — the old desktop's context-aware TrackList.
    {
        let (db_c, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_console_opened_in_table(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            if !st.built {
                st.built = true;
                st.all = db_c.get_all_tracks().unwrap_or_default();
            }
            st.scope = Scope::Console(id.to_string());
            rebuild_table(&mut st, &model);
            ui.set_table_faved(false);
            ui.set_table_scope_kind(SharedString::from("console"));
            ui.set_table_scope_label(SharedString::from(scope_label(&db_c, &st.scope)));
            ui.set_table_folder_id(id);
            ui.set_table_sort_col(
                st.visible.iter().position(|k| k == &st.sort_key).map(|i| i as i32).unwrap_or(-1),
            );
        });
    }
    {
        let (db_c, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_playlist_opened_in_table(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            // Ordered ids from the playlist, resolved against `all` on rebuild.
            let ids: Vec<String> = db_c
                .get_playlist_tracks(&id)
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.id)
                .collect();
            let mut st = st.borrow_mut();
            if !st.built {
                st.built = true;
                st.all = db_c.get_all_tracks().unwrap_or_default();
            }
            st.scope = Scope::Playlist { ids, id: id.to_string() };
            // Enter on stored order: the sentinel sort shows no column arrow
            // until the user picks one.
            st.sort_key = "__scope__".to_string();
            rebuild_table(&mut st, &model);
            ui.set_table_faved(false);
            ui.set_table_scope_kind(SharedString::from("playlist"));
            ui.set_table_scope_label(SharedString::from(scope_label(&db_c, &st.scope)));
            ui.set_table_folder_id(id);
            ui.set_table_sort_col(-1);
        });
    }
    {
        let (db_c, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let (plmodel, allpl) = (playlists_model.clone(), all_playlists_model.clone());
        ui.on_table_remove_from_playlist(move |index| {
            let mut st = st.borrow_mut();
            let Scope::Playlist { id, .. } = st.scope.clone() else { return };
            let i = index as usize;
            // The whole selection when the clicked row is in it, else just it
            // — the old desktop's Delete removed everything selected.
            let ids: Vec<String> = if st.selected.len() > 1 && st.selected.contains(&i) {
                let mut idx: Vec<usize> = st.selected.iter().copied().collect();
                idx.sort_unstable();
                idx.iter().filter_map(|&j| st.tracks.get(j).map(|t| t.id.clone())).collect()
            } else {
                st.tracks.get(i).map(|t| t.id.clone()).into_iter().collect()
            };
            if ids.is_empty() {
                return;
            }
            for track_id in &ids {
                let _ = db_c.remove_track_from_playlist(&id, track_id);
            }
            // Re-resolve the playlist's ids and rebuild in place.
            let ids: Vec<String> = db_c
                .get_playlist_tracks(&id)
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.id)
                .collect();
            st.scope = Scope::Playlist { ids, id: id.clone() };
            rebuild_table(&mut st, &model);
            drop(st);
            // The sidebar counts move with it.
            refresh_playlists_models(&db_c, &plmodel, &allpl);
        });
    }
    {
        let (st, model) = (table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_table_scope_cleared(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            st.scope = Scope::Library;
            if st.sort_key == "__scope__" {
                st.sort_key = "title".to_string();
            }
            rebuild_table(&mut st, &model);
            ui.set_table_faved(false);
            ui.set_table_scope_kind(SharedString::from(""));
            ui.set_table_folder_id(SharedString::from(""));
            ui.set_table_scope_label(SharedString::from(""));
            ui.set_table_sort_col(
                st.visible.iter().position(|k| k == &st.sort_key).map(|i| i as i32).unwrap_or(-1),
            );
        });
    }
    // The folder sheet's four new verbs, path-based.
    {
        let (db_p, player_p, queue_model_p) = (db.clone(), player.clone(), queue_model.clone());
        let weak = ui.as_weak();
        ui.on_library_play_path(move |path| {
            let Some(ui) = weak.upgrade() else { return };
            // get_tracks_by_folder already answers for the whole subtree.
            let tracks = db_p.get_tracks_by_folder(&path).unwrap_or_default();
            play_collection(&ui, &player_p, &queue_model_p, tracks);
        });
    }
    {
        let (db_p, target, sugg) = (db.clone(), reclass_target.clone(), sugg_model.clone());
        let weak = ui.as_weak();
        ui.on_library_reclassify_path(move |path| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(t) = db_p
                .get_tracks_by_folder(&path)
                .unwrap_or_default()
                .into_iter()
                .next()
            else {
                return;
            };
            *target.borrow_mut() = Some((path.to_string(), t.path.clone()));
            let folder_name = std::path::Path::new(path.as_str())
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            ui.set_reclass_heading(SharedString::from(folder_name));
            ui.set_reclass_scope_folder(true);
            ui.set_reclass_console_filter(SharedString::new());
            ui.set_consoles(ModelRc::new(VecModel::from(consoles_for_filter(""))));
            ui.set_reclass_console(SharedString::from(t.console_id.as_str()));
            ui.set_reclass_game(SharedString::from(t.game.as_str()));
            sugg.set_vec(Vec::new());
            ui.set_reclassifying(true);
        });
    }
    {
        let (db_p, model) = (db.clone(), pinned_model.clone());
        ui.on_library_pin_path(move |path| {
            let _ = db_p.add_pinned_folder(&uuid::Uuid::new_v4().to_string(), &path);
            refresh_pinned(&db_p, &model);
        });
    }
    ui.on_library_reveal_path(|path| {
        integrate::reveal(&std::path::Path::new(path.as_str()).join("."));
    });

    {
        let (db_f, model) = (db.clone(), folders_model.clone());
        let sync = sync_watches.clone();
        ui.on_folder_watch_toggled(move |id, on| {
            if let Err(e) = db_f.toggle_folder_watching(&id, on) {
                eprintln!("no se pudo cambiar la vigilancia: {e}");
                return;
            }
            sync();
            refresh_library_folders(&db_f, &model);
        });
    }
    {
        let (db_f, model) = (db.clone(), folders_model.clone());
        let sync = sync_watches.clone();
        let dirty = library_dirty.clone();
        ui.on_folder_removed(move |id| {
            let folders = db_f.get_monitored_folders().unwrap_or_default();
            let Some(folder) = folders.iter().find(|f| f.id == id.as_str()) else {
                return;
            };
            // Prune its tracks, but never the ones another root still
            // covers — nested folders were absorbed on add, and removal
            // must not take the survivors' music with it.
            let keep: Vec<String> = folders
                .iter()
                .filter(|f| f.id != id.as_str())
                .map(|f| f.path.clone())
                .collect();
            if let Err(e) = db_f.remove_monitored_folder(&id) {
                eprintln!("no se pudo quitar la carpeta: {e}");
                return;
            }
            match db_f.remove_tracks_by_folder_path_excluding(&folder.path, &keep) {
                Ok(n) => eprintln!("carpeta fuera: {} pistas menos", n),
                Err(e) => eprintln!("la carpeta se quitó pero la poda falló: {e}"),
            }
            sync();
            refresh_library_folders(&db_f, &model);
            dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }
    {
        let (db_p, st) = (db.clone(), table_state.clone());
        let model = pinned_model.clone();
        ui.on_table_pin_folder(move |index| {
            let path = {
                let st = st.borrow();
                st.tracks.get(index as usize).map(|t| t.path.clone())
            };
            let Some(path) = path else { return };
            let (real, _) = tunante_core::vgm_path::parse_vgm_path(&path);
            let Some(folder) = std::path::Path::new(real).parent() else { return };
            let _ = db_p.add_pinned_folder(
                &uuid::Uuid::new_v4().to_string(),
                &folder.to_string_lossy(),
            );
            refresh_pinned(&db_p, &model);
        });
    }
    {
        let (db_p, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_folder_opened(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let folder = sidebar_folder_path(&db_p, &id);
            let mut st = st.borrow_mut();
            if !st.built {
                st.built = true;
                st.all = db_p.get_all_tracks().unwrap_or_default();
            }
            st.scope = match folder {
                Some(f) => Scope::Folder(f),
                None => Scope::Library,
            };
            rebuild_table(&mut st, &model);
            let scoped = matches!(st.scope, Scope::Folder(_));
            ui.set_table_faved(false);
            ui.set_table_scope_kind(SharedString::from(if scoped { "folder" } else { "" }));
            ui.set_table_folder_id(if scoped { id } else { SharedString::from("") });
            ui.set_table_scope_label(SharedString::from(scope_label(&db_p, &st.scope)));
        });
    }
    {
        let db_p = db.clone();
        ui.on_folder_revealed(move |id| {
            if let Some(path) = sidebar_folder_path(&db_p, &id) {
                // reveal() selects a file; for a folder, opening it plain is
                // the right verb, so hand it a child that may not exist and
                // let the fallback open the folder itself.
                integrate::reveal(&std::path::Path::new(&path).join("."));
            }
        });
    }
    {
        let (db_p, st, tmodel) = (db.clone(), table_state.clone(), table_model.clone());
        let model = pinned_model.clone();
        let weak = ui.as_weak();
        ui.on_folder_unpinned(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let _ = db_p.remove_pinned_folder(&id);
            refresh_pinned(&db_p, &model);
            // Unpinning the folder that narrows the table widens it back.
            if ui.get_table_folder_id() == id {
                let mut st = st.borrow_mut();
                st.scope = Scope::Library;
                rebuild_table(&mut st, &tmodel);
                ui.set_table_scope_kind(SharedString::from(""));
                ui.set_table_folder_id(SharedString::from(""));
                ui.set_table_scope_label(SharedString::from(""));
            }
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
                refresh_queue(p, &queue_model);
            }
        });
    }
    {
        let (player, queue_model) = (player.clone(), queue_model.clone());
        ui.on_queue_reordered(move |from, to| {
            if let Some(p) = player.borrow_mut().as_mut() {
                let (from, to) = (from.max(0) as usize, to.max(0) as usize);
                let u = p.user_queue().len();
                // Two sections, two orders; a drag across the border has no
                // meaning and does nothing.
                if from < u && to < u {
                    p.move_user(from, to);
                } else if from >= u && to >= u {
                    p.reorder(from - u, to - u);
                }
                refresh_queue(p, &queue_model);
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
                let i = index.max(0) as usize;
                let u = p.user_queue().len();
                if i < u {
                    p.dequeue_user(i);
                } else {
                    p.remove_from_queue(i - u);
                }
                refresh_queue(p, &queue_model);
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
                let i = index.max(0) as usize;
                let u = p.user_queue().len();
                let ok = if i < u {
                    p.play_user(i).is_ok()
                } else {
                    p.play_index(i - u).is_ok()
                };
                if ok {
                    push_now_playing(&ui, p);
                    refresh_queue(p, &queue_model);
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
        let db_adopt = db.clone();
        let weak = ui.as_weak();
        ui.on_next_track(move || {
            if let Some(p) = player.borrow_mut().as_mut() {
                let _ = p.next();
                adopt_pending_context(p, &db_adopt);
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
                    let _ = tunante_helper::scan::scan_folder_with(&db, &folder, &probe_opts(&db), |p| {
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
    // --- Bulk cover download: preview first, apply second, undo forever ----
    let bulk_plans_model = Rc::new(VecModel::from(Vec::<PlanRow>::new()));
    ui.set_bulk_plans(ModelRc::from(bulk_plans_model.clone()));
    let bulk_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    ui.set_undo_covers_label(SharedString::from(
        if db
            .get_setting("mini.last_cover_run")
            .ok()
            .flatten()
            .filter(|v| !v.is_empty())
            .is_some()
        {
            "disponible"
        } else {
            "nada que deshacer"
        },
    ));
    {
        let (db, tx, cancel) = (db.clone(), cover_tx.clone(), bulk_cancel.clone());
        let (plans, weak) = (bulk_plans_model.clone(), ui.as_weak());
        ui.on_descargar_caratulas(move || {
            let Some(ui) = weak.upgrade() else { return };
            let tracks = db.get_all_tracks().unwrap_or_default();
            if tracks.is_empty() {
                return;
            }
            cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            plans.set_vec(Vec::new());
            ui.set_bulk_busy(true);
            ui.set_bulk_status(SharedString::from("Preparando…"));
            ui.set_bulk_covering(true);
            spawn_bulk_covers(tx.clone(), tracks, cancel.clone(), true);
        });
    }
    {
        let (db, tx, cancel) = (db.clone(), cover_tx.clone(), bulk_cancel.clone());
        let weak = ui.as_weak();
        ui.on_bulk_apply(move || {
            let Some(ui) = weak.upgrade() else { return };
            let tracks = db.get_all_tracks().unwrap_or_default();
            cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            ui.set_bulk_busy(true);
            ui.set_bulk_status(SharedString::from("Descargando…"));
            spawn_bulk_covers(tx.clone(), tracks, cancel.clone(), false);
        });
    }
    {
        let cancel = bulk_cancel.clone();
        let weak = ui.as_weak();
        ui.on_bulk_cancelled(move || {
            let Some(ui) = weak.upgrade() else { return };
            // Closing the sheet is also the stop button: a run in flight sees
            // the flag at its next request.
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            ui.set_bulk_covering(false);
        });
    }
    {
        let (db, dirty) = (db.clone(), std::sync::Arc::clone(&art_dirty));
        let weak = ui.as_weak();
        ui.on_undo_covers(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(stamp) = db
                .get_setting("mini.last_cover_run")
                .ok()
                .flatten()
                .filter(|v| !v.is_empty())
                .and_then(|v| v.parse::<u64>().ok())
            else {
                return;
            };
            match tunante_art::folder::Manifest::undo(&tunante_art::cache::cache_dir(), stamp) {
                Ok(n) => {
                    let _ = db.set_setting("mini.last_cover_run", "");
                    ui.set_undo_covers_label(SharedString::from(format!(
                        "{n} borradas"
                    )));
                    dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                Err(e) => {
                    ui.set_undo_covers_label(SharedString::from(format!("falló: {e}")));
                }
            }
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
    // --- Global shortcuts (portal) --------------------------------------------
    //
    // The session is created lazily, on the first enable, because the first
    // BindShortcuts opens the desktop's own binding dialog. The forward flag
    // outlives toggles so re-enabling costs nothing.
    let shortcut_forward = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shortcut_rx: Rc<RefCell<Option<std::sync::mpsc::Receiver<shortcuts::Msg>>>> =
        Rc::new(RefCell::new(None));
    {
        let enabled = db
            .get_setting("mini.global_shortcuts")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);
        ui.set_global_shortcuts_on(enabled);
        if enabled {
            // The portal needs the app to exist as a .desktop entry before
            // it will listen to it. Idempotent, so re-running is free.
            let _ = integrate::make_desktop_entry();
            shortcut_forward.store(true, std::sync::atomic::Ordering::Relaxed);
            *shortcut_rx.borrow_mut() = Some(shortcuts::spawn(shortcut_forward.clone()));
            ui.set_global_shortcuts_label(SharedString::from("vinculando…"));
        } else {
            ui.set_global_shortcuts_label(SharedString::from("no"));
        }
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        let (flag, rx_slot) = (shortcut_forward.clone(), shortcut_rx.clone());
        ui.on_toggle_global_shortcuts(move || {
            let Some(ui) = weak.upgrade() else { return };
            let enable = !ui.get_global_shortcuts_on();
            ui.set_global_shortcuts_on(enable);
            let _ = db.set_setting(
                "mini.global_shortcuts",
                if enable { "true" } else { "false" },
            );
            flag.store(enable, std::sync::atomic::Ordering::Relaxed);
            if enable {
                if rx_slot.borrow().is_none() {
                    let _ = integrate::make_desktop_entry();
                    *rx_slot.borrow_mut() = Some(shortcuts::spawn(flag.clone()));
                    ui.set_global_shortcuts_label(SharedString::from("vinculando…"));
                } else {
                    ui.set_global_shortcuts_label(SharedString::from("sí"));
                }
            } else {
                ui.set_global_shortcuts_label(SharedString::from("no"));
            }
        });
    }

    // --- Mouse side buttons ---------------------------------------------------
    //
    // evdev readers behind a toggle (mini.mouse_buttons, off by default).
    // The stop flag is per-activation: toggling off abandons the old
    // generation of threads, toggling on starts a fresh one — which is also
    // how a mouse plugged in later gets noticed.
    let (button_tx, button_rx) = std::sync::mpsc::channel::<buttons::ButtonCmd>();
    let button_stop: Rc<RefCell<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
        Rc::new(RefCell::new(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))));
    let buttons_label = |n: usize| -> String {
        if n == 0 {
            "sin acceso (grupo input)".to_string()
        } else {
            format!("sí ({n} dispositivos)")
        }
    };
    {
        let enabled = db
            .get_setting("mini.mouse_buttons")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);
        ui.set_mouse_buttons_on(enabled);
        if enabled {
            let n = buttons::spawn(button_tx.clone(), button_stop.borrow().clone());
            ui.set_mouse_buttons_label(SharedString::from(buttons_label(n)));
        } else {
            ui.set_mouse_buttons_label(SharedString::from("no"));
        }
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        let (stop, tx) = (button_stop.clone(), button_tx.clone());
        ui.on_toggle_mouse_buttons(move || {
            let Some(ui) = weak.upgrade() else { return };
            let enable = !ui.get_mouse_buttons_on();
            ui.set_mouse_buttons_on(enable);
            let _ = db.set_setting("mini.mouse_buttons", if enable { "true" } else { "false" });
            stop.borrow().store(true, std::sync::atomic::Ordering::Relaxed);
            if enable {
                let fresh = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                *stop.borrow_mut() = fresh.clone();
                let n = buttons::spawn(tx.clone(), fresh);
                ui.set_mouse_buttons_label(SharedString::from(buttons_label(n)));
            } else {
                ui.set_mouse_buttons_label(SharedString::from("no"));
            }
        });
    }

    // --- Cover fit and cache ------------------------------------------------
    //
    // Five ways to sit a non-square cover in a square, under the desktop's
    // `cover_fit` key and with its five values.
    {
        let stored = db.get_setting("cover_fit").ok().flatten();
        let fit = cover_fit_from_key(stored.as_deref().unwrap_or("cover"));
        ui.set_cover_fit(fit);
        ui.set_cover_fit_label(SharedString::from(cover_fit_label(fit)));
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_cycle_cover_fit(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = (ui.get_cover_fit() + 1) % 5;
            ui.set_cover_fit(next);
            ui.set_cover_fit_label(SharedString::from(cover_fit_label(next)));
            let _ = db.set_setting("cover_fit", cover_fit_key(next));
        });
    }
    ui.set_clear_cache_label(SharedString::from("🗑"));
    {
        let weak = ui.as_weak();
        ui.on_clear_cover_cache(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_clear_cache_label(SharedString::from(
                match tunante_art::cache::clear() {
                    Ok(n) => format!("{n} fuera"),
                    Err(e) => format!("falló: {e}"),
                },
            ));
        });
    }

    // --- Startup rating reconciliation ----------------------------------------
    //
    // The old desktop's pass, through the pipe instead of in-process: a
    // _ratings.m3u edited on another machine or a tag written by another
    // player wins by the same priority order, and the diff lands in the
    // database with library_dirty repainting everything.
    {
        let (dbfile, dirty) = (dbfile.clone(), library_dirty.clone());
        let order = db.get_setting("rating_source_priority").ok().flatten();
        let items: Vec<(i32, String, String)> = db
            .get_all_tracks()
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.rating, t.path, t.id))
            .collect();
        if !items.is_empty() {
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let total = items.len();
                let by_path: std::collections::HashMap<&str, &str> = items
                    .iter()
                    .map(|(_, p, id)| (p.as_str(), id.as_str()))
                    .collect();
                let pairs: Vec<(i32, String)> =
                    items.iter().map(|(r, p, _)| (*r, p.clone())).collect();
                match tunante_helper::resolve_ratings(&pairs, order.as_deref()) {
                    Ok(diff) if diff.is_empty() => {
                        log::info!(
                            "ratings: {total} pistas revisadas en {:?}, nada que cambiar",
                            started.elapsed()
                        );
                    }
                    Ok(diff) => {
                        let Ok(db) = Database::new(&dbfile) else { return };
                        let mut n = 0;
                        for (rating, path) in &diff {
                            if let Some(id) = by_path.get(path.as_str()) {
                                if db.set_track_rating(id, *rating).is_ok() {
                                    n += 1;
                                }
                            }
                        }
                        log::info!(
                            "ratings: {n} de {total} actualizadas desde disco en {:?}",
                            started.elapsed()
                        );
                        dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    Err(e) => log::warn!("ratings: la pasada falló: {e}"),
                }
            });
        }
    }

    // --- Loose files -------------------------------------------------------
    //
    // No file dialog of our own: kdialog or zenity, whichever this desktop
    // has, on a worker thread (a dialog can sit open for minutes). The picks
    // are probed with the library's own knobs and inserted like any scanned
    // track; the row's label is the whole report.
    let (loose_tx, loose_rx) = std::sync::mpsc::channel::<String>();
    ui.set_add_files_label(SharedString::from("＋"));
    {
        let (dbfile, dirty) = (dbfile.clone(), library_dirty.clone());
        let tx = loose_tx.clone();
        let weak = ui.as_weak();
        ui.on_add_files(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_add_files_label(SharedString::from("eligiendo…"));
            let (dbfile, dirty, tx) = (dbfile.clone(), dirty.clone(), tx.clone());
            std::thread::spawn(move || {
                let picked = pick_files();
                let paths: Vec<_> = picked
                    .iter()
                    .filter(|p| tunante_core::vgm_path::is_audio_file(std::path::Path::new(p)))
                    .collect();
                if picked.is_empty() {
                    let _ = tx.send("＋".to_string());
                    return;
                }
                if paths.is_empty() {
                    let _ = tx.send("nada reconocible".to_string());
                    return;
                }
                let Ok(db) = Database::new(&dbfile) else {
                    let _ = tx.send("sin base de datos".to_string());
                    return;
                };
                let opts = probe_opts(&db);
                let mut n = 0usize;
                for path in paths {
                    let Ok(values) = tunante_helper::probe_with(
                        std::path::Path::new(path),
                        tunante_helper::scan::PROBE_TIMEOUT,
                        &opts,
                    ) else {
                        continue;
                    };
                    for v in values {
                        if let Ok(track) =
                            serde_json::from_value::<tunante_core::db::models::Track>(v)
                        {
                            if db.insert_track(&track).is_ok() {
                                n += 1;
                            }
                        }
                    }
                }
                dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = tx.send(format!("{n} añadidas"));
            });
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

    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        ui.on_cycle_output(move || {
            let Some(ui) = weak.upgrade() else { return };
            // system, then every device, round and round. The list is asked
            // for on each press rather than cached: plugging headphones in is
            // exactly the moment this row gets used.
            let mut ring = vec!["system".to_string()];
            ring.extend(tunante_audio::list_output_devices());
            let current = db
                .get_setting("audio_output_device")
                .ok()
                .flatten()
                .unwrap_or_else(|| "system".to_string());
            let idx = ring.iter().position(|d| *d == current).unwrap_or(0);
            let next = ring[(idx + 1) % ring.len()].clone();

            let sel = tunante_audio::OutputSelection::from_setting(&next);
            let _ = db.set_setting("audio_output_device", &sel.to_setting());
            if let Some(p) = player.borrow_mut().as_mut() {
                if let Err(e) = p.engine_mut().set_output_selection(sel) {
                    eprintln!("no se pudo cambiar la salida: {e}");
                }
            }
            ui.set_output_label(SharedString::from(output_label(&next)));
        });
    }
    ui.set_sidebar_width(
        db.get_setting("mini.sidebar_width")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i32>().ok())
            .map(|w| w.clamp(150, 500))
            .unwrap_or(210),
    );
    {
        let db = db.clone();
        ui.on_sidebar_resized(move |w| {
            let _ = db.set_setting("mini.sidebar_width", &w.clamp(150, 500).to_string());
        });
    }
    let tray_click: Rc<RefCell<String>> = Rc::new(RefCell::new(
        db.get_setting("tray_middle_click_action")
            .ok()
            .flatten()
            .unwrap_or_else(|| "toggle".to_string()),
    ));
    ui.set_tray_click_label(SharedString::from(tray_click_label(&tray_click.borrow())));
    {
        let (db, weak, click) = (db.clone(), ui.as_weak(), tray_click.clone());
        ui.on_cycle_tray_click(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = match click.borrow().as_str() {
                "toggle" => "play_pause",
                "play_pause" => "stop",
                "stop" => "next_track",
                "next_track" => "next_track_with_fade",
                _ => "toggle",
            }
            .to_string();
            let _ = db.set_setting("tray_middle_click_action", &next);
            ui.set_tray_click_label(SharedString::from(tray_click_label(&next)));
            *click.borrow_mut() = next;
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        let style = Rc::new(std::cell::Cell::new(tray_style));
        ui.on_cycle_tray_style(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = (style.get() + 1) % 3;
            style.set(next);
            let _ = db.set_setting(
                "tray_icon_style",
                match next {
                    1 => "symbolic",
                    2 => "logo",
                    _ => "system",
                },
            );
            tray::set_style(next);
            ui.set_tray_style_label(SharedString::from(tray_style_label(next)));
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        let theme_mode = theme_mode.clone();
        ui.on_cycle_theme(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = (theme_mode.get() + 1) % 3;
            theme_mode.set(next);
            let _ = db.set_setting(
                "theme",
                match next {
                    1 => "light",
                    2 => "system",
                    _ => "dark",
                },
            );
            let dark = match next {
                1 => false,
                2 => theme_watch::prefers_dark().unwrap_or(true),
                _ => true,
            };
            ui.global::<Theme>().set_dark(dark);
            ui.set_theme_label(SharedString::from(theme_mode_label(next)));
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_cycle_ui_mode(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = (ui.get_ui_mode() + 1) % 3;
            ui.set_ui_mode(next);
            let _ = db.set_setting("mini.ui_mode", &next.to_string());
            ui.set_ui_mode_label(SharedString::from(ui_mode_label(next)));
        });
    }

    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        ui.on_cycle_short_filter(move || {
            let Some(ui) = weak.upgrade() else { return };
            // desactivado → 5 → 10 → 30 → desactivado. Five seconds catches
            // SFX rows; thirty catches jingles without eating real intros.
            let next = match ui.get_short_filter_secs() {
                0 => 5,
                5 => 10,
                10 => 30,
                _ => 0,
            };
            ui.set_short_filter_secs(next);
            let _ = db.set_setting("mini.short_filter_secs", &next.to_string());
            if let Some(p) = player.borrow_mut().as_mut() {
                p.set_short_filter(next as i64 * 1000);
            }
        });
    }

    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        ui.on_toggle_continue_queue(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = !ui.get_continue_queue();
            ui.set_continue_queue(next);
            let _ = db.set_setting("continue_from_queue", if next { "true" } else { "false" });
            if let Some(p) = player.borrow_mut().as_mut() {
                p.set_continue_from_queue(next);
            }
        });
    }
    ui.set_resume_on_open(
        db.get_setting("resume_playback_on_open")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false),
    );
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_toggle_resume_on_open(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = !ui.get_resume_on_open();
            ui.set_resume_on_open(next);
            let _ = db.set_setting("resume_playback_on_open", if next { "true" } else { "false" });
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_cycle_loop_max(move || {
            let Some(ui) = weak.upgrade() else { return };
            // predeterminado → 2 → 3 → 5 → 8 min. Applies from the next scan;
            // existing rows keep the durations they were sealed with.
            let next = match ui.get_loop_max_mins() {
                0 => 2,
                2 => 3,
                3 => 5,
                5 => 8,
                _ => 0,
            };
            ui.set_loop_max_mins(next);
            let _ = db.set_setting("loop_max_seconds", &(next * 60).to_string());
        });
    }
    {
        let get_bool = |k: &str, def: bool| {
            db.get_setting(k)
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(def)
        };
        ui.set_auto_covers(get_bool("auto_download_cover_art", false));
        ui.set_covers_in_folder(get_bool("store_covers_in_folder", false));
        ui.set_titlebar_track(get_bool("show_track_in_titlebar", true));
        ui.set_show_cover(get_bool("show_cover_art", true));
        ui.set_show_faved(get_bool("show_faved", true));
        ui.set_show_folders(get_bool("show_folders_list", true));
        ui.set_show_playlists(get_bool("show_playlists", true));
    }
    // Four section toggles, one shape.
    macro_rules! sidebar_toggle {
        ($on:ident, $get:ident, $set:ident, $key:literal) => {{
            let (db, weak) = (db.clone(), ui.as_weak());
            ui.$on(move || {
                let Some(ui) = weak.upgrade() else { return };
                let next = !ui.$get();
                ui.$set(next);
                let _ = db.set_setting($key, if next { "true" } else { "false" });
            });
        }};
    }
    sidebar_toggle!(on_toggle_show_cover, get_show_cover, set_show_cover, "show_cover_art");
    sidebar_toggle!(on_toggle_show_faved, get_show_faved, set_show_faved, "show_faved");
    sidebar_toggle!(on_toggle_show_folders, get_show_folders, set_show_folders, "show_folders_list");
    sidebar_toggle!(on_toggle_show_playlists, get_show_playlists, set_show_playlists, "show_playlists");
    ui.set_show_consoles(get_bool_setting(&db, "show_consoles", true));
    ui.set_show_files(get_bool_setting(&db, "show_files_browser", true));
    sidebar_toggle!(on_toggle_show_consoles, get_show_consoles, set_show_consoles, "show_consoles");
    sidebar_toggle!(on_toggle_show_files, get_show_files, set_show_files, "show_files_browser");
    // Cap the endless-track limit over declared lengths too.
    ui.set_caps_all(get_bool_setting(&db, "loop_max_caps_all", false));
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_toggle_caps_all(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = !ui.get_caps_all();
            ui.set_caps_all(next);
            let _ = db.set_setting("loop_max_caps_all", if next { "true" } else { "false" });
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_reset_dsp(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            *c = tunante_core::dsp::DspConfig::default();
            push_dsp_ui(&ui, &c);
            store_dsp(&db, &player, &c);
        });
    }

    // --- In-app shortcuts ----------------------------------------------------
    //
    // The old Shortcuts tab's soul: every action records whatever combo the
    // user presses next. Bindings live one per setting (shortcut.<id>), the
    // map inverts them for dispatch, and the shell-level FocusScope hands
    // every unclaimed key through shortcut-key below.
    let shortcut_rows = Rc::new(VecModel::from(Vec::<ShortcutRow>::new()));
    ui.set_shortcut_rows(ModelRc::from(shortcut_rows.clone()));
    let shortcut_capturing: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let shortcut_map: Rc<RefCell<std::collections::HashMap<String, String>>> =
        Rc::new(RefCell::new(std::collections::HashMap::new()));
    let rebuild_shortcuts = {
        let (db, rows, map, capturing) = (
            db.clone(),
            shortcut_rows.clone(),
            shortcut_map.clone(),
            shortcut_capturing.clone(),
        );
        Rc::new(move || {
            let mut m = map.borrow_mut();
            m.clear();
            let capturing = capturing.borrow();
            rows.set_vec(
                SHORTCUT_ACTIONS
                    .iter()
                    .map(|(id, label)| {
                        let key = db
                            .get_setting(&format!("shortcut.{id}"))
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        if !key.is_empty() {
                            m.insert(key.clone(), id.to_string());
                        }
                        ShortcutRow {
                            id: SharedString::from(*id),
                            label: SharedString::from(*label),
                            key: SharedString::from(
                                if capturing.as_deref() == Some(*id) {
                                    "pulsa una tecla…".to_string()
                                } else {
                                    key
                                },
                            ),
                        }
                    })
                    .collect::<Vec<_>>(),
            );
        })
    };
    rebuild_shortcuts();
    {
        let (capturing, rebuild) = (shortcut_capturing.clone(), rebuild_shortcuts.clone());
        ui.on_shortcut_capture(move |id| {
            *capturing.borrow_mut() = Some(id.to_string());
            rebuild();
        });
    }
    {
        let (db, capturing, map, rebuild) = (
            db.clone(),
            shortcut_capturing.clone(),
            shortcut_map.clone(),
            rebuild_shortcuts.clone(),
        );
        let (player, weak) = (player.clone(), ui.as_weak());
        ui.on_shortcut_key(move |text, ctrl, alt, shift| {
            let Some(ui) = weak.upgrade() else { return false };
            // Recording: the next key becomes the binding. Escape cancels,
            // Supr/Backspace unbinds.
            let armed = capturing.borrow().clone();
            if let Some(id) = armed {
                let c = text.chars().next().unwrap_or('\0');
                if c == '\u{1b}' {
                    *capturing.borrow_mut() = None;
                    rebuild();
                    return true;
                }
                if c == '\u{7f}' || c == '\u{8}' {
                    let _ = db.set_setting(&format!("shortcut.{id}"), "");
                    *capturing.borrow_mut() = None;
                    rebuild();
                    return true;
                }
                let Some(combo) = shortcut_combo(&text, ctrl, alt, shift) else {
                    return true;
                };
                let _ = db.set_setting(&format!("shortcut.{id}"), &combo);
                *capturing.borrow_mut() = None;
                rebuild();
                return true;
            }
            let Some(combo) = shortcut_combo(&text, ctrl, alt, shift) else {
                return false;
            };
            // Ctrl+P is the house key, not a binding: Ajustes, like the old
            // desktop.
            if combo == "Ctrl+P" {
                ui.set_open_settings_tick(ui.get_open_settings_tick() + 1);
                return true;
            }
            let action = map.borrow().get(&combo).cloned();
            let Some(action) = action else { return false };
            match action.as_str() {
                "play_pause" => ui.invoke_toggle_play(),
                "stop" => ui.invoke_stop_playback(),
                "prev_track" => ui.invoke_prev_track(),
                "next_track" => ui.invoke_next_track(),
                "volume_up" | "volume_down" => {
                    if let Some(p) = player.borrow_mut().as_mut() {
                        let d = if action == "volume_up" { 0.05 } else { -0.05 };
                        p.set_volume((p.volume() + d).clamp(0.0, 1.0));
                        ui.set_volume(p.volume());
                    }
                }
                "mute" => ui.invoke_toggle_mute(),
                "toggle_shuffle" => ui.invoke_toggle_shuffle(),
                "cycle_repeat" => ui.invoke_cycle_repeat(),
                "focus_search" => {
                    ui.set_focus_filter_tick(ui.get_focus_filter_tick() + 1)
                }
                "toggle_fav" => {
                    let r = ui.get_now_rating();
                    ui.invoke_rate_now(if r > 0 { r } else { 5 });
                }
                _ => return false,
            }
            true
        });
    }

    ui.set_about_label(SharedString::from(format!("v{} — jjolmo", update::CURRENT_VERSION)));
    ui.on_open_repo(|| {
        let _ = std::process::Command::new("xdg-open")
            .arg("https://github.com/jjolmo/tunante")
            .spawn();
    });
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_toggle_auto_covers(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = !ui.get_auto_covers();
            ui.set_auto_covers(next);
            let _ = db.set_setting("auto_download_cover_art", if next { "true" } else { "false" });
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_toggle_covers_in_folder(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = !ui.get_covers_in_folder();
            ui.set_covers_in_folder(next);
            let _ = db.set_setting("store_covers_in_folder", if next { "true" } else { "false" });
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_toggle_titlebar_track(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = !ui.get_titlebar_track();
            ui.set_titlebar_track(next);
            let _ = db.set_setting("show_track_in_titlebar", if next { "true" } else { "false" });
            if !next {
                ui.set_window_title(SharedString::new());
            }
        });
    }
    ui.set_album_game_label(SharedString::from(
        if table_state.borrow().album_game_prefers_game { "el juego" } else { "el álbum" },
    ));
    {
        let (db, st, model) = (db.clone(), table_state.clone(), table_model.clone());
        let weak = ui.as_weak();
        ui.on_toggle_album_game(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = st.borrow_mut();
            st.album_game_prefers_game = !st.album_game_prefers_game;
            let _ = db.set_setting(
                "album_game_prefers",
                if st.album_game_prefers_game { "game" } else { "album" },
            );
            ui.set_album_game_label(SharedString::from(
                if st.album_game_prefers_game { "el juego" } else { "el álbum" },
            ));
            if st.built {
                rebuild_table(&mut st, &model);
            }
        });
    }
    {
        let stored = db
            .get_setting("vgm_loop_count")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<f64>().ok());
        ui.set_vgm_loops_label(SharedString::from(vgm_loops_label(stored)));
    }
    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        ui.on_cycle_vgm_loops(move || {
            let Some(ui) = weak.upgrade() else { return };
            let current = db
                .get_setting("vgm_loop_count")
                .ok()
                .flatten()
                .and_then(|v| v.parse::<f64>().ok());
            // predet. → 1 → 2 → 3 → 5 → 10 → predet.
            let next = match current.map(|v| v as i64) {
                None => Some(1.0),
                Some(1) => Some(2.0),
                Some(2) => Some(3.0),
                Some(3) => Some(5.0),
                Some(5) => Some(10.0),
                _ => None,
            };
            let _ = db.set_setting(
                "vgm_loop_count",
                &next.map(|v| v.to_string()).unwrap_or_default(),
            );
            if let (Some(p), Some(v)) = (player.borrow_mut().as_mut(), next) {
                p.engine_mut().set_vgm_loop_count(v);
            }
            ui.set_vgm_loops_label(SharedString::from(vgm_loops_label(next)));
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_toggle_slow_scan(move || {
            let Some(ui) = weak.upgrade() else { return };
            let slow = !ui.get_slow_scan();
            ui.set_slow_scan(slow);
            let _ = db.set_setting("fast_scan", if slow { "false" } else { "true" });
        });
    }
    {
        let (db, weak, player) = (db.clone(), ui.as_weak(), player.clone());
        ui.on_cycle_crossfade(move || {
            let Some(ui) = weak.upgrade() else { return };
            // desactivado → 2 → 4 → 8 → desactivado, total de la transición.
            let next = match ui.get_crossfade_secs() {
                0 => 2,
                2 => 4,
                4 => 8,
                _ => 0,
            };
            ui.set_crossfade_secs(next);
            let _ = db.set_setting(
                "fade_on_track_change",
                if next > 0 { "true" } else { "false" },
            );
            if next > 0 {
                let _ = db.set_setting("fade_seconds", &next.to_string());
            }
            if let Some(p) = player.borrow_mut().as_mut() {
                let engine = p.engine_mut();
                engine.set_fade_on_track_change(next > 0);
                if next > 0 {
                    engine.set_fade_seconds(next as f32);
                }
            }
        });
    }

    // Close-to-tray: the close button hides instead of quitting, so the
    // tray's Mostrar/Ocultar (and its left-click on KDE) becomes the way
    // back. Off by default — surprising quits are worse than surprising
    // survivals, but only just.
    let close_to_tray = Rc::new(std::cell::Cell::new(
        db.get_setting("close_to_tray")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false),
    ));
    ui.set_close_to_tray(close_to_tray.get());
    {
        let flag = close_to_tray.clone();
        ui.window().on_close_requested(move || {
            if cfg!(all(target_os = "linux", feature = "tray")) && flag.get() {
                slint::CloseRequestResponse::HideWindow
            } else {
                let _ = slint::quit_event_loop();
                slint::CloseRequestResponse::HideWindow
            }
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        let flag = close_to_tray.clone();
        ui.on_toggle_close_to_tray(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = !flag.get();
            flag.set(next);
            ui.set_close_to_tray(next);
            let _ = db.set_setting("close_to_tray", if next { "true" } else { "false" });
        });
    }

    let log_model = Rc::new(VecModel::from(Vec::<SharedString>::new()));
    ui.set_log_lines(ModelRc::from(log_model.clone()));
    // 0 todo · 1 error · 2 aviso · 3 info; the filter is a plain substring,
    // both applied at refresh time so the ring itself stays raw.
    let log_level = Rc::new(std::cell::Cell::new(0u8));
    let log_filter: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    fn log_level_label(l: u8) -> &'static str {
        match l {
            1 => "solo error",
            2 => "aviso+",
            _ => "todo",
        }
    }
    fn refresh_log(
        model: &VecModel<SharedString>,
        level: u8,
        filter: &str,
    ) {
        let needle = filter.to_lowercase();
        model.set_vec(
            debuglog::lines()
                .into_iter()
                .filter(|l| match level {
                    1 => l.starts_with("[ERROR]"),
                    2 => l.starts_with("[ERROR]") || l.starts_with("[WARN]"),
                    _ => true,
                })
                .filter(|l| needle.is_empty() || l.to_lowercase().contains(&needle))
                .map(SharedString::from)
                .collect::<Vec<_>>(),
        );
    }
    {
        let (model, level, filter) = (log_model.clone(), log_level.clone(), log_filter.clone());
        let weak = ui.as_weak();
        ui.on_show_log(move || {
            let Some(ui) = weak.upgrade() else { return };
            refresh_log(&model, level.get(), &filter.borrow());
            ui.set_showing_log(true);
        });
    }
    {
        let (model, level, filter) = (log_model.clone(), log_level.clone(), log_filter.clone());
        let weak = ui.as_weak();
        ui.on_cycle_log_level(move || {
            let Some(ui) = weak.upgrade() else { return };
            let next = (level.get() + 1) % 3;
            level.set(next);
            ui.set_log_level_label(SharedString::from(log_level_label(next)));
            refresh_log(&model, next, &filter.borrow());
        });
    }
    {
        let (model, level, filter) = (log_model.clone(), log_level.clone(), log_filter.clone());
        ui.on_log_filter_changed(move |t| {
            *filter.borrow_mut() = t.to_string();
            refresh_log(&model, level.get(), &filter.borrow());
        });
    }
    {
        let (model, level, filter) = (log_model.clone(), log_level.clone(), log_filter.clone());
        ui.on_clear_log(move || {
            debuglog::clear();
            refresh_log(&model, level.get(), &filter.borrow());
        });
    }
    {
        let model = log_model.clone();
        ui.on_copy_log(move || {
            let text: String = (0..model.row_count())
                .filter_map(|i| model.row_data(i))
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            // wl-copy on Wayland, xclip as the fallback; both read stdin.
            std::thread::spawn(move || {
                use std::io::Write;
                let try_cmd = |cmd: &str, args: &[&str]| -> bool {
                    std::process::Command::new(cmd)
                        .args(args)
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                        .ok()
                        .and_then(|mut c| {
                            c.stdin.take()?.write_all(text.as_bytes()).ok()?;
                            c.wait().ok()
                        })
                        .is_some()
                };
                if !try_cmd("wl-copy", &[]) {
                    let _ = try_cmd("xclip", &["-selection", "clipboard"]);
                }
            });
        });
    }

    // --- Self-update -----------------------------------------------------
    //
    // Two taps: one asks GitHub, one installs what it offered. The row's text
    // is the whole state machine, and the workers report through a channel
    // the timer drains like every other background job here.
    let badge_fp: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let (update_tx, update_rx) = std::sync::mpsc::channel::<update::UpdateMsg>();
    let update_pending: Rc<RefCell<Option<(String, String)>>> = Rc::new(RefCell::new(None));
    ui.set_update_status(SharedString::from(format!(
        "v{} — toca para comprobar",
        update::CURRENT_VERSION
    )));
    // A tapped check must show everything; only the silent startup check
    // honours the skipped version.
    let update_manual = Rc::new(std::cell::Cell::new(false));
    let update_skipped: Option<String> = db.get_setting("update.skip_version").ok().flatten();
    if cfg!(all(target_os = "linux", feature = "updater")) && update::IS_RELEASE {
        update::spawn_check(update_tx.clone());
    }
    {
        let (pending, tx) = (update_pending.clone(), update_tx.clone());
        let (weak, manual) = (ui.as_weak(), update_manual.clone());
        ui.on_check_update(move || {
            let Some(ui) = weak.upgrade() else { return };
            let offered = pending.borrow_mut().take();
            match offered {
                Some((version, url)) => {
                    ui.set_update_skippable(false);
                    ui.set_update_status(SharedString::from(format!(
                        "Descargando v{version}…"
                    )));
                    update::spawn_install(tx.clone(), version, url);
                }
                None => {
                    manual.set(true);
                    ui.set_update_status(SharedString::from("Comprobando…"));
                    update::spawn_check(tx.clone());
                }
            }
        });
    }

    {
        let (db, pending) = (db.clone(), update_pending.clone());
        let weak = ui.as_weak();
        ui.on_skip_update(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some((version, _)) = pending.borrow_mut().take() {
                let _ = db.set_setting("update.skip_version", &version);
            }
            ui.set_update_skippable(false);
            ui.set_update_status(SharedString::from(format!(
                "v{} — toca para comprobar",
                update::CURRENT_VERSION
            )));
        });
    }

    ui.set_desktop_entry_label(SharedString::from("＋"));
    {
        let weak = ui.as_weak();
        ui.on_make_desktop_entry(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_desktop_entry_label(SharedString::from(
                match integrate::make_desktop_entry() {
                    Ok(()) => "hecho ✓".to_string(),
                    Err(e) => e,
                },
            ));
        });
    }

    // --- The equalizer section -----------------------------------------------
    //
    // One pattern five times: mutate the mirrored config, push it into the
    // engine's atomics (audible on the track already playing), persist it.
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_toggle_eq(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            c.eq_enabled = !c.eq_enabled;
            ui.set_eq_enabled(c.eq_enabled);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_set_eq_band(move |band, v| {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            let v = v.clamp(-12.0, 12.0);
            match band {
                0 => c.eq_low_db = v,
                1 => c.eq_mid_db = v,
                _ => c.eq_high_db = v,
            }
            // Dragging a band while the section is off would move a slider
            // that changes nothing — turning it on is what the gesture meant.
            c.eq_enabled = true;
            ui.set_eq_enabled(true);
            ui.set_eq_low(c.eq_low_db);
            ui.set_eq_mid(c.eq_mid_db);
            ui.set_eq_high(c.eq_high_db);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_set_preamp(move |v| {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            c.preamp_db = v.clamp(-12.0, 12.0);
            // A slider with no separate switch: zero means off, which is also
            // what the bar shows.
            c.preamp_enabled = c.preamp_db.abs() >= 0.5;
            ui.set_preamp_db(c.preamp_db);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_toggle_mono(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            c.mono = !c.mono;
            ui.set_dsp_mono(c.mono);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_toggle_mono_compensate(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            c.mono_compensate = !c.mono_compensate;
            ui.set_dsp_mono_compensate(c.mono_compensate);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_toggle_mono_phase(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            c.mono_phase_safe = !c.mono_phase_safe;
            ui.set_dsp_mono_phase(c.mono_phase_safe);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_toggle_limiter(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            c.limiter = !c.limiter;
            ui.set_dsp_limiter(c.limiter);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_set_balance(move |v| {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            // Snap the middle: nobody can drop a finger on exactly 0.0, and a
            // balance of 0.03 is indistinguishable from a broken speaker.
            c.balance = if v.abs() < 0.05 { 0.0 } else { v.clamp(-1.0, 1.0) };
            ui.set_dsp_balance(c.balance);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (cfg, db, player, weak) =
            (dsp_config.clone(), db.clone(), player.clone(), ui.as_weak());
        ui.on_set_width(move |v| {
            let Some(ui) = weak.upgrade() else { return };
            let mut c = cfg.borrow_mut();
            // Same snap at 1.0, and the enable flag follows the value — a
            // slider with no separate switch, like the preamp.
            c.width = if (v - 1.0).abs() < 0.05 { 1.0 } else { v.clamp(0.0, 2.0) };
            c.width_enabled = (c.width - 1.0).abs() > 0.01;
            ui.set_dsp_width(c.width);
            store_dsp(&db, &player, &c);
        });
    }
    {
        let (db, weak) = (db.clone(), ui.as_weak());
        ui.on_cycle_rating_priority(move || {
            let Some(ui) = weak.upgrade() else { return };
            let current = db
                .get_setting("rating_source_priority")
                .ok()
                .flatten()
                .unwrap_or_default();
            // Three sensible presets rather than a reorderable list: who
            // wins — the database, the file's tag, or the folder's m3u.
            let next = match current.as_str() {
                "file,folder,db" => "folder,file,db",
                "folder,file,db" => "db,file,folder",
                _ => "file,folder,db",
            };
            let _ = db.set_setting("rating_source_priority", next);
            ui.set_rating_priority_label(SharedString::from(rating_priority_label(
                Some(next),
            )));
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
            // Folders first, capped at 50 like the old «Find folder…» box;
            // tapping one reveals it in the tree (see library-activated).
            let folder_rows: Vec<library::Row> = tree
                .borrow()
                .matching_folders(&db, &q, 50)
                .into_iter()
                .map(|(path, n)| library::Row {
                    label: std::path::Path::new(&path)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone()),
                    detail: library::pistas(n),
                    depth: 0,
                    is_folder: true,
                    expanded: false,
                    path,
                })
                .collect();
            let rows: Vec<library::Row> = folder_rows
                .into_iter()
                .chain(hits
                .iter()
                .take(300)
                .map(|t| library::Row {
                    label: if t.title.is_empty() { t.path.clone() } else { t.title.clone() },
                    detail: library::format_duration(t.duration_ms),
                    depth: 0,
                    is_folder: false,
                    expanded: false,
                    path: t.path.clone(),
                }))
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
        let (views, library_dirty, sync_watches) =
            (views.clone(), library_dirty.clone(), sync_watches.clone());
        let (table_state, table_model) = (table_state.clone(), table_model.clone());
        let (cover_model, cover_urls) = (cover_model.clone(), cover_urls.clone());
        let bulk_plans_model = bulk_plans_model.clone();
        let (names_pending, names_rows_model) =
            (names_pending.clone(), names_rows_model.clone());
        let (reclass_gen, sugg_model) = (reclass_gen.clone(), sugg_model.clone());
        let log_model = log_model.clone();
        let (log_level, log_filter) = (log_level.clone(), log_filter.clone());
        let update_manual = update_manual.clone();
        let pinned_model = pinned_model.clone();
        let folders_model = folders_model.clone();
        let consoles_side = consoles_side.clone();
        let badge_fp = badge_fp.clone();
        let tray_click = tray_click.clone();
        let vol_tooltip_hold = Rc::new(std::cell::Cell::new(0u8));
        let pending_search = pending_search.clone();
        let art_try: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let theme_mode = theme_mode.clone();
        let update_pending = update_pending.clone();
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
                    ui.set_cover_busy(false);
                    art_cache.borrow_mut().clear();
                    let path = ui.get_now_path().to_string();
                    refresh_artwork(&ui, (!path.is_empty()).then_some(path.as_str()), MAX_ART_SIDE);
                    let rows = tree.borrow().rows(&db);
                    rows_model.set_vec(to_ui_rows(&rows));
                }

                // Following the system: the watcher thread refreshed its
                // atomic; apply only on an actual flip so the binding does
                // not churn every half second.
                if theme_mode.get() == 2 {
                    if let Some(dark) = theme_watch::prefers_dark() {
                        if ui.global::<Theme>().get_dark() != dark {
                            ui.global::<Theme>().set_dark(dark);
                        }
                    }
                }

                // The error toast ages out at ~8 s, the old desktop's clock.
                if !ui.get_error_toast().is_empty() {
                    let age = ui.get_error_toast_age() + 1;
                    if age >= 16 {
                        ui.set_error_toast(SharedString::new());
                        ui.set_error_toast_age(0);
                    } else {
                        ui.set_error_toast_age(age);
                    }
                }

                if ui.get_showing_log() {
                    refresh_log(&log_model, log_level.get(), &log_filter.borrow());
                }

                while let Ok(label) = loose_rx.try_recv() {
                    ui.set_add_files_label(SharedString::from(label));
                }

                // The updater's worker reported in.
                while let Ok(msg) = update_rx.try_recv() {
                    match msg {
                        update::UpdateMsg::UpToDate => {
                            ui.set_update_status(SharedString::from(format!(
                                "al día (v{})",
                                update::CURRENT_VERSION
                            )));
                        }
                        update::UpdateMsg::Available { version, url } => {
                            if !update_manual.get()
                                && update_skipped.as_deref() == Some(version.as_str())
                            {
                                // The startup check found the one version the
                                // user asked not to hear about again.
                                continue;
                            }
                            update_manual.set(false);
                            ui.set_update_status(SharedString::from(format!(
                                "v{version} disponible — toca para instalar"
                            )));
                            ui.set_update_skippable(true);
                            *update_pending.borrow_mut() = Some((version, url));
                        }
                        update::UpdateMsg::Installed(version) => {
                            ui.set_update_status(SharedString::from(format!(
                                "v{version} instalada — reinicia la app"
                            )));
                        }
                        update::UpdateMsg::Error(e) => {
                            // The silent startup check failing (no network,
                            // package-managed build) is not worth a row of
                            // red text nobody asked for.
                            if update_manual.get() {
                                ui.set_update_status(SharedString::from(e));
                            }
                            update_manual.set(false);
                        }
                    }
                }

                // The cover picker's worker reported in.
                while let Ok(msg) = cover_rx.try_recv() {
                    match msg {
                        CoverMsg::Status(text) => {
                            ui.set_cover_status(SharedString::from(text));
                        }
                        CoverMsg::Options(list) => {
                            let mut u = cover_urls.borrow_mut();
                            u.clear();
                            let rows: Vec<CoverCandidate> = list
                                .into_iter()
                                .map(|hit| {
                                    u.push(hit.url);
                                    let img = hit
                                        .bytes
                                        .and_then(|b| image::load_from_memory(&b).ok())
                                        .map(|d| d.thumbnail(192, 192).to_rgba8())
                                        .map(|rgba| {
                                            let mut buf = slint::SharedPixelBuffer::<
                                                slint::Rgba8Pixel,
                                            >::new(
                                                rgba.width(), rgba.height()
                                            );
                                            buf.make_mut_bytes().copy_from_slice(rgba.as_raw());
                                            slint::Image::from_rgba8(buf)
                                        })
                                        .unwrap_or_default();
                                    CoverCandidate {
                                        img,
                                        source: SharedString::from(hit.source),
                                        name: SharedString::from(hit.name),
                                        conf: SharedString::from(hit.conf),
                                    }
                                })
                                .collect();
                            cover_model.set_vec(rows);
                            ui.set_cover_status(SharedString::new());
                        }
                        CoverMsg::Saved => {
                            ui.set_cover_status(SharedString::new());
                            ui.set_covering(false);
                            // The folder-art cache remembers misses too; this
                            // is the flag that makes the new cover show up.
                            art_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        CoverMsg::BulkStatus(text) => {
                            ui.set_bulk_status(SharedString::from(text));
                        }
                        CoverMsg::BulkPlans(rows) => {
                            let would = rows.iter().filter(|r| r.3 == "se escribiría").count();
                            ui.set_bulk_status(SharedString::from(format!(
                                "{} juegos · {} se escribirían",
                                rows.len(),
                                would
                            )));
                            bulk_plans_model.set_vec(
                                rows.into_iter()
                                    .map(|(game, console, source, action)| PlanRow {
                                        game: SharedString::from(game),
                                        console: SharedString::from(console),
                                        source: SharedString::from(source),
                                        action: SharedString::from(action),
                                    })
                                    .collect::<Vec<_>>(),
                            );
                            ui.set_bulk_busy(false);
                        }
                        CoverMsg::NamesReady { titles, lengths, named } => {
                            let rows: Vec<SharedString> = titles
                                .iter()
                                .enumerate()
                                .map(|(i, t)| {
                                    let title = if t.is_empty() { "—" } else { t.as_str() };
                                    let len = lengths.get(i).map(String::as_str).unwrap_or("");
                                    SharedString::from(if len.is_empty() {
                                        format!("{:>2} · {}", i + 1, title)
                                    } else {
                                        format!("{:>2} · {} · {}", i + 1, title, len)
                                    })
                                })
                                .collect();
                            ui.set_names_status(SharedString::from(format!(
                                "{named} de {} con nombre. Se escribe un .m3u junto al fichero.",
                                titles.len()
                            )));
                            names_rows_model.set_vec(rows);
                            *names_pending.borrow_mut() =
                                Some((String::new(), titles, lengths));
                            ui.set_names_can_apply(true);
                        }
                        CoverMsg::NamesProblem(text) => {
                            ui.set_names_status(SharedString::from(text));
                            ui.set_names_can_apply(false);
                        }
                        CoverMsg::NamesApplied(n) => {
                            ui.set_names_status(SharedString::from(format!(
                                "{n} pistas renombradas."
                            )));
                            // The rows changed under every cache; the watcher's
                            // flag re-reads them all.
                            library_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        CoverMsg::ReclassSuggestions { generation, names } => {
                            if generation
                                == reclass_gen.load(std::sync::atomic::Ordering::SeqCst)
                            {
                                sugg_model.set_vec(
                                    names
                                        .into_iter()
                                        .map(SharedString::from)
                                        .collect::<Vec<_>>(),
                                );
                            }
                        }
                        CoverMsg::BulkDone { written, stamp } => {
                            ui.set_bulk_status(SharedString::from(format!(
                                "{written} carátulas escritas"
                            )));
                            ui.set_bulk_busy(false);
                            let _ = db.set_setting("mini.last_cover_run", &stamp.to_string());
                            ui.set_undo_covers_label(SharedString::from("disponible"));
                            art_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }

                // The watcher changed rows underneath: re-read whatever view
                // is on screen, and the table's caches with it.
                if library_dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    refresh_counts(&db, &ui);
                    refresh_pinned(&db, &pinned_model);
                    refresh_library_folders(&db, &folders_model);
                    refresh_sidebar_consoles(&db, &tree, &consoles_side);
                    refresh_library(&ui, &tree, &db, &views);
                    let mut st = table_state.borrow_mut();
                    if st.built {
                        st.all = db.get_all_tracks().unwrap_or_default();
                        rebuild_table(&mut st, &table_model);
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
                    // The folder list can have just grown; watch the newcomers.
                    sync_watches();
                }

                // A second launch knocked: bring the window up, and if it
                // carried a file, play it. Before the player borrow below —
                // play_from_path takes the Rc and borrows it itself.
                if let Some(msg) = instance.poll() {
                    let _ = ui.window().show();
                    if let Some(path) = msg.trim().strip_prefix("play ") {
                        play_from_path(&ui, &db, &player, &queue_model, path);
                    }
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
                            adopt_pending_context(p, &db);
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
                        // MPRIS Seek is a relative offset; the player's
                        // seek is absolute, so add from where we are.
                        mpris::Command::Seek(offset_ms) => {
                            let now = p.position_ms() as i64;
                            p.seek((now + offset_ms).max(0) as u64);
                        }
                    }
                    push_now_playing(&ui, p);
                    sync_queue_marker(p, &queue_model);
                }

                if let Some(q) = pending_search.borrow_mut().take() {
                    let _ = db.set_setting("search_query", &q);
                }

                // A fresh track with no art, and permission to go get it:
                // one request through the same resolver the bulk run uses,
                // art_dirty repaints when it lands. Tried once per track
                // per session — a miss is not a reason to hammer.
                if ui.get_auto_covers() {
                    let path = ui.get_now_path().to_string();
                    if !path.is_empty()
                        && *art_try.borrow() != path
                        && ui.get_now_art().size().width == 0
                    {
                        *art_try.borrow_mut() = path.clone();
                        if let Some(track) =
                            p.current().filter(|t| t.path == path).cloned()
                        {
                            let store = ui.get_covers_in_folder();
                            let dirty = std::sync::Arc::clone(&art_dirty);
                            ui.set_cover_busy(true);
                            std::thread::spawn(move || {
                                let opts = tunante_art::resolver::BulkOptions {
                                    min_confidence: tunante_art::Confidence::High,
                                    ..Default::default()
                                };
                                let req = cover_request_for(&track, store);
                                let _ = cover_resolver().resolve_many(
                                    vec![req],
                                    &opts,
                                    |_| {},
                                );
                                dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                            });
                        }
                    }
                }

                // The queue badges in the table: »N on rows waiting in the
                // user queue. Recomputed only when the queue or the table
                // actually changed — the fingerprint is cheap, 29k
                // set_row_data calls are not.
                {
                    let fp = {
                        let st = table_state.borrow();
                        let paths: Vec<&str> =
                            p.user_queue().iter().map(|t| t.path.as_str()).collect();
                        format!("{}|{}", st.stamp, paths.join("\u{1f}"))
                    };
                    if *badge_fp.borrow() != fp {
                        *badge_fp.borrow_mut() = fp;
                        let pos: std::collections::HashMap<String, usize> = p
                            .user_queue()
                            .iter()
                            .enumerate()
                            .map(|(i, t)| (t.path.clone(), i + 1))
                            .collect();
                        for i in 0..table_model.row_count() {
                            if let Some(mut r) = table_model.row_data(i) {
                                let want = pos
                                    .get(r.path.as_str())
                                    .map(|n| format!("»{n}"))
                                    .unwrap_or_default();
                                if r.queue_pos.as_str() != want {
                                    r.queue_pos = SharedString::from(want);
                                    table_model.set_row_data(i, r);
                                }
                            }
                        }
                    }
                }

                // A global shortcut, wherever the focus was.
                let pending: Vec<shortcuts::Msg> = shortcut_rx
                    .borrow()
                    .as_ref()
                    .map(|rx| rx.try_iter().collect())
                    .unwrap_or_default();
                for msg in pending {
                    match msg {
                        shortcuts::Msg::Status(text) => {
                            ui.set_global_shortcuts_label(SharedString::from(text));
                            continue;
                        }
                        shortcuts::Msg::PlayPause => p.toggle_play(),
                        shortcuts::Msg::Next => {
                            let _ = p.next();
                            adopt_pending_context(p, &db);
                        }
                        shortcuts::Msg::Prev => {
                            let _ = p.prev();
                        }
                    }
                    push_now_playing(&ui, p);
                    sync_queue_marker(p, &queue_model);
                }

                // A thumb button on the mouse, wherever the focus was.
                while let Ok(cmd) = button_rx.try_recv() {
                    match cmd {
                        buttons::ButtonCmd::Next => {
                            let _ = p.next();
                            adopt_pending_context(p, &db);
                        }
                        buttons::ButtonCmd::Prev => {
                            let _ = p.prev();
                        }
                    }
                    push_now_playing(&ui, p);
                    sync_queue_marker(p, &queue_model);
                }

                // Scroll over the tray icon: volume, five percent a notch,
                // like every SNI player before this one.
                let notches = tray::take_scroll();
                if notches != 0 {
                    let v = (p.volume() + notches as f32 * 0.05).clamp(0.0, 1.0);
                    p.set_volume(v);
                    ui.set_volume(p.volume());
                    // The pointer is on the icon (that is what scrolling is),
                    // so the tooltip is the old volume popup, for free. Held
                    // for ~1.5 s before the track tooltip takes it back.
                    tray::set_tooltip(&format!("Volumen {:.0}%", v * 100.0));
                    vol_tooltip_hold.set(3);
                }

                // Anything the tray menu asked for. Same shapes as MPRIS,
                // plus the two only a tray can mean: the window and the app.
                while let Some(action) = tray::poll() {
                    match action {
                        tray::TrayAction::PlayPause => p.toggle_play(),
                        tray::TrayAction::Next => {
                            let _ = p.next();
                            adopt_pending_context(p, &db);
                        }
                        tray::TrayAction::Prev => {
                            let _ = p.prev();
                        }
                        tray::TrayAction::ToggleWindow => {
                            // The configurable click action rides the one
                            // channel ayatana gives us.
                            match tray_click.borrow().as_str() {
                                "play_pause" => p.toggle_play(),
                                "stop" => p.stop(),
                                "next_track" => {
                                    let _ = p.next();
                                    adopt_pending_context(p, &db);
                                }
                                "next_track_with_fade" => {
                                    // Force the fade even when the setting is
                                    // off: flip it for this one change — the
                                    // machine reads it at play time.
                                    let was = ui.get_crossfade_secs();
                                    if was == 0 {
                                        let engine = p.engine_mut();
                                        engine.set_fade_on_track_change(true);
                                        engine.set_fade_seconds(4.0);
                                    }
                                    let _ = p.next();
                                    adopt_pending_context(p, &db);
                                    if was == 0 {
                                        p.engine_mut().set_fade_on_track_change(false);
                                    }
                                }
                                _ => {
                                    if ui.window().is_visible() {
                                        let _ = ui.window().hide();
                                    } else {
                                        let _ = ui.window().show();
                                        // Coming back, land on what plays —
                                        // the old restore-scroll, on the one
                                        // event this stack can actually see.
                                        if !ui.get_now_path().is_empty() {
                                            ui.invoke_now_clicked();
                                        }
                                    }
                                }
                            }
                        }
                        tray::TrayAction::Quit => {
                            let _ = slint::quit_event_loop();
                        }
                    }
                    push_now_playing(&ui, p);
                    sync_queue_marker(p, &queue_model);
                }

                if p.poll_track_end() {
                    adopt_pending_context(p, &db);
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
                    // Same cadence as the desktop's output supervisor: rebuild
                    // the stream if the device died under us or the system
                    // default moved (Bluetooth arriving is the everyday case).
                    // The engine rate-limits itself, so a quiet tick is one
                    // atomic swap and a device-name lookup.
                    if let Some(name) = p.reconcile_output() {
                        eprintln!("salida de audio recuperada: {name}");
                    }
                    let _ = db.set_setting(
                        "mini.was_playing",
                        if p.is_playing() { "true" } else { "false" },
                    );
                    if let Ok(now) = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                    {
                        let _ = db.set_setting("mini.closed_at", &now.as_secs().to_string());
                    }
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
                // The tray's tooltip says what the lock screen would —
                // unless a volume scroll just claimed it for a moment.
                if vol_tooltip_hold.get() > 0 {
                    vol_tooltip_hold.set(vol_tooltip_hold.get() - 1);
                } else {
                    tray::set_tooltip(&if title.is_empty() {
                        "Tunante".to_string()
                    } else if artist.is_empty() {
                        title.clone()
                    } else {
                        format!("{title} — {artist}")
                    });
                }
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
/// The two playlist models (visible + all), without the rest of Views —
/// for callers that only changed a playlist's contents.
fn refresh_playlists_models(
    db: &Database,
    visible: &VecModel<PlaylistRow>,
    all_pl: &VecModel<PlaylistRow>,
) {
    let all = db.get_playlists().unwrap_or_default();
    let rows: Vec<PlaylistRow> = all
        .iter()
        .map(|p| PlaylistRow {
            id: SharedString::from(p.id.as_str()),
            name: SharedString::from(p.name.as_str()),
            subtitle: SharedString::from(playlist_subtitle(p.track_count)),
        })
        .collect();
    visible.set_vec(rows.clone());
    all_pl.set_vec(rows);
}

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
        refresh_queue(p, &queue_model);
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
        refresh_queue(p, &queue_model);
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

/// Push a DSP config into the engine and remember it — the two halves of
/// every equalizer gesture.
fn store_dsp(
    db: &Database,
    player: &Rc<RefCell<Option<player::Player>>>,
    cfg: &tunante_core::dsp::DspConfig,
) {
    if let Some(p) = player.borrow_mut().as_mut() {
        cfg.apply_to(p.engine_mut().dsp());
    }
    if let Ok(json) = serde_json::to_string(cfg) {
        let _ = db.set_setting("dsp_config", &json);
    }
}

fn get_bool_setting(db: &Database, key: &str, default: bool) -> bool {
    db.get_setting(key)
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(default)
}

fn rating_priority_label(raw: Option<&str>) -> &'static str {
    match raw {
        Some("file,folder,db") => "fichero primero",
        Some("folder,file,db") => "carpeta primero",
        _ => "BD manda",
    }
}

fn tray_click_label(a: &str) -> &'static str {
    match a {
        "play_pause" => "reproducir/pausa",
        "stop" => "stop",
        "next_track" => "siguiente",
        "next_track_with_fade" => "siguiente con fundido",
        _ => "mostrar/ocultar",
    }
}

fn tray_style_label(style: u8) -> &'static str {
    match style {
        1 => "simbólico",
        2 => "logo",
        _ => "sistema",
    }
}

fn theme_mode_label(mode: u8) -> &'static str {
    match mode {
        1 => "claro",
        2 => "sistema",
        _ => "oscuro",
    }
}

fn ui_mode_label(mode: i32) -> &'static str {
    match mode {
        1 => "mini",
        2 => "escritorio",
        _ => "auto",
    }
}

/// What the output row shows. Device names are ALSA/Pulse strings that can
/// run to a hundred characters; the row elides, but a hard cap keeps the
/// label from eating the whole width first.
fn output_label(stored: &str) -> String {
    if stored == "system" {
        return "sistema".to_string();
    }
    let mut label: String = stored.chars().take(34).collect();
    if label.len() < stored.len() {
        label.push('…');
    }
    label
}

/// The desktop table's world: the whole library once, then whatever the
/// current filter and sort make of it. `all` is read from the database the
/// first time the pane exists and reused after — a rescan refreshes it the
/// next time the app starts, which is the same freshness the tree's indexes
/// live with.
/// What the desktop table is narrowed to. The old desktop showed every kind
/// of collection in the one powerful table — playlists and consoles included
/// — and the grande shell does the same: this is what makes the sidebar's
/// lists and consoles land in columns-and-sorting instead of the phone grid.
#[derive(Clone, Default, PartialEq)]
enum Scope {
    #[default]
    Library,
    Faved,
    Folder(String),
    Console(String),
    /// Ordered track ids, plus the playlist id for the sidebar highlight.
    Playlist { ids: Vec<String>, id: String },
}

impl Scope {
    /// (kind, id) for the sidebar to know what to light up.
    fn tag(&self) -> (&'static str, &str) {
        match self {
            Scope::Folder(_) => ("folder", ""),
            Scope::Console(id) => ("console", id),
            Scope::Playlist { id, .. } => ("playlist", id),
            _ => ("", ""),
        }
    }
}

struct TableState {
    all: Vec<tunante_core::db::models::Track>,
    tracks: Vec<tunante_core::db::models::Track>,
    sort_key: String,
    asc: bool,
    filter: String,
    /// Which catalog columns are visible, in DISPLAY order — the user can
    /// drag headers around, so the vector's order is the table's order.
    visible: Vec<String>,
    /// Per-key width weights (same unit as ColumnDef.fraction), only for
    /// columns the user has resized by hand; everything else keeps its
    /// default. Persisted as `key:weight` in mini.table_columns.
    widths: std::collections::HashMap<String, f32>,
    /// What the table is narrowed to (library / faved / folder / console /
    /// playlist). The old desktop's context-aware TrackList, one field.
    scope: Scope,
    built: bool,
    /// Indices into `tracks`. Cleared on every rebuild: a sort or a filter
    /// reshuffles what the indices mean, and a stale selection pointing at
    /// different songs is worse than an empty one.
    selected: std::collections::HashSet<usize>,
    /// Where a Shift-range grows from.
    anchor: usize,
    /// «Álbum / Juego» shows which of the two first — the old desktop's
    /// `album_game_prefers` setting; the other is the fallback.
    album_game_prefers_game: bool,
    /// Bumped by every rebuild, so the queue-badge pass in the timer knows
    /// the rows are fresh (and badge-less) even when the queue is not.
    stamp: u64,
}

impl Default for TableState {
    /// By title, ascending — matching the `table-sort-col: 1` the UI declares,
    /// so the arrow in the header tells the truth before the first click.
    fn default() -> Self {
        Self {
            all: Vec::new(),
            tracks: Vec::new(),
            sort_key: "title".to_string(),
            asc: true,
            filter: String::new(),
            visible: DEFAULT_COLUMNS.split(',').map(str::to_string).collect(),
            widths: std::collections::HashMap::new(),
            scope: Scope::Library,
            built: false,
            selected: std::collections::HashSet::new(),
            anchor: 0,
            album_game_prefers_game: false,
            stamp: 0,
        }
    }
}

/// What one search worker sends back for one candidate.
struct CoverHit {
    bytes: Option<Vec<u8>>,
    source: String,
    name: String,
    conf: String,
    url: String,
}

enum CoverMsg {
    Status(String),
    Options(Vec<CoverHit>),
    Saved,
    BulkStatus(String),
    /// game, console label, source, action — the dry run's findings.
    BulkPlans(Vec<(String, String, String, String)>),
    BulkDone { written: usize, stamp: u64 },
    NamesReady {
        titles: Vec<String>,
        lengths: Vec<String>,
        named: usize,
    },
    NamesProblem(String),
    NamesApplied(usize),
    /// Generation-stamped: typing outruns the network, and a stale answer
    /// must never overwrite a fresher one.
    ReclassSuggestions { generation: u64, names: Vec<String> },
}

/// The desktop's suggest_game_names, on a worker: what the library already
/// calls things, the Libretro index for the machine (No-Intro names — the
/// exact strings the cover downloader will later match), and Steam for what
/// no console archive carries.
fn spawn_reclass_suggest(
    tx: std::sync::mpsc::Sender<CoverMsg>,
    generation: u64,
    console_id: String,
    query: String,
    library: Vec<String>,
) {
    std::thread::spawn(move || {
        let q = query.trim().to_lowercase();
        let mut out: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = Default::default();
        let mut push = |name: String, out: &mut Vec<String>| {
            if seen.insert(name.to_lowercase()) {
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
            (if l.starts_with(&q) { 0 } else { 1 }, n.len(), l)
        });
        out.truncate(12);
        let _ = tx.send(CoverMsg::ReclassSuggestions { generation, names: out });
    });
}

/// Ask the archive for the track names of a one-file-per-game rip, off the UI
/// thread. Fetches and counts, refuses on any mismatch — position is the
/// entire mapping, and a listing of the wrong length would rename every track
/// to the wrong song.
fn spawn_names_fetch(
    tx: std::sync::mpsc::Sender<CoverMsg>,
    system: &'static str,
    game: String,
    subsongs: usize,
) {
    std::thread::spawn(move || {
        use tunante_art::tracklist;
        let http = tunante_art::http::UreqHttp::default();
        let entries = tracklist::fetch(&http, system, &game);
        if entries.is_empty() {
            let _ = tx.send(CoverMsg::NamesProblem(format!(
                "No hay listado para «{game}» en el archivo."
            )));
            return;
        }
        if !tracklist::matches_subsongs(&entries, subsongs) {
            let _ = tx.send(CoverMsg::NamesProblem(format!(
                "El archivo lista {} pistas y este fichero tiene {subsongs}. \
                 La posición es todo el mapeo: otra cuenta es otro rip, y \
                 aplicarlo renombraría cada pista mal.",
                entries.len()
            )));
            return;
        }
        let named = tracklist::named_count(&entries);
        if named == 0 {
            let _ = tx.send(CoverMsg::NamesProblem(format!(
                "El archivo lista {} pistas de «{game}» pero no ha nombrado \
                 ninguna — todo son «Track N». No hay nada que renombrar.",
                entries.len()
            )));
            return;
        }
        let _ = tx.send(CoverMsg::NamesReady {
            titles: entries
                .iter()
                .map(|e| {
                    if tracklist::is_placeholder(e) {
                        String::new()
                    } else {
                        e.title.clone()
                    }
                })
                .collect(),
            lengths: entries.iter().map(|e| e.length.clone()).collect(),
            named,
        });
    });
}

/// Write the names as an `.m3u` beside the file and re-seal the library rows
/// through the ordinary path: a fresh probe, which already prefers an m3u
/// title over everything else. Never overwrites a playlist Tunante did not
/// write — one already there was put there by somebody.
fn spawn_names_apply(
    tx: std::sync::mpsc::Sender<CoverMsg>,
    dbfile: PathBuf,
    file: String,
    titles: Vec<String>,
    lengths: Vec<String>,
) {
    std::thread::spawn(move || {
        use tunante_art::tracklist;
        let path = std::path::PathBuf::from(&file);
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            let _ = tx.send(CoverMsg::NamesProblem("no es un fichero".into()));
            return;
        };
        let m3u = path.with_extension("m3u");
        if m3u.exists() {
            let ours = std::fs::read_to_string(&m3u)
                .map(|b| tracklist::is_ours(&b))
                .unwrap_or(false);
            if !ours {
                let _ = tx.send(CoverMsg::NamesProblem(format!(
                    "{} ya existe junto al fichero y no lo escribió Tunante. \
                     Reemplazarlo tiraría el trabajo de alguien.",
                    m3u.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                )));
                return;
            }
        }
        let entries: Vec<tracklist::Entry> = titles
            .iter()
            .enumerate()
            .map(|(i, t)| tracklist::Entry {
                number: i as u32 + 1,
                title: t.clone(),
                length: lengths.get(i).cloned().unwrap_or_default(),
            })
            .collect();
        if let Err(e) = std::fs::write(&m3u, tracklist::to_m3u(&name, &entries)) {
            let _ = tx.send(CoverMsg::NamesProblem(format!(
                "no se pudo escribir {}: {e}",
                m3u.display()
            )));
            return;
        }

        // Re-read through the pipe and swap the rows — the watcher's own
        // recipe, on the watcher's own kind of private connection.
        let Ok(values) = tunante_helper::probe(
            &path,
            tunante_helper::scan::PROBE_TIMEOUT,
            true,
        ) else {
            let _ = tx.send(CoverMsg::NamesProblem(
                "el m3u se escribió, pero la relectura falló".into(),
            ));
            return;
        };
        let Ok(db) = Database::new(&dbfile) else {
            let _ = tx.send(CoverMsg::NamesProblem("sin base de datos".into()));
            return;
        };
        let _ = db.remove_tracks_by_base_path(&file);
        let mut n = 0usize;
        for v in values {
            if let Ok(track) = serde_json::from_value::<tunante_core::db::models::Track>(v) {
                if db.insert_track(&track).is_ok() {
                    n += 1;
                }
            }
        }
        let _ = tx.send(CoverMsg::NamesApplied(n));
    });
}

/// The whole-library cover run, both halves: `dry` previews, `!dry` writes
/// behind a manifest so exactly this run can be undone. One request per game
/// — a hundred tracks of one soundtrack want one cover between them —
/// deduplicated on the matcher's notion of sameness.
fn spawn_bulk_covers(
    tx: std::sync::mpsc::Sender<CoverMsg>,
    tracks: Vec<tunante_core::db::models::Track>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dry: bool,
) {
    std::thread::spawn(move || {
        let mut seen = std::collections::HashSet::new();
        let reqs: Vec<tunante_art::resolver::CoverRequest> = tracks
            .iter()
            .filter(|t| {
                seen.insert((
                    t.console_id.clone(),
                    tunante_art::name::normalize(&t.game).key,
                ))
            })
            .map(|t| cover_request_for(t, !dry))
            .collect();

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let manifest = (!dry)
            .then(|| {
                tunante_art::folder::Manifest::new(&tunante_art::cache::cache_dir(), stamp).ok()
            })
            .flatten();

        let opts = tunante_art::resolver::BulkOptions {
            dry_run: dry,
            min_confidence: tunante_art::Confidence::High,
            cancel,
            ..Default::default()
        };
        let verb = if dry { "Buscando" } else { "Descargando" };
        let started = std::time::Instant::now();
        let plans = cover_resolver().resolve_many(reqs, &opts, |p| {
            // The old panel's humanized ETA, from the pace so far.
            let eta = if p.done > 0 && p.done < p.total {
                let secs = started.elapsed().as_secs_f64() / p.done as f64
                    * (p.total - p.done) as f64;
                if secs < 60.0 {
                    " · menos de un minuto".to_string()
                } else if secs < 3600.0 {
                    format!(" · ~{} min", (secs / 60.0).round() as u64)
                } else {
                    format!(" · ~{}h {}m", secs as u64 / 3600, (secs as u64 % 3600) / 60)
                }
            } else {
                String::new()
            };
            let current = if p.current.is_empty() {
                String::new()
            } else {
                format!("\n{}", p.current)
            };
            let _ = tx.send(CoverMsg::BulkStatus(format!(
                "{verb}… {}/{} · {} encontradas{eta}{current}",
                p.done, p.total, p.found
            )));
        });

        if dry {
            let rows = plans
                .iter()
                .map(|p| {
                    let action = if p.existing.is_some() {
                        "ya tiene (se conserva)"
                    } else if p.source == "none" || p.url.is_none() {
                        "sin resultado"
                    } else {
                        "se escribiría"
                    };
                    (
                        p.game.clone(),
                        tunante_core::console::by_id(&p.console_id)
                            .map(|c| c.name_es.to_string())
                            .unwrap_or_default(),
                        if p.source == "none" { String::new() } else { p.source.clone() },
                        action.to_string(),
                    )
                })
                .collect();
            let _ = tx.send(CoverMsg::BulkPlans(rows));
        } else {
            let mut written = 0usize;
            for p in plans.iter().filter_map(|p| p.written.as_ref()) {
                if let Some(m) = &manifest {
                    let _ = m.record(std::path::Path::new(p));
                }
                written += 1;
            }
            let _ = tx.send(CoverMsg::BulkDone { written, stamp });
        }
    });
}

/// One resolver for the whole app: it owns the HTTP agent with its per-host
/// gates and the archive caches, and all of that is worth sharing.
fn cover_resolver() -> std::sync::Arc<tunante_art::resolver::Resolver> {
    static R: std::sync::OnceLock<std::sync::Arc<tunante_art::resolver::Resolver>> =
        std::sync::OnceLock::new();
    std::sync::Arc::clone(
        R.get_or_init(|| std::sync::Arc::new(tunante_art::resolver::Resolver::new())),
    )
}

/// Turn a track into a lookup — the mirror of the desktop's `request_for`,
/// candidate order included: the resolved game first, because for a rip that
/// is the album tag even when the folder is called `ct/`.
fn cover_request_for(
    track: &tunante_core::db::models::Track,
    store_in_folder: bool,
) -> tunante_art::resolver::CoverRequest {
    let candidates =
        tunante_art::resolver::candidates_for(&track.game, &track.album, &track.path);
    let (real, _) = tunante_core::vgm_path::parse_vgm_path(&track.path);
    let dir = store_in_folder
        .then(|| std::path::Path::new(real).parent().map(|p| p.to_path_buf()))
        .flatten();
    let all: Vec<(String, String)> = tunante_core::console::CONSOLES
        .iter()
        .filter_map(|c| c.libretro.map(|s| (c.id.to_string(), s.to_string())))
        .collect();
    tunante_art::resolver::CoverRequest {
        libretro_system: tunante_core::console::by_id(&track.console_id)
            .and_then(|c| c.libretro)
            .map(str::to_string),
        other_systems: all
            .into_iter()
            .filter(|(o, _)| *o != track.console_id)
            .collect(),
        console_id: track.console_id.clone(),
        candidates,
        dir,
    }
}

/// Ask every archive and service, then fetch the thumbnails, all off the UI
/// thread. Twelve candidates: enough to pick from, few enough that the
/// downloads finish while the person is still reading the names.
fn spawn_cover_search(
    tx: std::sync::mpsc::Sender<CoverMsg>,
    track: tunante_core::db::models::Track,
    query: Option<String>,
) {
    std::thread::spawn(move || {
        use tunante_art::http::Http;
        let req = cover_request_for(&track, false);
        let hits = cover_resolver().options(&req, query.as_deref(), 12);
        if hits.is_empty() {
            let _ = tx.send(CoverMsg::Status(
                "Sin resultados. Prueba con otro nombre.".to_string(),
            ));
            return;
        }
        let http = tunante_art::http::UreqHttp::default();
        let out = hits
            .into_iter()
            .map(|h| CoverHit {
                bytes: http
                    .get(&h.url, 4 * 1024 * 1024)
                    .ok()
                    .filter(|r| r.is_success())
                    .map(|r| r.body),
                source: h.source.to_string(),
                name: h.matched_name,
                conf: match h.confidence {
                    tunante_art::Confidence::Exact => "exacta",
                    tunante_art::Confidence::High => "alta",
                    tunante_art::Confidence::Medium => "media",
                    _ => "baja",
                }
                .to_string(),
                url: h.url,
            })
            .collect();
        let _ = tx.send(CoverMsg::Options(out));
    });
}

/// The scan knobs, read where every probing path can share them. Mini's
/// tradition is the fast scan (a phone cannot afford silence detection), so
/// `fast_scan` defaults ON here — the desktop defaulted off. "false" in the
/// shared key means someone asked for the thorough one.
fn probe_opts(db: &Database) -> tunante_helper::ProbeOpts {
    let get = |k: &str| db.get_setting(k).ok().flatten();
    tunante_helper::ProbeOpts {
        fast: get("fast_scan").map(|v| v != "false").unwrap_or(true),
        loop_max_ms: get("loop_max_seconds")
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|s| *s > 0)
            .map(|s| s * 1000),
        vgm_loop_count: get("vgm_loop_count").and_then(|v| v.parse::<f64>().ok()),
        caps_all: get("loop_max_caps_all").map(|v| v == "true").unwrap_or(false),
    }
}

/// With continue-from-queue on, a user-queued track from elsewhere asks for
/// its own context once it plays. Called after every advance that can pop
/// the user queue.
fn adopt_pending_context(p: &mut player::Player, db: &Database) {
    if let Some(t) = p.take_pending_context() {
        let folder = std::path::Path::new(
            tunante_core::vgm_path::parse_vgm_path(&t.path).0,
        )
        .parent()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_default();
        let tracks = db.get_tracks_by_folder(&folder).unwrap_or_default();
        if !tracks.is_empty() {
            p.adopt_context(tracks, &t.id);
        }
    }
}

/// The column catalog: everything the table can show. `fraction` values are
/// relative weights, normalised when the visible subset is built.
struct ColumnDef {
    key: &'static str,
    label: &'static str,
    fraction: f32,
    right: bool,
}

const TABLE_COLUMNS: &[ColumnDef] = &[
    ColumnDef { key: "n", label: "#", fraction: 0.5, right: false },
    ColumnDef { key: "title", label: "Título", fraction: 3.4, right: false },
    ColumnDef { key: "artist", label: "Artista", fraction: 2.2, right: false },
    ColumnDef { key: "album", label: "Álbum", fraction: 2.4, right: false },
    ColumnDef { key: "game", label: "Juego", fraction: 2.4, right: false },
    ColumnDef { key: "console", label: "Consola", fraction: 1.3, right: false },
    ColumnDef { key: "albumgame", label: "Álbum / Juego", fraction: 2.4, right: false },
    ColumnDef { key: "albumartist", label: "Artista del álbum", fraction: 2.0, right: false },
    ColumnDef { key: "disc", label: "Disco", fraction: 0.6, right: true },
    ColumnDef { key: "stars", label: "★", fraction: 1.4, right: false },
    ColumnDef { key: "duration", label: "Duración", fraction: 1.0, right: true },
    ColumnDef { key: "codec", label: "Códec", fraction: 1.0, right: false },
    ColumnDef { key: "bitrate", label: "Bitrate", fraction: 1.0, right: true },
    ColumnDef { key: "samplerate", label: "Muestreo", fraction: 1.1, right: true },
    ColumnDef { key: "channels", label: "Canales", fraction: 0.9, right: true },
    ColumnDef { key: "size", label: "Tamaño", fraction: 1.0, right: true },
    ColumnDef { key: "path", label: "Ruta", fraction: 3.4, right: false },
];

const DEFAULT_COLUMNS: &str = "n,title,artist,game,console,stars,duration";

/// One cell, painted. The UI never computes a cell — the GridLine rule.
fn cell_for(t: &tunante_core::db::models::Track, key: &str, prefers_game: bool) -> String {
    match key {
        "albumgame" => {
            let (first, second) = if prefers_game {
                (&t.game, &t.album)
            } else {
                (&t.album, &t.game)
            };
            if first.is_empty() { second.clone() } else { first.clone() }
        }
        "n" => t.track_number.map(|n| n.to_string()).unwrap_or_default(),
        "title" => {
            if t.title.is_empty() { t.path.clone() } else { t.title.clone() }
        }
        "artist" => t.artist.clone(),
        "album" => t.album.clone(),
        "game" => t.game.clone(),
        "console" => table_console_label(t).to_string(),
        "albumartist" => t.album_artist.clone(),
        "disc" => t.disc_number.map(|n| n.to_string()).unwrap_or_default(),
        "samplerate" => t
            .sample_rate
            .map(|r| format!("{r} Hz"))
            .unwrap_or_default(),
        "channels" => match t.channels {
            Some(1) => "mono".to_string(),
            Some(2) => "estéreo".to_string(),
            Some(n) => n.to_string(),
            None => String::new(),
        },
        "stars" => stars_for(t.rating),
        "duration" => format!("{}:{:02}", t.duration_ms / 60_000, (t.duration_ms / 1_000) % 60),
        "codec" => t.codec.clone(),
        "bitrate" => t
            .bitrate
            .map(|b| format!("{b} kbps"))
            .unwrap_or_default(),
        "size" => {
            if t.file_size <= 0 {
                String::new()
            } else if t.file_size < 1024 * 1024 {
                format!("{} KB", t.file_size / 1024)
            } else {
                format!("{:.1} MB", t.file_size as f64 / (1024.0 * 1024.0))
            }
        }
        "path" => t.path.clone(),
        _ => String::new(),
    }
}

/// Rebuild the column models the UI paints from: the visible subset with
/// fractions normalised to sum 1, and the chooser with its ticks.
/// The `mini.table_columns` value: keys in display order, each carrying its
/// hand-set weight when there is one.
fn persist_columns(st: &TableState) -> String {
    st.visible
        .iter()
        .map(|k| match st.widths.get(k) {
            Some(w) => format!("{k}:{w:.4}"),
            None => k.clone(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn rebuild_columns(
    ui: &AppWindow,
    st: &TableState,
    columns_model: &VecModel<TableColumn>,
    choices_model: &VecModel<ColumnChoice>,
) {
    // The visible vector's order is the display order — headers are
    // draggable — and a hand-resized column keeps its weight.
    let defs: Vec<&ColumnDef> = st
        .visible
        .iter()
        .filter_map(|k| TABLE_COLUMNS.iter().find(|d| d.key == *k))
        .collect();
    let weight = |d: &ColumnDef| st.widths.get(d.key).copied().unwrap_or(d.fraction);
    let total: f32 = defs.iter().map(|d| weight(d)).sum();
    columns_model.set_vec(
        defs.iter()
            .map(|d| TableColumn {
                key: SharedString::from(d.key),
                label: SharedString::from(d.label),
                fraction: weight(d) / total.max(0.001),
                right: d.right,
            })
            .collect::<Vec<_>>(),
    );
    choices_model.set_vec(
        TABLE_COLUMNS
            .iter()
            .map(|d| ColumnChoice {
                key: SharedString::from(d.key),
                label: SharedString::from(d.label),
                shown: st.visible.iter().any(|k| k == d.key),
            })
            .collect::<Vec<_>>(),
    );
    let _ = ui;
}

/// Push the selection set back into the rows. A full pass with one set_vec:
/// selection changes happen at click speed, and the spike put a whole-model
/// swap at 11–21 ms over 30k rows — simpler than bookkeeping point updates.
fn repaint_selection(st: &TableState, model: &VecModel<TableRow>) {
    // Per-row updates, never set_vec: replacing the whole model makes the
    // ListView rebuild every row element, and the TouchArea that counted
    // the first click of a double-click dies with it — which is why
    // double-clicking a track played nothing for the first human to try.
    // set_row_data only notifies the row, and only changed rows at that.
    for i in 0..model.row_count() {
        if let Some(mut r) = model.row_data(i) {
            let should = st.selected.contains(&i);
            if r.selected != should {
                r.selected = should;
                model.set_row_data(i, r);
            }
        }
    }
}

/// Five glyphs, filled up to the rating. Pre-painted here because the UI
/// never needs the number back — a click reports which star was hit.
fn stars_for(rating: i32) -> String {
    let r = rating.clamp(0, 5) as usize;
    "★".repeat(r) + &"☆".repeat(5 - r)
}

fn table_console_label(t: &tunante_core::db::models::Track) -> &'static str {
    tunante_core::console::label_es(tunante_core::console::key_of(t))
}

/// The sidebar's pinned folders, re-read whole: the list is short and the
/// database is the one truth about it.
fn refresh_pinned(db: &Database, model: &VecModel<PinnedRow>) {
    let row = |id: String, path: &str, removable: bool| PinnedRow {
        name: SharedString::from(
            std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string()),
        ),
        count: SharedString::from(
            db.count_tracks_under(path)
                .map(|n| n.to_string())
                .unwrap_or_default(),
        ),
        id: SharedString::from(id),
        removable,
    };
    // The library's roots first (they leave through Ajustes, never from
    // here), then the pinned folders with their hover ✕ — the old
    // sidebar's Folders section, both kinds in one list.
    let mut rows: Vec<PinnedRow> = db
        .get_monitored_folders()
        .unwrap_or_default()
        .into_iter()
        .map(|f| row(format!("root:{}", f.id), &f.path, false))
        .collect();
    rows.extend(
        db.get_pinned_folders()
            .unwrap_or_default()
            .into_iter()
            .map(|f| row(f.id.clone(), &f.path, true)),
    );
    model.set_vec(rows);
}

/// The chip text for the table's current scope, or "" for library/faved.
fn scope_label(db: &Database, scope: &Scope) -> String {
    match scope {
        Scope::Console(id) => format!("Consola · {}", tunante_core::console::label_es(id)),
        Scope::Folder(f) => format!(
            "Carpeta · {}",
            std::path::Path::new(f)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| f.clone())
        ),
        Scope::Playlist { id, .. } => {
            let name = db
                .get_playlists()
                .unwrap_or_default()
                .into_iter()
                .find(|p| p.id == *id)
                .map(|p| p.name)
                .unwrap_or_default();
            format!("Lista · {name}")
        }
        _ => String::new(),
    }
}

/// A sidebar folder id back to its path: `root:<id>` rows live in
/// monitored_folders, plain ids in pinned_folders.
fn sidebar_folder_path(db: &Database, id: &str) -> Option<String> {
    match id.strip_prefix("root:") {
        Some(rid) => db
            .get_monitored_folders()
            .ok()?
            .into_iter()
            .find(|f| f.id == rid)
            .map(|f| f.path),
        None => db
            .get_pinned_folders()
            .ok()?
            .into_iter()
            .find(|f| f.id == id)
            .map(|f| f.path),
    }
}

/// The consoles that hold music, for the sidebar section — PlaylistRow
/// reused (id, name, count-as-subtitle).
fn refresh_sidebar_consoles(
    db: &Database,
    tree: &Rc<RefCell<library::Tree>>,
    model: &VecModel<PlaylistRow>,
) {
    model.set_vec(
        tree.borrow()
            .console_counts(db)
            .into_iter()
            .map(|(id, name, n)| PlaylistRow {
                id: SharedString::from(id),
                name: SharedString::from(name),
                subtitle: SharedString::from(n.to_string()),
            })
            .collect::<Vec<_>>(),
    );
}

/// The library's roots as Ajustes shows them: removable, watchable.
fn refresh_library_folders(db: &Database, model: &VecModel<FolderRow>) {
    model.set_vec(
        db.get_monitored_folders()
            .unwrap_or_default()
            .into_iter()
            .map(|f| FolderRow {
                id: SharedString::from(f.id),
                path: SharedString::from(f.path),
                watching: f.watching_enabled,
            })
            .collect::<Vec<_>>(),
    );
}

/// The sidebar's numbers: every entry that is a set says how big it is.
fn refresh_counts(db: &Database, ui: &AppWindow) {
    if let Ok((total, faved)) = db.count_tracks() {
        ui.set_total_count(SharedString::from(total.to_string()));
        ui.set_faved_count(SharedString::from(faved.to_string()));
    }
}

/// Apply the filter and the sort, and hand the result to the UI model.
fn rebuild_table(st: &mut TableState, model: &VecModel<TableRow>) {
    st.selected.clear();
    let needle = library::plegar(&st.filter);
    // The scope decides the base set; a playlist keeps its own order, so it
    // is built by id rather than filtered out of `all`.
    let base: Vec<tunante_core::db::models::Track> = match &st.scope {
        Scope::Playlist { ids, .. } => {
            let by_id: std::collections::HashMap<&str, &tunante_core::db::models::Track> =
                st.all.iter().map(|t| (t.id.as_str(), t)).collect();
            ids.iter()
                .filter_map(|id| by_id.get(id.as_str()).map(|t| (*t).clone()))
                .collect()
        }
        _ => st
            .all
            .iter()
            .filter(|t| match &st.scope {
                Scope::Faved => t.rating > 0,
                Scope::Folder(f) => {
                    // Boundary-aware: /a/b must not catch /a/bc.
                    let (real, _) = tunante_core::vgm_path::parse_vgm_path(&t.path);
                    real.strip_prefix(f.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                }
                Scope::Console(c) => tunante_core::console::key_of(t) == c.as_str(),
                _ => true,
            })
            .cloned()
            .collect(),
    };
    let mut tracks: Vec<_> = base
        .into_iter()
        .filter(|t| {
            needle.is_empty()
                || library::plegar(&t.title).contains(&needle)
                || library::plegar(&t.artist).contains(&needle)
                || library::plegar(&t.game).contains(&needle)
        })
        .collect();
    // A playlist with the sentinel sort keeps its stored order untouched.
    let keep_order = matches!(st.scope, Scope::Playlist { .. }) && st.sort_key == "__scope__";
    if !keep_order {

    match st.sort_key.as_str() {
        "n" => tracks.sort_by_key(|t| t.track_number.unwrap_or(0)),
        "artist" => tracks.sort_by(|a, b| library::plegar(&a.artist).cmp(&library::plegar(&b.artist))),
        "album" => tracks.sort_by(|a, b| library::plegar(&a.album).cmp(&library::plegar(&b.album))),
        "game" => tracks.sort_by(|a, b| library::plegar(&a.game).cmp(&library::plegar(&b.game))),
        "console" => tracks.sort_by(|a, b| table_console_label(a).cmp(table_console_label(b))),
        "albumgame" => {
            let g = st.album_game_prefers_game;
            tracks.sort_by(|a, b| {
                let pick = |t: &tunante_core::db::models::Track| {
                    let (first, second) = if g { (&t.game, &t.album) } else { (&t.album, &t.game) };
                    library::plegar(if first.is_empty() { second } else { first })
                };
                pick(a).cmp(&pick(b))
            });
        }
        "albumartist" => {
            tracks.sort_by(|a, b| library::plegar(&a.album_artist).cmp(&library::plegar(&b.album_artist)))
        }
        "disc" => tracks.sort_by_key(|t| t.disc_number.unwrap_or(0)),
        "samplerate" => tracks.sort_by_key(|t| t.sample_rate.unwrap_or(0)),
        "channels" => tracks.sort_by_key(|t| t.channels.unwrap_or(0)),
        "stars" => tracks.sort_by_key(|t| t.rating),
        "duration" => tracks.sort_by_key(|t| t.duration_ms),
        "codec" => tracks.sort_by(|a, b| a.codec.cmp(&b.codec)),
        "bitrate" => tracks.sort_by_key(|t| t.bitrate.unwrap_or(0)),
        "size" => tracks.sort_by_key(|t| t.file_size),
        "path" => tracks.sort_by(|a, b| a.path.cmp(&b.path)),
        _ => tracks.sort_by(|a, b| library::plegar(&a.title).cmp(&library::plegar(&b.title))),
    }
    if !st.asc {
        tracks.reverse();
    }
    }

    model.set_vec(
        tracks
            .iter()
            .map(|t| TableRow {
                cells: ModelRc::new(VecModel::from(
                    st.visible
                        .iter()
                        .map(|k| SharedString::from(cell_for(t, k, st.album_game_prefers_game)))
                        .collect::<Vec<_>>(),
                )),
                path: SharedString::from(t.path.as_str()),
                selected: false,
                queue_pos: SharedString::new(),
                tip: SharedString::from({
                    let title = if t.title.is_empty() { t.path.as_str() } else { t.title.as_str() };
                    let mut lines = vec![title.to_string()];
                    let mid: Vec<&str> = [t.artist.as_str(), t.game.as_str(), t.album.as_str()]
                        .into_iter()
                        .filter(|v| !v.is_empty())
                        .collect();
                    if !mid.is_empty() {
                        lines.push(mid.join(" · "));
                    }
                    lines.push(t.path.clone());
                    lines.join("\n")
                }),
            })
            .collect::<Vec<_>>(),
    );
    st.tracks = tracks;
    st.stamp = st.stamp.wrapping_add(1);
}

/// Play a file by path, with its folder as the queue context — or, when the
/// library has never seen it, whatever the decoder says the file contains.
/// The five cover-fit modes, as the desktop stored them. The ints are what
/// the UI switches on; the keys are what the database keeps.
/// A playback failure the user can see, not just stderr: the toast in the
/// corner, dismissed by click or aged out by the timer.
fn show_play_error(ui: &AppWindow, e: &str) {
    eprintln!("no se pudo reproducir: {e}");
    ui.set_error_toast(SharedString::from(format!("No se pudo reproducir: {e}")));
    ui.set_error_toast_age(0);
}

/// The in-app shortcut catalog: id, settings key suffix, row label.
const SHORTCUT_ACTIONS: &[(&str, &str)] = &[
    ("play_pause", "Tecla · Reproducir/Pausa"),
    ("stop", "Tecla · Stop"),
    ("prev_track", "Tecla · Anterior"),
    ("next_track", "Tecla · Siguiente"),
    ("volume_up", "Tecla · Subir volumen"),
    ("volume_down", "Tecla · Bajar volumen"),
    ("mute", "Tecla · Silenciar"),
    ("toggle_shuffle", "Tecla · Aleatorio"),
    ("cycle_repeat", "Tecla · Repetir"),
    ("focus_search", "Tecla · Buscar"),
    ("toggle_fav", "Tecla · Favorito"),
];

/// A key event as the display/storage combo ("Ctrl+Alt+P", "Espacio", "F5").
/// None for keys that make no binding (bare modifiers, control chars).
fn shortcut_combo(text: &str, ctrl: bool, alt: bool, shift: bool) -> Option<String> {
    let c = text.chars().next()?;
    // Slint delivers special keys as macOS-style private-use codepoints.
    let name = match c {
        ' ' => "Espacio".to_string(),
        '\u{f700}' => "Arriba".to_string(),
        '\u{f701}' => "Abajo".to_string(),
        '\u{f702}' => "Izquierda".to_string(),
        '\u{f703}' => "Derecha".to_string(),
        '\u{f729}' => "Inicio".to_string(),
        '\u{f72b}' => "Fin".to_string(),
        '\u{f72c}' => "RePág".to_string(),
        '\u{f72d}' => "AvPág".to_string(),
        c @ '\u{f704}'..='\u{f726}' => format!("F{}", c as u32 - 0xf703),
        c if c.is_control() => return None,
        c => c.to_uppercase().to_string(),
    };
    let mut combo = String::new();
    if ctrl {
        combo.push_str("Ctrl+");
    }
    if alt {
        combo.push_str("Alt+");
    }
    if shift && name.chars().count() > 1 {
        // Shift only matters for named keys; a letter already arrived
        // uppercase and "Shift+A" would just be a second spelling of it.
        combo.push_str("Shift+");
    }
    combo.push_str(&name);
    Some(combo)
}

/// The console dropdown, filtered and ranked: exact name, prefix, a codec
/// the machine owns (`spc` → SNES), then substring — the old type-ahead's
/// order, over a list instead of a field.
fn consoles_for_filter(q: &str) -> Vec<ConsoleOption> {
    let auto = ConsoleOption {
        id: SharedString::from(""),
        name: SharedString::from("(automática)"),
    };
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        let mut out = vec![auto];
        out.extend(tunante_core::console::CONSOLES.iter().map(|c| ConsoleOption {
            id: SharedString::from(c.id),
            name: SharedString::from(c.name_es),
        }));
        return out;
    }
    let mut ranked: Vec<(u8, &tunante_core::console::Console)> = tunante_core::console::CONSOLES
        .iter()
        .filter_map(|c| {
            let name = c.name_es.to_lowercase();
            let name_en = c.name.to_lowercase();
            let codec = c
                .codecs
                .iter()
                .chain(c.weak_codecs.iter())
                .any(|e| *e == q);
            let rank = if name == q || name_en == q {
                0
            } else if name.starts_with(&q) || name_en.starts_with(&q) {
                1
            } else if codec {
                2
            } else if name.contains(&q) || name_en.contains(&q) {
                3
            } else {
                return None;
            };
            Some((rank, c))
        })
        .collect();
    ranked.sort_by_key(|(r, c)| (*r, tunante_core::console::display_order(c.id)));
    let mut out = vec![auto];
    out.extend(ranked.into_iter().map(|(_, c)| ConsoleOption {
        id: SharedString::from(c.id),
        name: SharedString::from(c.name_es),
    }));
    out
}

fn vgm_loops_label(v: Option<f64>) -> String {
    match v {
        None => "predeterminado".to_string(),
        Some(v) if v <= 1.0 => "1 pasada".to_string(),
        Some(v) => format!("{} pasadas", v as i64),
    }
}

fn cover_fit_from_key(key: &str) -> i32 {
    match key {
        "contain" => 1,
        "blur" => 2,
        "fill" => 3,
        "none" => 4,
        _ => 0,
    }
}

fn cover_fit_key(fit: i32) -> &'static str {
    match fit {
        1 => "contain",
        2 => "blur",
        3 => "fill",
        4 => "none",
        _ => "cover",
    }
}

fn cover_fit_label(fit: i32) -> &'static str {
    match fit {
        1 => "entera",
        2 => "con fondo",
        3 => "estirada",
        4 => "original",
        _ => "recortada",
    }
}

/// Ask the desktop for files, with whichever picker it ships. Returns empty
/// on cancel and on desktops with neither tool — the row says so.
fn pick_files() -> Vec<String> {
    let parse = |out: std::process::Output| -> Option<Vec<String>> {
        if !out.status.success() {
            return None;
        }
        Some(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect(),
        )
    };
    // kdialog first when the session is KDE; zenity otherwise. Both print
    // one path per line with these flags.
    let kde = std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_uppercase().contains("KDE"))
        .unwrap_or(false);
    let kdialog = || {
        std::process::Command::new("kdialog")
            .args(["--getopenfilename", ".", "", "--multiple", "--separate-output"])
            .output()
            .ok()
            .and_then(parse)
    };
    let zenity = || {
        std::process::Command::new("zenity")
            .args(["--file-selection", "--multiple", "--separator=\n"])
            .output()
            .ok()
            .and_then(parse)
    };
    let picked = if kde {
        kdialog().or_else(zenity)
    } else {
        zenity().or_else(kdialog)
    };
    picked.unwrap_or_default()
}

fn play_from_path(
    ui: &AppWindow,
    db: &Database,
    player: &Rc<RefCell<Option<player::Player>>>,
    queue_model: &VecModel<QueueRow>,
    path: &str,
) {
    let folder = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut tracks = db.get_tracks_by_folder(&folder).unwrap_or_default();
    // Not in the library yet — ask the decoder about it directly, so the
    // app can play a file it has never scanned.
    if tracks.is_empty() {
        if let Ok(values) = tunante_helper::probe(
            std::path::Path::new(path),
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
            Ok(()) => push_now_playing(ui, p),
            Err(e) => show_play_error(ui, &e),
        }
    }
    queue_model.set_vec(to_queue_rows(&tracks, Some(start)));
    ui.set_setup_mode(false);
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

/// Move the "now playing" marker to wherever the queue says we are.
///
/// Asks the queue for the index rather than hunting for a matching title: two
/// tracks in an album can share a title, and a set of subsongs from one file
/// routinely do.
/// Rebuild the queue pane: the user-queued "play next" rows first, marked
/// with », then the context. Rebuilt whole rather than marked in place — the
/// user section shrinks every time a row of it plays, so a marker alone
/// cannot keep the pane honest.
fn refresh_queue(p: &player::Player, model: &VecModel<QueueRow>) {
    let mut rows: Vec<QueueRow> = p
        .user_queue()
        .iter()
        .map(|t| QueueRow {
            title: SharedString::from(format!(
                "» {}",
                if t.title.is_empty() { t.path.as_str() } else { t.title.as_str() }
            )),
            subtitle: SharedString::from("a continuación"),
            playing: false,
        })
        .collect();
    // While a user-queued track sounds, the context marker must not claim it.
    let marker = if p.playing_from_user_queue() {
        None
    } else {
        p.current_index()
    };
    rows.extend(to_queue_rows(p.queue().tracks(), marker));
    model.set_vec(rows);
}

fn sync_queue_marker(p: &player::Player, model: &Rc<VecModel<QueueRow>>) {
    refresh_queue(p, model);
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

    ui.set_window_title(SharedString::from(match p.current().filter(|_| ui.get_titlebar_track()) {
        Some(t) => {
            let title = if t.title.is_empty() { t.path.as_str() } else { t.title.as_str() };
            if t.artist.is_empty() {
                format!("{title} — Tunante")
            } else {
                format!("{title} - {} — Tunante", t.artist)
            }
        }
        None => String::new(),
    }));
    ui.set_now_rating(p.current().map(|t| t.rating).unwrap_or(0));
    ui.set_now_stars(SharedString::from(
        p.current().map(|t| stars_for(t.rating)).unwrap_or_default(),
    ));

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

    /// Pinning /a/b must narrow to that subtree and nothing else: not /a/bc,
    /// and the #n suffix of a multi-track rip must not confuse the boundary.
    #[test]
    fn a_pinned_folder_narrows_to_its_subtree_only() {
        let mk = |path: &str| Track {
            id: path.to_string(),
            path: path.to_string(),
            title: path.to_string(),
            artist: String::new(),
            album: String::new(),
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
            game: String::new(),
            header_game: String::new(),
            console_id: String::new(),
        };
        let mut st = TableState {
            all: vec![
                mk("/a/b/inside.nsf"),
                mk("/a/b/deeper/also.nsf"),
                mk("/a/b/set.nsf#3"),
                mk("/a/bc/outside.nsf"),
                mk("/a/elsewhere.nsf"),
            ],
            scope: Scope::Folder("/a/b".to_string()),
            built: true,
            ..TableState::default()
        };
        let model = VecModel::from(Vec::<TableRow>::new());
        rebuild_table(&mut st, &model);
        let kept: Vec<_> = st.tracks.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(
            kept,
            ["/a/b/deeper/also.nsf", "/a/b/inside.nsf", "/a/b/set.nsf#3"],
            "boundary or vgm-suffix handling broke"
        );
    }

    /// A Track for scope tests, fields spelled out for the same reason
    /// bare_track does: adding a field should stop this compiling.
    fn scoped(path: &str, console: &str, rating: i32) -> Track {
        Track {
            id: path.to_string(),
            path: path.to_string(),
            title: path.to_string(),
            artist: String::new(),
            album: String::new(),
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
            rating,
            modified_at: 0,
            game: String::new(),
            header_game: String::new(),
            console_id: console.to_string(),
        }
    }

    /// A console scope keeps only that machine's tracks — the grande shell's
    /// sidebar console lands in the powerful table, not the phone grid.
    #[test]
    fn a_console_scope_keeps_only_that_machine() {
        let mut st = TableState {
            all: vec![
                scoped("/a.nsf", "nes", 0),
                scoped("/b.spc", "snes", 0),
                scoped("/c.nsf", "nes", 0),
            ],
            scope: Scope::Console("nes".to_string()),
            built: true,
            ..TableState::default()
        };
        let model = VecModel::from(Vec::<TableRow>::new());
        rebuild_table(&mut st, &model);
        let mut kept: Vec<_> = st.tracks.iter().map(|t| t.path.as_str()).collect();
        kept.sort();
        assert_eq!(kept, ["/a.nsf", "/c.nsf"]);
    }

    /// A playlist scope keeps its STORED ORDER under the sentinel sort, and
    /// resolves ids against the whole library — a track missing from the
    /// library just drops out rather than blanking the row.
    #[test]
    fn a_playlist_scope_keeps_its_order() {
        let mut st = TableState {
            all: vec![
                scoped("/x.nsf", "nes", 0),
                scoped("/y.nsf", "nes", 0),
                scoped("/z.nsf", "nes", 0),
            ],
            scope: Scope::Playlist {
                // deliberately not library order, and one id that is gone
                ids: vec![
                    "/z.nsf".to_string(),
                    "/gone.nsf".to_string(),
                    "/x.nsf".to_string(),
                ],
                id: "pl1".to_string(),
            },
            sort_key: "__scope__".to_string(),
            built: true,
            ..TableState::default()
        };
        let model = VecModel::from(Vec::<TableRow>::new());
        rebuild_table(&mut st, &model);
        let kept: Vec<_> = st.tracks.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(kept, ["/z.nsf", "/x.nsf"], "playlist order or id resolution broke");
    }
}
