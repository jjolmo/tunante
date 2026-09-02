//! The system tray icon (Linux).
//!
//! The patched `tray-icon` crate (see the workspace `[patch]` and
//! `vendor/tray-icon-patch`) on a dedicated GTK thread: nobody else in this
//! app runs a GTK main loop — the shell is winit — so the tray brings its
//! own, exactly the arrangement spike 0.2 of docs/plan-desktop-slint.md
//! proved against a live StatusNotifierWatcher.
//!
//! Communication is deliberately thin. The crate already delivers menu clicks
//! through a global crossbeam channel, so the UI timer just drains
//! [`poll`] twice a second next to the MPRIS commands — same cadence, same
//! handler shapes. Nothing here talks back to the GTK thread yet: the
//! tooltip stays static and the menu labels are action names, not state.

/// What a tray menu click asks for, in the UI thread's terms.
#[derive(Clone, Copy, Debug)]
pub enum TrayAction {
    ToggleWindow,
    PlayPause,
    Next,
    Prev,
    Quit,
}

/// The tooltip's mailbox. The UI thread writes what is playing; a 1 s glib
/// timeout on the tray's own thread reads it and talks to the SNI — GTK
/// objects never cross threads, the string does.
#[cfg(all(target_os = "linux", feature = "tray"))]
static TOOLTIP: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<String>>> =
    std::sync::OnceLock::new();

#[cfg(all(target_os = "linux", feature = "tray"))]
fn tooltip_cell() -> std::sync::Arc<std::sync::Mutex<String>> {
    TOOLTIP
        .get_or_init(|| std::sync::Arc::new(std::sync::Mutex::new("Tunante".to_string())))
        .clone()
}

/// Which face the icon wears: 0 sistema (white glyph pixmap — panels are
/// overwhelmingly dark and a pixmap always draws), 1 simbólico (published by
/// *name* so the panel recolours it — the native look, at the risk that a
/// name that fails to resolve is no icon at all), 2 logo (the pixel-art
/// cartridge, immune to every theme question). The desktop app's taxonomy,
/// under its `tray_icon_style` key.
#[cfg(all(target_os = "linux", feature = "tray"))]
static STYLE: std::sync::Mutex<Option<u8>> = std::sync::Mutex::new(None);

/// Ask the tray to change style. Picked up by the 1 Hz timeout on the GTK
/// thread, same as the tooltip.
pub fn set_style(style: u8) {
    #[cfg(all(target_os = "linux", feature = "tray"))]
    if let Ok(mut s) = STYLE.lock() {
        *s = Some(style);
    }
    #[cfg(not(all(target_os = "linux", feature = "tray")))]
    let _ = style;
}

/// Scroll notches over the tray icon, accumulated on the GTK thread and
/// swapped out by the UI timer. Notches rather than raw deltas: the patch
/// normalises to ±1 per click, and volume wants clicks.
#[cfg(all(target_os = "linux", feature = "tray"))]
static SCROLL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// How many notches since last asked; positive is up. Zero when the tray
/// feature is off, which keeps the caller ignorant of platforms.
pub fn take_scroll() -> i32 {
    #[cfg(all(target_os = "linux", feature = "tray"))]
    {
        SCROLL.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(all(target_os = "linux", feature = "tray")))]
    {
        0
    }
}

/// Tell the tray what to say. Cheap: one mutexed string write.
#[cfg(all(target_os = "linux", feature = "tray"))]
pub fn set_tooltip(text: &str) {
    if let Ok(mut t) = tooltip_cell().lock() {
        if *t != text {
            *t = text.to_string();
        }
    }
}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn set_tooltip(_text: &str) {}

/// The pixmap a style draws: the pixel-art logo, or the glyph in the colour
/// the panel needs. All embedded, so the icon works from any install path.
///
/// `dark` is what the portal says about the desktop right now. The old
/// desktop shipped white-always on the "panels are overwhelmingly dark"
/// argument, and the one user on a light KDE got a white ghost — both in
/// the tray and wherever else that guess was reused. The portal removes
/// the guess.
#[cfg(all(target_os = "linux", feature = "tray"))]
fn pixmap_for(style: u8, dark: bool) -> Option<tray_icon::Icon> {
    let bytes: &[u8] = if style == 2 {
        include_bytes!("../dist/icons/128x128/tunante-mini.png")
    } else if dark {
        include_bytes!("../dist/icons/tray/mono-white.png")
    } else {
        include_bytes!("../dist/icons/tray/mono-black.png")
    };
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    tray_icon::Icon::from_rgba(rgba.into_raw(), w, h).ok()
}

