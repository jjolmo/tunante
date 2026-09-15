//! The volume OSD: the little panel the tray's wheel raises.
//!
//! A window of ours, deliberately, and not a desktop notification. A
//! notification belongs to the notification server — themed by the panel,
//! filed in a history, silenced by Do Not Disturb, drawn differently on every
//! desktop, and absent altogether on Windows and macOS unless the app asks to
//! be allowed one. This is a control: it must look like Tunante and behave the
//! same way on all three. Same parts and the same 1.5 s as the popup the old
//! desktop had, only drawn in Slint instead of a WebView.
//!
//! It lives on the UI thread, which is where the wheel's notches are already
//! folded into the volume (the 500 ms timer in `main.rs`), so there is no
//! channel here and no second thread: show, restart the timer, hide.

#[cfg(feature = "tray")]
mod imp {
    use crate::{Theme, VolumeOsd};
    use slint::ComponentHandle;
    use std::cell::RefCell;
    /// How long the panel stays up after the last notch. The old build's
    /// popup hid itself after the same 1.5 s.
    const HOLD: std::time::Duration = std::time::Duration::from_millis(1500);

    /// Distance from the screen's edge, in logical pixels. The old popup aimed
    /// at the tray icon itself and fell back to this corner; the corner is all
    /// that is left to aim at, because no tray API in the tree reports where
    /// the icon is: SNI has no geometry at all.
    ///
    /// Bottom right, clear of the taskbar — except on macOS, where the status
    /// item lives in the menu bar at the top, so the panel goes under it, as
    /// the old build's did.
    const MARGIN_X: f32 = 16.0;
    #[cfg(not(target_os = "macos"))]
    const MARGIN_Y: f32 = 60.0;
    #[cfg(target_os = "macos")]
    const MARGIN_TOP: f32 = 30.0;

    struct Osd {
        window: VolumeOsd,
        /// Kept alive here: a dropped `Timer` never fires, so the panel would
        /// stay on screen for good.
        hide: slint::Timer,
        /// Where it landed the first time, in physical pixels. Asked for
        /// again before every later show: Slint keeps a position set while
        /// there is no winit window in the attributes the next one is built
        /// from, so the panel is *created* in place instead of appearing in
        /// the middle and jumping. Stays `None` on Wayland, which is the one
        /// place that never gets a position at all.
        at: std::cell::Cell<Option<(i32, i32)>>,
    }

    thread_local! {
        static OSD: RefCell<Option<Osd>> = const { RefCell::new(None) };
    }

    /// Show the volume, 0..=100, and start the countdown that hides it.
    ///
    /// `dark` is the palette the app is wearing: a second window gets its own
    /// copy of Slint's globals, so the theme has to be handed over rather than
    /// inherited.
    pub fn show_volume(percent: u32, dark: bool) {
        OSD.with(|slot| {
            let mut slot = slot.borrow_mut();
            // Built on the first notch, not at boot: a session that never
            // scrolls the icon never pays for the window.
            if slot.is_none() {
                match VolumeOsd::new() {
                    Ok(window) => {
                        *slot = Some(Osd {
                            window,
                            hide: slint::Timer::default(),
                            at: std::cell::Cell::new(None),
                        })
                    }
                    Err(e) => {
                        log::warn!("osd: sin panel de volumen ({e})");
                        return;
                    }
                }
            }
            let Some(osd) = slot.as_ref() else { return };

            osd.window.global::<Theme>().set_dark(dark);
            // `t` is what every colour is actually mixed by; the main window
            // animates it as the theme changes, and this one has no such
            // animation to run — it is on screen for a second and a half.
            osd.window
                .global::<Theme>()
                .set_t(if dark { 1.0 } else { 0.0 });
            osd.window.set_percent(percent.min(100) as i32);
            // Only on the way in. Calling `show()` on a window that is
            // already up leaves it never drawn on Wayland: the surface is
            // recreated before the first frame is committed, so the
            // compositor has nothing to map and the panel silently never
            // appears — which is exactly what a spun wheel does, a show per
            // notch.
            if !osd.window.window().is_visible() {
                if let Some((x, y)) = osd.at.get() {
                    osd.window
                        .window()
                        .set_position(slint::PhysicalPosition::new(x, y));
                }
                if let Err(e) = osd.window.show() {
                    log::warn!("osd: no pude mostrar el panel de volumen ({e})");
                    return;
                }
                place(osd);
            }

            let weak = osd.window.as_weak();
            osd.hide.start(slint::TimerMode::SingleShot, HOLD, move || {
                if let Some(w) = weak.upgrade() {
                    let _ = w.hide();
                }
            });
        });
    }

    /// Put the panel in the screen's bottom-right corner, where a window is
    /// allowed to place itself.
    ///
    /// Wayland is not such a place: a client there cannot position its own
    /// toplevel, and asking anyway is worse than a no-op — the request before
    /// the first frame leaves the surface unmapped, so the panel never appears
    /// at all — an hour of "the window says it is visible and nothing is on
    /// screen" before that came out. The compositor's own placement (centred,
    /// which is where KDE puts its own volume OSD) is the answer there, so on
    /// Wayland this returns and lets it decide.
    ///
    /// The old popup aimed at the tray icon itself and fell back to this
    /// corner. The corner is all that is left to aim at: no tray API in the
    /// tree reports where the icon is — SNI has no geometry at all.
    fn place(osd: &Osd) {
        use slint::winit_030::winit::dpi::PhysicalPosition;
        use slint::winit_030::WinitWindowAccessor;

        osd.window.window().with_winit_window(|win| {
            #[cfg(target_os = "linux")]
            {
                use slint::winit_030::winit::platform::wayland::WindowExtWayland;
                if win.xdg_toplevel().is_some() {
                    return;
                }
            }
            let Some(monitor) = win.current_monitor() else {
                return;
            };
            let scale = win.scale_factor() as f32;
            let screen = monitor.size();
            let origin = monitor.position();
            let size = win.outer_size();
            let x = origin.x + screen.width as i32 - size.width as i32 - (MARGIN_X * scale) as i32;
            #[cfg(not(target_os = "macos"))]
            let y =
                origin.y + screen.height as i32 - size.height as i32 - (MARGIN_Y * scale) as i32;
            #[cfg(target_os = "macos")]
            let y = origin.y + (MARGIN_TOP * scale) as i32;
            win.set_outer_position(PhysicalPosition::new(x, y));
            // Remembered so the next show can ask for it before the window
            // exists, which is the only way to get there without a hop.
            osd.at.set(Some((x, y)));
        });
    }
}

#[cfg(feature = "tray")]
pub use imp::show_volume;

/// No tray, no wheel to turn over it, no panel. Same shape as the tray's own
/// stubs so the event loop never has to know which build it is in.
#[cfg(not(feature = "tray"))]
pub fn show_volume(_percent: u32, _dark: bool) {}
