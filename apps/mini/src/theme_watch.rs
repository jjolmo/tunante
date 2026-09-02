//! "Follow the system" for the theme, via the settings portal.
//!
//! `org.freedesktop.appearance color-scheme` is the one cross-desktop truth
//! about dark mode — GNOME, KDE and the phone shells all publish it. Read
//! through `busctl` rather than a D-Bus crate: this is one tiny call every
//! few seconds, not worth a dependency that brings an async runtime.

use std::sync::atomic::{AtomicU8, Ordering};

/// 0 = unknown / no preference, 1 = dark, 2 = light.
static SYSTEM_SCHEME: AtomicU8 = AtomicU8::new(0);

fn read_portal() -> u8 {
    let Ok(out) = std::process::Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "ReadOne",
            "ss",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .output()
    else {
        return 0;
    };
    if !out.status.success() {
        return 0;
    }
    // The reply prints as `v u 1`.
    match String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .last()
        .and_then(|n| n.parse::<u8>().ok())
    {
        Some(1) => 1,
        Some(2) => 2,
        _ => 0,
    }
}

/// One synchronous read, then a thread that keeps the answer fresh. Cheap on
/// purpose: the UI timer only compares an atomic, and the subprocess runs
/// once every five seconds regardless of how many timer ticks pass.
pub fn start() {
    SYSTEM_SCHEME.store(read_portal(), Ordering::Relaxed);
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        SYSTEM_SCHEME.store(read_portal(), Ordering::Relaxed);
    });
}

/// What the system wants right now. `None` when it has no opinion (portal
/// missing, or explicit no-preference) — the caller keeps its default.
pub fn prefers_dark() -> Option<bool> {
    match SYSTEM_SCHEME.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}