/// Put the symbolic SVG somewhere the panel can resolve it, and name it —
/// or take the name back. Written at runtime because the same binary ships
/// as a tarball, an apk and a `cargo run`, and only a package has anywhere
/// to install icons.
#[cfg(all(target_os = "linux", feature = "tray"))]
fn apply_symbolic(on: bool) {
    if !on {
        tray_icon::set_symbolic_icon(None);
        return;
    }
    const SVG: &[u8] = include_bytes!("../dist/icons/tray/tunante-symbolic.svg");
    const NAME: &str = "tunante-symbolic";
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("tunante-tray");
    // Two copies of the same file: a flat dir is what libayatana documents,
    // but Plasma feeds the path to Qt's theme search, which wants
    // `<theme>/<size>/<context>/`. A kilobyte buys not guessing the host.
    let themed = dir.join("hicolor/scalable/apps");
    if std::fs::create_dir_all(&themed).is_err() {
        return;
    }
    for target in [dir.join(format!("{NAME}.svg")), themed.join(format!("{NAME}.svg"))] {
        if std::fs::write(&target, SVG).is_err() {
            return;
        }
    }
    tray_icon::set_symbolic_icon(Some((NAME.to_string(), dir)));
}

#[cfg(all(target_os = "linux", feature = "tray"))]
pub fn spawn(style: u8) {
    std::thread::spawn(move || {
        if gtk::init().is_err() {
            eprintln!("sin GTK: la app funciona, el icono de bandeja no");
            return;
        }

        // The name has to be published before the tray is built — the
        // patch reads it inside TrayIcon::new.
        apply_symbolic(style == 1);
        let dark = crate::theme_watch::prefers_dark().unwrap_or(true);
        let Some(icon) = pixmap_for(style, dark) else {
            eprintln!("el icono embebido no decodifica; sin bandeja");
            return;
        };

        use tray_icon::menu::{Menu, MenuItem};
        let menu = Menu::new();
        let items = [
            MenuItem::with_id("toggle", "Mostrar/Ocultar", true, None),
            MenuItem::with_id("play", "Reproducir/Pausa", true, None),
            MenuItem::with_id("next", "Siguiente", true, None),
            MenuItem::with_id("prev", "Anterior", true, None),
            MenuItem::with_id("quit", "Salir", true, None),
        ];
        for item in &items {
            let _ = menu.append(item);
        }

        // Held for the life of the thread: dropping it unregisters the SNI
        // item, and gtk::main() below never returns.
        let tray = tray_icon::TrayIconBuilder::new()
            .with_id("tunante-mini")
            .with_tooltip("Tunante")
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .build();
        // The patch delivers AppIndicator scroll-event here, on this GTK
        // thread; the atomic carries it across.
        tray_icon::set_scroll_handler(|_id, delta| {
            let notch = if delta > 0.0 { 1 } else { -1 };
            SCROLL.fetch_add(notch, std::sync::atomic::Ordering::Relaxed);
        });

        match tray {
            Ok(tray) => {
                // The tooltip follows the track. Polled at 1 Hz on this
                // thread rather than pushed: the mailbox is a string, and a
                // second of lag on a tooltip is beneath noticing.
                let cell = tooltip_cell();
                let mut last = String::new();
                let tray = tray;
                let mut current = (style, dark);
                gtk::glib::timeout_add_seconds_local(1, move || {
                    if let Ok(text) = cell.lock() {
                        if *text != last {
                            last = text.clone();
                            let _ = tray.set_tooltip(Some(last.as_str()));
                        }
                    }
                    // A style change from Ajustes, or the desktop switching
                    // light/dark under the "sistema" glyph — either way,
                    // republish name and pixmap. set_icon re-reads the
                    // symbolic global, so the order is the name first, then
                    // the forced repaint.
                    let style = STYLE
                        .lock()
                        .ok()
                        .and_then(|mut s| s.take())
                        .unwrap_or(current.0);
                    let dark = crate::theme_watch::prefers_dark().unwrap_or(true);
                    if (style, dark) != current {
                        current = (style, dark);
                        apply_symbolic(style == 1);
                        let _ = tray.set_icon(pixmap_for(style, dark));
                    }
                    gtk::glib::ControlFlow::Continue
                });
                gtk::main()
            }
            Err(e) => eprintln!("sin icono de bandeja: {e}"),
        }
    });
}

#[cfg(all(target_os = "linux", feature = "tray"))]
pub fn poll() -> Option<TrayAction> {
    let ev = tray_icon::menu::MenuEvent::receiver().try_recv().ok()?;
    match ev.id.as_ref() {
        "toggle" => Some(TrayAction::ToggleWindow),
        "play" => Some(TrayAction::PlayPause),
        "next" => Some(TrayAction::Next),
        "prev" => Some(TrayAction::Prev),
        "quit" => Some(TrayAction::Quit),
        _ => None,
    }
}

// Same shape as the mpris stubs: the event loop in main.rs never has to know
// which platform it is on.
#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn spawn(_style: u8) {}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn poll() -> Option<TrayAction> {
    None
}
