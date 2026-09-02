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

#[cfg(all(target_os = "linux", feature = "tray"))]
pub fn spawn() {
    std::thread::spawn(|| {
        if gtk::init().is_err() {
            eprintln!("sin GTK: la app funciona, el icono de bandeja no");
            return;
        }

        // The 128px source, decoded here: the SNI protocol wants RGBA pixels,
        // and embedding the PNG keeps the icon working from any install path.
        let icon = {
            let bytes = include_bytes!("../dist/icons/128x128/tunante-mini.png");
            match image::load_from_memory(bytes) {
                Ok(img) => {
                    let rgba = img.into_rgba8();
                    let (w, h) = rgba.dimensions();
                    tray_icon::Icon::from_rgba(rgba.into_raw(), w, h).ok()
                }
                Err(_) => None,
            }
        };
        let Some(icon) = icon else {
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
                gtk::glib::timeout_add_seconds_local(1, move || {
                    if let Ok(text) = cell.lock() {
                        if *text != last {
                            last = text.clone();
                            let _ = tray.set_tooltip(Some(last.as_str()));
                        }
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
pub fn spawn() {}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn poll() -> Option<TrayAction> {
    None
}
