//! Is the sound going anywhere, or only pretending to?
//!
//! When this phone loses its sound card — see the suspend trap in HANDOVER.md —
//! PulseAudio does not fail the clients that were using it. `module-always-sink`
//! hands them `auto_null`, a sink that swallows samples at real time, and
//! everything downstream carries on as if nothing had happened.
//!
//! Measured, with the card removed from PulseAudio by hand: rodio raised no
//! error, cpal's error callback was never called, the player's position kept
//! advancing second per second, and tracks went on ending and starting. The app
//! showed a moving progress bar against twenty minutes of silence and spent 12 %
//! of the battery decoding into nothing.
//!
//! So there is no signal for this inside rodio, and none in the app either: the
//! position is a wall clock, and a wall clock is right whatever the speaker is
//! doing. The only component that knows is the sound server, and this asks it.
//!
//! # Why `pactl`, and why polling
//!
//! `pactl` rather than libpulse, for the same reason the decoder is a separate
//! process: it costs one child and no library, and where it is missing the app
//! degrades to what it did before instead of failing to build or to start.
//!
//! `pactl subscribe` would be the event-driven way and it does not work here.
//! Its stdout is fully buffered into a pipe, so the events sit in libc's buffer
//! and arrive in a batch when the process ends — which is never, for a
//! subscription. `stdbuf` would fix that and is not on this phone, and a pty to
//! get line buffering costs more than it saves.
//!
//! So: ask every ten seconds, and only while the app believes it is playing.
//! Paused or idle it asks nothing at all, and ten seconds is a tolerable delay
//! on noticing something that otherwise goes unnoticed for twenty minutes.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct OutputWatch {
    silent: Arc<AtomicBool>,
    playing: Arc<AtomicBool>,
}

impl OutputWatch {
    /// True when nothing PulseAudio offers can make a sound.
    ///
    /// False whenever that cannot be established — no `pactl`, no server, a
    /// reply that does not parse. Claiming silence wrongly would stop music
    /// that was playing perfectly well, which is a worse failure than the one
    /// this is here to catch.
    pub fn is_silent(&self) -> bool {
        self.silent.load(Ordering::Relaxed)
    }

    /// Tell the watch whether there is anything to protect.
    ///
    /// Cheap to call on every tick. While this is false — and nothing is known
    /// to be wrong — the thread sleeps and spawns nothing.
    ///
    /// Note what this does *not* do: it does not clear the verdict. Pausing is
    /// the first thing that happens when the output is found to be dumb, so
    /// clearing on pause would erase the finding half a second after making it,
    /// and the warning would appear for exactly one frame.
    pub fn note_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }
}

/// Ask whether every sink is a null sink.
///
/// `None` means the question could not be answered, which is not the same as
/// "no" and must not be treated as one.
fn nowhere_to_play() -> Option<bool> {
    let out = Command::new("pactl")
        .args(["list", "short", "sinks"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut sinks = 0usize;
    let mut real = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        sinks += 1;
        // The module owning a sink is what distinguishes a speaker from a
        // placeholder. The name does not: `auto_null` is a convention, not a
        // promise, and a Bluetooth headset is a real sink under a name this
        // code has never heard of.
        if !line.contains("module-null-sink") {
            real += 1;
        }
    }
    // No sinks at all is silence too, and it is the state the phone is in for a
    // moment after the card disappears, before always-sink steps in.
    Some(sinks == 0 || real == 0)
}

/// Start watching. Never fails: a watch that cannot run reports nothing.
pub fn spawn() -> OutputWatch {
    let silent = Arc::new(AtomicBool::new(false));
    let playing = Arc::new(AtomicBool::new(false));

    let (flag, active) = (silent.clone(), playing.clone());
    std::thread::Builder::new()
        .name("output-watch".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(10));
            // Keep asking while the answer is bad, even though playback has
            // stopped by then — otherwise nothing would ever notice the
            // speaker coming back, and the warning would be permanent.
            let watching =
                active.load(Ordering::Relaxed) || flag.load(Ordering::Relaxed);
            if !watching {
                continue;
            }
            if let Some(s) = nowhere_to_play() {
                flag.store(s, Ordering::Relaxed);
            }
        })
        .ok();

    OutputWatch { silent, playing }
}
