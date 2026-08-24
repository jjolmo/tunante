//! tunante-mini — Tunante for the phone.
//!
//! Native Slint, no webview. The emulator cores are not linked here at all: they
//! live in the `tunante-decoder` helper process, spawned per track, so the tens
//! of megabytes a console core allocates never land in this process.
//!
//! # This build is the spike
//!
//! It exists to answer three questions on the actual device before any real work
//! is designed around the answers:
//!
//! 1. **Does tapping the search field raise the on-screen keyboard?** postmarketOS
//!    replaced squeekboard with Stevia in June 2025; both speak the same
//!    client-facing protocol, so what matters is that Slint sends
//!    `zwp_text_input_v3::enable` on focus. A search box that cannot be typed
//!    into is a broken search box.
//! 2. **Does a hundred thousand rows still flick smoothly, with inertia?**
//!    `--rows` fills the library tab with fake entries for exactly this.
//! 3. **Does turning the phone move the switcher from the bottom to the side?**
//!
//! And to get a real number for the fourth: read **PSS**, not RSS, from
//! `/proc/<pid>/smaps_rollup`. RSS counts shared library pages that are already
//! resident for other processes and will overstate this by several times.

use std::rc::Rc;

use slint::{Model, ModelRc, VecModel, SharedString};

slint::include_modules!();

/// How many fake library rows to generate when none are requested.
const DEFAULT_ROWS: usize = 100_000;

fn main() -> Result<(), slint::PlatformError> {
    let rows = std::env::args()
        .skip_while(|a| a != "--rows")
        .nth(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_ROWS);

    let ui = AppWindow::new()?;

    // Fake data. The point is the row count, not the content: this is measuring
    // whether the list virtualizes, so the rows have to be numerous and cheap.
    //
    // Note what this is *not* doing: it is not a stand-in for the real library.
    // That one never materialises every row — the list asks SQLite for the window
    // it can see. Holding 100k rows here is the worst case on purpose.
    let library: Vec<LibraryRow> = (0..rows)
        .map(|i| {
            let is_folder = i % 12 == 0;
            LibraryRow {
                title: SharedString::from(if is_folder {
                    format!("Folder {}", i / 12)
                } else {
                    format!("Track {i:06} — a title long enough to need eliding")
                }),
                subtitle: SharedString::from(if is_folder {
                    format!("{} items", 11)
                } else {
                    format!("{}:{:02}", i % 5 + 1, i % 60)
                }),
                depth: (i % 3) as i32,
                is_folder,
                expanded: is_folder && i % 24 == 0,
            }
        })
        .collect();

    let queue: Vec<QueueRow> = (0..40)
        .map(|i| QueueRow {
            title: SharedString::from(format!("Queued track {i}")),
            subtitle: SharedString::from("Some Artist — Some Album"),
            playing: i == 0,
        })
        .collect();

    let library_model = Rc::new(VecModel::from(library));
    let queue_model = Rc::new(VecModel::from(queue));

    ui.set_library_total(library_model.row_count() as i32);
    ui.set_library_rows(ModelRc::from(library_model));
    ui.set_queue_rows(ModelRc::from(queue_model));
    ui.set_now_title("Tunante mini".into());
    ui.set_now_artist("spike build".into());
    ui.set_now_album("not wired to the decoder yet".into());

    ui.on_search_changed(|text| {
        // The real one queries the FTS5 index that already exists in
        // tunante-core's schema (`tracks_fts`) and replaces the model with the
        // matching window.
        println!("search: {text}");
    });

    ui.on_library_activated(|i| println!("library row {i} activated"));
    ui.on_queue_activated(|i| println!("queue row {i} activated"));

    eprintln!("tunante-mini spike — {rows} library rows");
    eprintln!("measure with: grep Pss /proc/{}/smaps_rollup", std::process::id());

    ui.run()
}
