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
//!
//! Two shapes, because the window systems differ on one point that matters:
//! whether a window may say where it goes. X11, Windows and macOS let it, so
//! there the panel is this window, placed in the corner the tray lives in.
//! Wayland does not, at all — so there the same panel goes up as a layer
//! surface, which anchors itself; that half is `osd_layer.rs`, and it answers
//! first when it can.

#[cfg(feature = "tray")]
mod imp {
    use crate::{Theme, VolumeOsd};
    use slint::ComponentHandle;
    use std::cell::RefCell;
    /// How long the panel stays up after the last notch. The old build's
    /// popup hid itself after the same 1.5 s.
    const HOLD: std::time::Duration = std::time::Duration::from_millis(1500);

    /// Distance from the screen's edge — or from the tray icon, when the tray
    /// knows where it is. The old popup aimed at the icon and fell back to the
    /// corner; so does this.
    ///
    /// The corner is the bottom right, clear of the taskbar — except on macOS,
    /// where the status item lives in the menu bar at the top, so the panel
    /// goes under it, as the old build's did.
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
    pub fn show_volume(percent: u32, dark: bool, icon: Option<(i32, i32)>) {
        // On Wayland the panel is not a window at all: a client there cannot
        // place one, so it goes up as a layer surface that anchors itself to
        // the tray's corner. When that path answers, there is nothing else to
        // do; when it does not — X11, a compositor without the layer shell,
        // Windows, macOS — the window below can place itself and does.
        #[cfg(target_os = "linux")]
        if crate::osd_layer::try_show(percent.min(100), dark, icon) {
            return;
        }

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
                place(osd, icon);
            }

            let weak = osd.window.as_weak();
            osd.hide.start(slint::TimerMode::SingleShot, HOLD, move || {
                if let Some(w) = weak.upgrade() {
                    let _ = w.hide();
                }
            });
        });
    }

    /// Put the panel beside the tray icon, or in the corner when nobody knows
    /// where that is.
    ///
    /// Wayland never gets here: a client there cannot position its own
    /// toplevel, and asking anyway is worse than a no-op — the request before
    /// the first frame leaves the surface unmapped and the panel never appears
    /// at all. That side is `osd_layer.rs`, which anchors instead of asking.
    fn place(osd: &Osd, icon: Option<(i32, i32)>) {
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
            let (x, y) = match icon {
                // Centred on the icon, and above it or below it depending on
                // which end of the screen the tray is at — a taskbar can be at
                // the top as easily as the bottom.
                Some((ix, iy)) => {
                    let x = (ix - size.width as i32 / 2).clamp(
                        origin.x + (MARGIN_X * scale) as i32,
                        origin.x + screen.width as i32 - size.width as i32
                            - (MARGIN_X * scale) as i32,
                    );
                    let gap = (8.0 * scale) as i32;
                    let y = if (iy - origin.y) * 2 >= screen.height as i32 {
                        iy - size.height as i32 - gap
                    } else {
                        iy + gap
                    };
                    (x, y)
                }
                None => {
                    let x = origin.x + screen.width as i32
                        - size.width as i32
                        - (MARGIN_X * scale) as i32;
                    #[cfg(not(target_os = "macos"))]
                    let y = origin.y + screen.height as i32
                        - size.height as i32
                        - (MARGIN_Y * scale) as i32;
                    #[cfg(target_os = "macos")]
                    let y = origin.y + (MARGIN_TOP * scale) as i32;
                    (x, y)
                }
            };
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
pub fn show_volume(_percent: u32, _dark: bool, _icon: Option<(i32, i32)>) {}
