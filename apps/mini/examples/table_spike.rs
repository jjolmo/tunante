//! Spike 0.1 of docs/plan-desktop-slint.md: can a Slint ListView carry the
//! desktop TrackList? 30 000 rows by 17 columns, sort on header click,
//! selection, and a --bench mode that scrolls the viewport itself and counts
//! frames from the rendering notifier — not with SLINT_DEBUG_PERFORMANCE,
//! whose full-speed mode flatters both renderers (see Cargo.toml).
//!
//!     cargo run --release -p tunante-mini --example table_spike -- --bench
//!     SLINT_BACKEND=winit-software cargo run --release -p tunante-mini --example table_spike -- --bench

use slint::{ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

slint::slint! {
    import { ListView } from "std-widgets.slint";

    export component SpikeWindow inherits Window {
        title: "table spike — 30000 × 17";
        preferred-width: 1600px;
        preferred-height: 900px;
        background: #121212;

        in property <[string]> headers;
        in property <[float]> widths;
        in property <[[string]]> rows;
        in-out property <int> selected: -1;
        in-out property <length> scroll <=> lv.viewport-y;
        out property <length> view-height <=> lv.visible-height;
        callback sort-by(int);

        VerticalLayout {
            Rectangle {
                height: 28px;
                background: #1f1f1f;
                HorizontalLayout {
                    for h[i] in root.headers: Rectangle {
                        width: root.widths[i] * 1px;
                        TouchArea { clicked => { root.sort-by(i); } }
                        Text {
                            x: 6px;
                            width: parent.width - 12px;
                            height: 100%;
                            text: h;
                            color: #bbbbbb;
                            vertical-alignment: center;
                            overflow: elide;
                        }
                    }
                }
            }
            lv := ListView {
                for row[r] in root.rows: Rectangle {
                    height: 26px;
                    background: r == root.selected ? #2a4d69
                        : Math.mod(r, 2) == 0 ? #161616 : #1b1b1b;
                    TouchArea { clicked => { root.selected = r; } }
                    HorizontalLayout {
                        for cell[c] in row: Rectangle {
                            width: root.widths[c] * 1px;
                            Text {
                                x: 6px;
                                width: parent.width - 12px;
                                height: 100%;
                                text: cell;
                                color: #d8d8d8;
                                vertical-alignment: center;
                                overflow: elide;
                            }
                        }
                    }
                }
            }
        }
    }
}

const ROWS: usize = 30_000;
const ROW_HEIGHT: f32 = 26.0;

const HEADERS: [&str; 17] = [
    "#", "Title", "Artist", "Album", "Game", "Console", "Track", "Duration",
    "Codec", "Bitrate", "Sample rate", "Ch", "Rating", "Path", "Size", "Year",
    "Disc",
];
const WIDTHS: [f32; 17] = [
    50.0, 260.0, 160.0, 200.0, 200.0, 110.0, 55.0, 75.0, 80.0, 80.0, 95.0,
    45.0, 70.0, 330.0, 80.0, 55.0, 45.0,
];

fn make_data() -> Vec<Vec<SharedString>> {
    let consoles = ["NES", "SNES", "Mega Drive", "PlayStation", "Nintendo DS", "PC Engine"];
    let codecs = ["gme/nsf", "gme/spc", "vgmstream", "psf", "2sf", "opus"];
    (0..ROWS)
        .map(|r| {
            let game = format!("Game Series {} — Subtitle of Some Length", r % 900);
            vec![
                SharedString::from(format!("{}", r + 1)),
                SharedString::from(format!("Track Title {} (Stage {} Theme)", r, r % 40)),
                SharedString::from(format!("Composer {}", r % 300)),
                SharedString::from(game.clone()),
                SharedString::from(game),
                SharedString::from(consoles[r % consoles.len()]),
                SharedString::from(format!("{}", r % 40 + 1)),
                SharedString::from(format!("{}:{:02}", r % 6, r % 60)),
                SharedString::from(codecs[r % codecs.len()]),
                SharedString::from(format!("{} kbps", 128 + (r % 8) * 32)),
                SharedString::from("44100 Hz"),
                SharedString::from("2"),
                SharedString::from(if r % 7 == 0 { "★★★★" } else { "" }),
                SharedString::from(format!("/music/consolas/{}/game-{}/track-{:03}.nsf", r % 6, r % 900, r % 40)),
                SharedString::from(format!("{} KB", 12 + r % 900)),
                SharedString::from(format!("{}", 1985 + r % 40)),
                SharedString::from("1"),
            ]
        })
        .collect()
}

fn rows_model(data: &[Vec<SharedString>]) -> Vec<ModelRc<SharedString>> {
    data.iter()
        .map(|row| ModelRc::new(VecModel::from(row.clone())))
        .collect()
}

fn main() {
    let bench = std::env::args().any(|a| a == "--bench");

    let build_start = Instant::now();
    let data = Rc::new(RefCell::new(make_data()));
    let outer = Rc::new(VecModel::from(rows_model(&data.borrow())));
    println!("build: {} rows in {:?}", ROWS, build_start.elapsed());

    let ui = SpikeWindow::new().unwrap();
    ui.set_headers(ModelRc::new(VecModel::from(
        HEADERS.iter().map(|h| SharedString::from(*h)).collect::<Vec<_>>(),
    )));
    ui.set_widths(ModelRc::new(VecModel::from(WIDTHS.to_vec())));
    ui.set_rows(ModelRc::from(outer.clone()));

    // Sort on header click: the real desktop pattern — sort the backing data,
    // rebuild every row model, swap the whole vector. Timed, because this is
    // exactly what a 30k-track library does on every column click.
    let sort_state = Rc::new(RefCell::new((usize::MAX, false)));
    {
        let data = data.clone();
        let outer = outer.clone();
        let sort_state = sort_state.clone();
        ui.on_sort_by(move |col| {
            let col = col as usize;
            let t = Instant::now();
            let mut st = sort_state.borrow_mut();
            let asc = if st.0 == col { !st.1 } else { true };
            *st = (col, asc);
            let mut d = data.borrow_mut();
            d.sort_by(|a, b| if asc { a[col].cmp(&b[col]) } else { b[col].cmp(&a[col]) });
            outer.set_vec(rows_model(&d));
            println!("sort col {} ({}): {:?}", col, if asc { "asc" } else { "desc" }, t.elapsed());
        });
    }

    if bench {
        // Count real frames, not full-speed redraws.
        let frames = Rc::new(RefCell::new(0u32));
        let have_notifier = {
            let frames = frames.clone();
            ui.window()
                .set_rendering_notifier(move |state, _| {
                    if matches!(state, slint::RenderingState::BeforeRendering) {
                        *frames.borrow_mut() += 1;
                    }
                })
                .is_ok()
        };
        if !have_notifier {
            // The software renderer refuses the notifier; measure that one with
            // SLINT_DEBUG_PERFORMANCE=refresh_lazy,console instead (lazy mode
            // counts real frames, unlike full_speed — see Cargo.toml).
            println!("no rendering notifier: fps lines come from SLINT_DEBUG_PERFORMANCE");
        }

        // Phase 1: 10 s of continuous scroll driven at 250 Hz so the redraw
        // rate, not the driver, is the ceiling. Phase 2: five sorts. Then quit.
        let timer = Timer::default();
        let tick = Rc::new(RefCell::new(0u32));
        let window_start = Rc::new(RefCell::new(Instant::now()));
        let weak = ui.as_weak();
        let outer_bench = outer;
        let data_bench = data;
        timer.start(TimerMode::Repeated, std::time::Duration::from_millis(4), move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut t = tick.borrow_mut();
            *t += 1;
            let content = ROWS as f32 * ROW_HEIGHT;
            let view = ui.get_view_height();
            let next = ui.get_scroll() - 8.0;
            ui.set_scroll(if -next > content - view { 0.0 } else { next });

            if *t % 500 == 0 && have_notifier {
                let elapsed = window_start.borrow().elapsed();
                let f = *frames.borrow();
                println!("scroll: {} frames in {:.1?} = {:.1} fps", f, elapsed, f as f32 / elapsed.as_secs_f32());
                *frames.borrow_mut() = 0;
                *window_start.borrow_mut() = Instant::now();
            }
            if *t == 2500 {
                for col in [1usize, 2, 13, 7, 1] {
                    let t = Instant::now();
                    let mut d = data_bench.borrow_mut();
                    d.sort_by(|a, b| a[col].cmp(&b[col]));
                    outer_bench.set_vec(rows_model(&d));
                    println!("bench sort col {}: {:?}", col, t.elapsed());
                }
                slint::quit_event_loop().unwrap();
            }
        });
        ui.run().unwrap();
    } else {
        ui.run().unwrap();
    }
}
