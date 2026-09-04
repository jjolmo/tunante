//! The system tray icon (Linux), a native StatusNotifierItem via ksni.
//!
//! ksni speaks `org.kde.StatusNotifierItem` straight over zbus — the same
//! async-io reactor MPRIS already runs on — so there is no GTK main loop and no
//! libdbus here. The reason for the rewrite: libayatana-appindicator forced the
//! context menu open on *every* click, so a left-click could never restore the
//! window. A native SNI keeps `Activate` (left-click) and the context menu
//! (right-click) separate, the way the old Tauri desktop behaved.
//!
//! ksni runs the item on its own thread. Its callbacks (activate, menu clicks,
//! scroll) hand work back to the UI thread through the same thin channel/atomic
//! the rest of main.rs drains twice a second.

/// What a tray interaction asks for, in the UI thread's terms.
#[derive(Clone, Copy, Debug)]
pub enum TrayAction {
    /// Left-click and the "Mostrar/Ocultar" menu item: always show or hide the
    /// window. Not configurable — restoring the window is what a tray icon is
    /// for, and the old build only tied it to a setting because libayatana gave
    /// it a single click channel to share.
    ToggleWindow,
    /// Middle-click: the configurable action (`tray_middle_click_action`) —
    /// play/pause, next, or, by default, toggle the window.
    MiddleClick,
    PlayPause,
    Next,
    Prev,
    Quit,
}

