//! Bringing the window to the front on Wayland, with a token the tray host got.
//!
//! A Wayland client cannot raise its own window. KWin's focus-stealing
//! prevention keeps a window that simply appears behind whatever was on top,
//! and Slint recreates the window on every show there (Wayland has no
//! "hidden"), so every tray click that brought the window back opened it
//! *under* the browser. From the tray that looked like a click doing nothing.
//!
//! The protocol's answer is `xdg_activation_v1`: whoever received the input
//! asks the compositor for a token, and the window that should come up
//! presents it. Plasma's tray does the asking and hands the token over
//! (`ProvideXdgActivationToken`, see `vendor/ksni-patch`); this presents it.
//!
//! winit has no call for "activate this window with a token from elsewhere"
//! (only at creation, and Slint builds those attributes itself), so this talks
//! to the compositor directly: a second event queue on winit's own connection,
//! bound to the activation global, pointed at the window's surface.

#[cfg(all(target_os = "linux", feature = "tray"))]
mod imp {
    use slint::winit_030::winit::raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    use slint::winit_030::WinitWindowAccessor;
    use smithay_client_toolkit::reexports::client::{
        backend::{Backend, ObjectId},
        globals::{registry_queue_init, GlobalListContents},
        protocol::{wl_registry::WlRegistry, wl_surface::WlSurface},
        Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    };
    use smithay_client_toolkit::reexports::protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
    use std::cell::RefCell;

    /// Nothing arrives on this queue that needs an answer: the registry's
    /// events are collected by `registry_queue_init`, and the activation
    /// global has none.
    struct State;

    impl Dispatch<WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: <WlRegistry as Proxy>::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<XdgActivationV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &XdgActivationV1,
            _: <XdgActivationV1 as Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    struct Bound {
        conn: Connection,
        activation: XdgActivationV1,
        /// Held so the proxies bound on it stay valid.
        _queue: EventQueue<State>,
    }

    thread_local! {
        /// Bound once: the display outlives every window Slint recreates.
        static BOUND: RefCell<Option<Bound>> = const { RefCell::new(None) };
    }

    fn bind(display: *mut std::ffi::c_void) -> Option<Bound> {
        // Safety: the pointer is winit's live `wl_display`, which stays open
        // for as long as the event loop runs — longer than this thread-local.
        let conn = Connection::from_backend(unsafe { Backend::from_foreign_display(display.cast()) });
        let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
        let activation = globals.bind::<XdgActivationV1, State, ()>(&queue.handle(), 1..=1, ()).ok()?;
        Some(Bound { conn, activation, _queue: queue })
    }

    /// Present `token` for the window's surface. `false` when there is nothing
    /// to present it for: no winit window yet, not Wayland, or a compositor
    /// without the protocol.
    pub fn activate(window: &slint::Window, token: &str) -> bool {
        let handles = window
            .with_winit_window(|w| {
                match (w.display_handle().ok()?.as_raw(), w.window_handle().ok()?.as_raw()) {
                    (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(s)) => {
                        Some((d.display.as_ptr(), s.surface.as_ptr()))
                    }
                    _ => None,
                }
            })
            .flatten();
        let Some((display, surface)) = handles else { return false };

        BOUND.with(|b| {
            let mut b = b.borrow_mut();
            if b.is_none() {
                *b = bind(display);
            }
            let Some(bound) = b.as_ref() else { return false };
            // Safety: the surface belongs to the window winit is holding right
            // now, on the same display the connection wraps.
            let Ok(id) = (unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.cast()) }) else {
                return false;
            };
            let Ok(surface) = WlSurface::from_id(&bound.conn, id) else { return false };
            bound.activation.activate(token.to_string(), &surface);
            bound.conn.flush().is_ok()
        })
    }
}

#[cfg(all(target_os = "linux", feature = "tray"))]
pub use imp::activate;

/// Everywhere else a window can raise itself, and there is no token to present.
#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn activate(_window: &slint::Window, _token: &str) -> bool {
    false
}
