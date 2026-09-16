//! The wheel over the tray icon, where `tray-icon` cannot hear it.
//!
//! The crate reports clicks and hovers on the icon but never a scroll: on
//! macOS the view it draws is its own, with no hook to add a `scrollWheel:`
//! to, and on Windows the notification area never sends a wheel message to
//! anyone at all. The old build's answer was a patched copy of the crate,
//! which is what the ksni migration got rid of. So each platform reads the
//! wheel one level up, from the system's own event stream, and hands
//! `tray.rs` whole notches: `+1` up (louder), `-1` down.
//!
//! Main thread only, on both: the tray was built there, and that is where
//! AppKit runs the monitor and where Windows reports a low-level hook.

/// macOS: a local `NSEvent` monitor sees every event the app is about to
/// dispatch, and a scroll whose window is the status item's is a scroll over
/// the icon. The monitor only looks; the event goes on to the view unchanged.
#[cfg(target_os = "macos")]
mod mac {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventPhase, NSStatusItem};
    use std::cell::Cell;
    use std::ptr::NonNull;

    /// A trackpad scrolls in points, not notches; this many make one, so a
    /// swipe moves the volume about as far as a wheel spun the same distance.
    const POINTS_PER_NOTCH: f64 = 10.0;

    /// Start watching. `on_notch` runs on the main thread, the moment it
    /// happens.
    ///
    /// The returned object is the monitor's handle: the monitor lives on
    /// whether or not it is kept, but it is what `NSEvent::removeMonitor`
    /// would need, and holding it is how the caller knows not to install a
    /// second one.
    pub fn watch(item: Retained<NSStatusItem>, on_notch: fn(i32)) -> Option<Retained<AnyObject>> {
        // Trackpad points left over from the last event, so a slow swipe
        // still adds up to something.
        let carry = Cell::new(0.0f64);
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let ev = unsafe { event.as_ref() };
            if let Some(mtm) = MainThreadMarker::new() {
                if over_icon(&item, ev, mtm) {
                    let notches = notches(ev, &carry);
                    if notches != 0 {
                        on_notch(notches);
                    }
                }
            }
            event.as_ptr()
        });
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::ScrollWheel, &block) }
    }

    /// Whether the event happened in the status item's window, which is the
    /// icon and nothing else: the menu bar gives each item a window of its own.
    fn over_icon(item: &NSStatusItem, ev: &NSEvent, mtm: MainThreadMarker) -> bool {
        let Some(ours) = item.button(mtm).and_then(|b| b.window()) else { return false };
        let Some(theirs) = ev.window(mtm) else { return false };
        std::ptr::eq(&*ours, &*theirs)
    }

    /// How many notches an event is worth, positive for up.
    ///
    /// A mouse wheel sends one event per click of the wheel and a delta in
    /// lines that acceleration can stretch, so it is the sign that counts, as
    /// on Linux. A trackpad sends points, many events per swipe, and keeps
    /// sending them after the fingers lift (momentum); those are ignored, or
    /// a flick would go on turning the volume for a second after the hand is
    /// gone.
    fn notches(ev: &NSEvent, carry: &Cell<f64>) -> i32 {
        let delta = ev.scrollingDeltaY();
        if !ev.hasPreciseScrollingDeltas() {
            return if delta > 0.0 {
                1
            } else if delta < 0.0 {
                -1
            } else {
                0
            };
        }
        if ev.momentumPhase() != NSEventPhase::None {
            return 0;
        }
        let total = carry.get() + delta;
        let whole = (total / POINTS_PER_NOTCH).trunc();
        carry.set(total - whole * POINTS_PER_NOTCH);
        whole as i32
    }
}

#[cfg(target_os = "macos")]
pub use mac::watch;

/// Windows: a low-level mouse hook, up only while the pointer is over the
/// icon.
///
/// Nothing short of `WH_MOUSE_LL` sees the wheel there, and such a hook has
/// every mouse message of the whole desktop routed through this thread —
/// which Windows watches, quietly dropping a hook whose thread is slow to
/// answer. Hooking on the tray's `Enter` and unhooking on its `Leave` keeps
/// that cost to the moments it is needed, and a hook that was dropped comes
/// back at the next `Enter`. The old build did the same.
#[cfg(target_os = "windows")]
mod win {
    use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, MSLLHOOKSTRUCT, WHEEL_DELTA,
        WH_MOUSE_LL, WM_MOUSEWHEEL,
    };

    struct Hooks {
        on_notch: fn(i32),
        /// Whether a screen point, in physical pixels, is on the icon: the
        /// hook sees the whole desktop's wheel, and only the icon's counts.
        over_icon: fn(i32, i32) -> bool,
    }
    static HOOKS: OnceLock<Hooks> = OnceLock::new();
    /// The live hook, null while the pointer is elsewhere.
    static HOOK: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
    /// Wheel units short of a notch, carried to the next message: a
    /// free-spinning wheel reports fractions of `WHEEL_DELTA`.
    static CARRY: AtomicI32 = AtomicI32::new(0);

    /// Register once what a notch does and where the icon is.
    pub fn watch(on_notch: fn(i32), over_icon: fn(i32, i32) -> bool) {
        let _ = HOOKS.set(Hooks { on_notch, over_icon });
    }

    /// The pointer came onto the icon (`true`) or left it. Main thread, the
    /// tray's: a low-level hook reports to the thread that set it, and that
    /// thread has to be pumping messages.
    pub fn hover(inside: bool) {
        if inside {
            if !HOOK.load(Ordering::Relaxed).is_null() {
                return;
            }
            CARRY.store(0, Ordering::Relaxed);
            let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), std::ptr::null_mut(), 0) };
            HOOK.store(hook, Ordering::Relaxed);
        } else {
            let hook = HOOK.swap(std::ptr::null_mut(), Ordering::Relaxed);
            if !hook.is_null() {
                unsafe { UnhookWindowsHookEx(hook) };
            }
        }
    }

    unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && wparam as u32 == WM_MOUSEWHEEL {
            if let Some(hooks) = HOOKS.get() {
                let m: &MSLLHOOKSTRUCT = &*(lparam as *const MSLLHOOKSTRUCT);
                if (hooks.over_icon)(m.pt.x, m.pt.y) {
                    // The wheel's delta is the high word, signed, in 120ths
                    // of a notch; up is positive, as on the other two.
                    let delta = (m.mouseData >> 16) as u16 as i16 as i32;
                    let total = CARRY.load(Ordering::Relaxed) + delta;
                    let notches = total / WHEEL_DELTA as i32;
                    CARRY.store(total - notches * WHEEL_DELTA as i32, Ordering::Relaxed);
                    if notches != 0 {
                        (hooks.on_notch)(notches);
                    }
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }
}

#[cfg(target_os = "windows")]
pub use win::{hover, watch};