#[cfg(all(target_os = "linux", feature = "tray"))]
mod imp {
    use super::TrayAction;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Mutex, OnceLock};

    /// Interactions flow tray-thread → UI-thread through this channel; the UI
    /// timer drains it with [`poll`], exactly where the old menu receiver sat.
    static ACTIONS: OnceLock<(Sender<TrayAction>, Mutex<Receiver<TrayAction>>)> = OnceLock::new();
    fn actions() -> &'static (Sender<TrayAction>, Mutex<Receiver<TrayAction>>) {
        ACTIONS.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            (tx, Mutex::new(rx))
        })
    }

    /// Scroll notches over the icon, accumulated on the tray thread and swapped
    /// out by the UI timer. Positive is up; the caller turns each into 5% of
    /// volume.
    static SCROLL: AtomicI32 = AtomicI32::new(0);

    /// What the icon should say and wear, written by the UI thread and applied
    /// by a 1 Hz watcher on the tray thread through the ksni handle — ksni only
    /// re-reads the tray on `update()`, so a mailbox plus a poll is the bridge.
    static TOOLTIP: Mutex<String> = Mutex::new(String::new());
    static STYLE: Mutex<Option<u8>> = Mutex::new(None);

    pub fn take_scroll() -> i32 {
        SCROLL.swap(0, Ordering::Relaxed)
    }

    pub fn set_tooltip(text: &str) {
        if let Ok(mut t) = TOOLTIP.lock() {
            if *t != text {
                *t = text.to_string();
            }
        }
    }

    pub fn set_style(style: u8) {
        if let Ok(mut s) = STYLE.lock() {
            *s = Some(style);
        }
    }

    pub fn poll() -> Option<TrayAction> {
        actions().1.lock().ok()?.try_recv().ok()
    }

    struct Tray {
        /// 0 sistema (mono glyph pixmap), 1 simbólico (icon name + theme path,
        /// so the panel recolours it), 2 logo (the pixel-art cartridge).
        style: u8,
        dark: bool,
        tooltip: String,
        /// Where the symbolic SVG was written, handed back as `IconThemePath`.
        theme_dir: String,
        tx: std::sync::mpsc::Sender<TrayAction>,
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "tunante".into()
        }

        fn title(&self) -> String {
            "Tunante".into()
        }

        fn category(&self) -> ksni::Category {
            ksni::Category::ApplicationStatus
        }

        fn icon_theme_path(&self) -> String {
            if self.style == 1 {
                self.theme_dir.clone()
            } else {
                String::new()
            }
        }

        fn icon_name(&self) -> String {
            if self.style == 1 {
                "tunante-symbolic".into()
            } else {
                String::new()
            }
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            if self.style == 1 {
                return Vec::new();
            }
            pixmap_for(self.style, self.dark).into_iter().collect()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: if self.tooltip.is_empty() {
                    "Tunante".into()
                } else {
                    self.tooltip.clone()
                },
                ..Default::default()
            }
        }

        // Left-click. The whole point of the rewrite: this used to be swallowed
        // by the menu.
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(TrayAction::ToggleWindow);
        }

        // Middle-click runs the configurable action, kept as it was.
        fn secondary_activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(TrayAction::MiddleClick);
        }

        fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
            if matches!(orientation, ksni::Orientation::Vertical) && delta != 0 {
                // ±1 per notch: volume wants clicks, not pixels. Up is louder.
                SCROLL.fetch_add(if delta > 0 { 1 } else { -1 }, Ordering::Relaxed);
            }
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            vec![
                StandardItem {
                    label: tunante_core::i18n::tr("Mostrar/Ocultar"),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.tx.send(TrayAction::ToggleWindow);
                    }),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                StandardItem {
                    label: tunante_core::i18n::tr("Reproducir/Pausa"),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.tx.send(TrayAction::PlayPause);
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: tunante_core::i18n::tr("Siguiente"),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.tx.send(TrayAction::Next);
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: tunante_core::i18n::tr("Anterior"),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.tx.send(TrayAction::Prev);
                    }),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                StandardItem {
                    label: tunante_core::i18n::tr("Salir"),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.tx.send(TrayAction::Quit);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub fn spawn(style: u8) {
        use ksni::blocking::TrayMethods;

        let dark = crate::theme_watch::prefers_dark().unwrap_or(true);
        let theme_dir = write_symbolic();
        let tray = Tray {
            style,
            dark,
            tooltip: "Tunante".to_string(),
            theme_dir,
            tx: actions().0.clone(),
        };

        // ksni's blocking spawn starts the item on its own thread and returns a
        // handle. No StatusNotifierWatcher (no SNI host) means no way back from
        // the tray, same as before: log and carry on windowed.
        let handle = match tray.spawn() {
            Ok(h) => h,
            Err(e) => {
                eprintln!("sin icono de bandeja: {e}");
                return;
            }
        };

        // Push tooltip/style/theme changes from the UI thread into the item.
        std::thread::spawn(move || {
            let mut last_tooltip = String::new();
            let mut cur_style = style;
            let mut cur_dark = dark;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let tooltip = TOOLTIP.lock().ok().map(|t| t.clone()).unwrap_or_default();
                let new_style = STYLE
                    .lock()
                    .ok()
                    .and_then(|mut s| s.take())
                    .unwrap_or(cur_style);
                let new_dark = crate::theme_watch::prefers_dark().unwrap_or(true);
                if tooltip == last_tooltip && new_style == cur_style && new_dark == cur_dark {
                    continue;
                }
                last_tooltip = tooltip.clone();
                cur_style = new_style;
                cur_dark = new_dark;
                let alive = handle.update(|t: &mut Tray| {
                    if !tooltip.is_empty() {
                        t.tooltip = tooltip.clone();
                    }
                    t.style = new_style;
                    t.dark = new_dark;
                });
                if alive.is_none() {
                    break; // the service is gone; nothing left to update.
                }
            }
        });
    }

    /// The ARGB32 pixmap a style draws: the pixel-art logo, or the glyph in the
    /// shade the panel needs. All embedded, so the icon works from any install
    /// path. `dark` is what the portal says about the desktop right now.
    fn pixmap_for(style: u8, dark: bool) -> Option<ksni::Icon> {
        let bytes: &[u8] = if style == 2 {
            include_bytes!("../dist/icons/128x128/tunante.png")
        } else if dark {
            include_bytes!("../dist/icons/tray/mono-white.png")
        } else {
            include_bytes!("../dist/icons/tray/mono-black.png")
        };
        let img = image::load_from_memory(bytes).ok()?;
        let rgba = img.into_rgba8();
        let (w, h) = rgba.dimensions();
        // The SNI pixmap is ARGB32 in network byte order; `image` hands us RGBA.
        let mut data = rgba.into_raw();
        for px in data.chunks_exact_mut(4) {
            px.rotate_right(1); // R,G,B,A -> A,R,G,B
        }
        Some(ksni::Icon {
            width: w as i32,
            height: h as i32,
            data,
        })
    }

    /// Put the symbolic SVG somewhere a panel can resolve it by name, and return
    /// the directory to hand back as `IconThemePath`. Written at run time
    /// because the same binary ships as a tarball, an apk and a `cargo run`, and
    /// only a package has anywhere to install icons.
    fn write_symbolic() -> String {
        const SVG: &[u8] = include_bytes!("../dist/icons/tray/tunante-symbolic.svg");
        const NAME: &str = "tunante-symbolic";
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("tunante-tray");
        // Two copies of the same file: a flat dir is what the spec's IconThemePath
        // wants, but Plasma feeds the path to Qt's theme search, which wants
        // `<theme>/<size>/<context>/`. A kilobyte buys not guessing the host.
        let themed = dir.join("hicolor/scalable/apps");
        if std::fs::create_dir_all(&themed).is_err() {
            return String::new();
        }
        for target in [dir.join(format!("{NAME}.svg")), themed.join(format!("{NAME}.svg"))] {
            if std::fs::write(&target, SVG).is_err() {
                return String::new();
            }
        }
        dir.to_string_lossy().into_owned()
    }
}

#[cfg(all(target_os = "linux", feature = "tray"))]
pub use imp::{poll, set_style, set_tooltip, spawn, take_scroll};

// Same shape as the mpris stubs: the event loop in main.rs never has to know
// which platform it is on. The tray is Linux-only (SNI is freedesktop), and the
// phone build turns the feature off to keep D-Bus and ksni out of that image.
#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn spawn(_style: u8) {}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn poll() -> Option<TrayAction> {
    None
}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn set_style(_style: u8) {}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn take_scroll() -> i32 {
    0
}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn set_tooltip(_text: &str) {}
