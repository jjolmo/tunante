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
        match tray {
            Ok(_tray) => gtk::main(),
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
