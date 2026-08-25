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
//! tunante-mini --scan <carpeta>    scan a folder into the library first
//! tunante-mini --rows N            fake rows instead, to measure the list
//! tunante-mini --focus-search      start on Library with the search focused
//! ```
//!
//! `--rows` is the measuring harness, not the app: it fills the library tab with
//! generated entries to see what the list costs at a size no real collection
//! reaches. Read **PSS** from `/proc/<pid>/smaps_rollup`, not RSS — RSS counts
//! shared library pages the session already has resident.

mod decoder;
mod library;
mod player;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use slint::{Model, ModelRc, SharedString, VecModel};

use tunante_core::db::Database;

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let arg_value = |name: &str| -> Option<String> {
        args.iter().skip_while(|a| *a != name).nth(1).cloned()
    };

    let fake_rows = arg_value("--rows").and_then(|s| s.parse::<usize>().ok());
    let scan_target = arg_value("--scan").map(PathBuf::from);
    let focus_search = args.iter().any(|a| a == "--focus-search");

    let db = Database::new(&db_path()?)?;

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

    // The library tab, either from the real database or from generated rows.
    let roots: Vec<PathBuf> = db
        .get_monitored_folders()?
        .into_iter()
        .map(|f| PathBuf::from(f.path))
        .collect();

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

    // The player owns the audio output for the whole session: one ALSA client,
    // never handed back, because re-taking the device on a track change is what
    // makes the gap between tracks audible.
    let player = Rc::new(RefCell::new(player::Player::new().ok()));
    if player.borrow().is_none() {
        eprintln!("aviso: no hay salida de audio; la interfaz funciona, el sonido no");
    }

    let queue_model = Rc::new(VecModel::from(Vec::<QueueRow>::new()));
    ui.set_queue_rows(ModelRc::from(queue_model.clone()));

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
            queue_model.set_vec(to_queue_rows(&tracks, start));
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

    // --- Progress, and moving to the next track when one ends ---------------
    let timer = slint::Timer::default();
    {
        let player = player.clone();
        let queue_model = queue_model.clone();
        let weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(500),
            move || {
                let Some(ui) = weak.upgrade() else { return };
                let mut guard = player.borrow_mut();
                let Some(p) = guard.as_mut() else { return };

                if p.poll_track_end() {
                    push_now_playing(&ui, p);
                    sync_queue_marker(p, &queue_model);
                }
                ui.set_position_ms(p.position_ms() as i32);
                ui.set_duration_ms(p.duration_ms() as i32);
                ui.set_playing(p.is_playing());
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

fn to_queue_rows(tracks: &[tunante_core::db::models::Track], playing: usize) -> Vec<QueueRow> {
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
            playing: i == playing,
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

fn sync_queue_marker(p: &player::Player, model: &Rc<VecModel<QueueRow>>) {
    if let Some(current) = p.current() {
        for i in 0..model.row_count() {
            if let Some(row) = model.row_data(i) {
                if row.title == current.title.as_str() {
                    mark_playing(model, i);
                    return;
                }
            }
        }
    }
}

fn push_now_playing(ui: &AppWindow, p: &player::Player) {
    match p.current() {
        Some(t) => {
            ui.set_now_title(SharedString::from(if t.title.is_empty() {
                t.path.as_str()
            } else {
                t.title.as_str()
            }));
            ui.set_now_artist(SharedString::from(t.artist.as_str()));
            ui.set_now_album(SharedString::from(t.album.as_str()));
        }
        None => {
            ui.set_now_title("Nada sonando".into());
            ui.set_now_artist(SharedString::new());
            ui.set_now_album(SharedString::new());
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
