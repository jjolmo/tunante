//! Mouse side buttons as transport controls, read straight from evdev.
//!
//! The desktop compositor never hands an unfocused window the thumb
//! buttons, so "next track from the mouse while another app has focus"
//! can only come from below: /dev/input. That needs membership in the
//! `input` group — spike 4 confirmed this box has it — and plenty of
//! machines will not; [`spawn`] reports how many devices it could open
//! and the settings row tells the truth with it. Off by default, both
//! because of the permission and because reading every input device is
//! the kind of thing an app should only do when asked.
//!
//! Custom global *keyboard* shortcuts stay parked: the portal's
//! GlobalShortcuts interface is the right home for them, but it answers
//! through D-Bus signals, which means a real bus connection rather than
//! this repository's busctl-subprocess idiom. Media keys already arrive
//! via MPRIS.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// Which physical thumb button fired — the caller maps it to an action.
#[derive(Clone, Copy, Debug)]
pub enum ButtonCmd {
    Back,
    Forward,
    Side,
    Extra,
}

// input-event-codes.h: the four thumb-button identities mice actually ship.
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;
const BTN_FORWARD: u16 = 0x115;
const BTN_BACK: u16 = 0x116;

/// One reader thread per device it can open; returns how many that was.
/// Zero means no access (or no devices), and the caller says so.
///
/// Devices are enumerated once — a mouse plugged in later is picked up by
/// toggling the setting off and on. Stopping is lazy on purpose: a thread
/// parked in a blocking read only notices `stop` on its next event, which
/// costs at most one swallowed button press after the toggle.
pub fn spawn(tx: Sender<ButtonCmd>, stop: Arc<AtomicBool>) -> usize {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return 0;
    };
    let mut n = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_event = path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.starts_with("event"));
        if !is_event {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        n += 1;
        let (tx, stop) = (tx.clone(), stop.clone());
        std::thread::spawn(move || read_loop(file, tx, stop));
    }
    n
}

fn read_loop(mut file: std::fs::File, tx: Sender<ButtonCmd>, stop: Arc<AtomicBool>) {
    // struct input_event on LP64: 16 bytes of timeval, then type u16,
    // code u16, value i32. The timestamp is dead weight here.
    const EV_SIZE: usize = 24;
    const EV_KEY: u16 = 1;
    let mut buf = [0u8; EV_SIZE];
    loop {
        if file.read_exact(&mut buf).is_err() {
            return;
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let typ = u16::from_ne_bytes([buf[16], buf[17]]);
        let code = u16::from_ne_bytes([buf[18], buf[19]]);
        let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
        // Presses only; the release (0) and auto-repeat (2) of a thumb
        // button are not two more track changes.
        if typ != EV_KEY || value != 1 {
            continue;
        }
        let cmd = match code {
            BTN_BACK => ButtonCmd::Back,
            BTN_FORWARD => ButtonCmd::Forward,
            BTN_SIDE => ButtonCmd::Side,
            BTN_EXTRA => ButtonCmd::Extra,
            _ => continue,
        };
        if tx.send(cmd).is_err() {
            return;
        }
    }
}
