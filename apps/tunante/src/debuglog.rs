//! The in-app log: a ring buffer behind the `log` facade, plus stderr.
//!
//! Worth more here than it looks: this app never installed a logger, so
//! every `log::warn!` from the engine (output rebuilds), the watcher, the
//! decoder's forwarded stderr and tunante-art's matcher went nowhere. Now
//! they land in a 500-line ring the Registro sheet can show — on the phone,
//! where there is no terminal behind the window, that is the only pair of
//! eyes there is.

use std::collections::VecDeque;
use std::sync::Mutex;

const CAPACITY: usize = 500;

static RING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

struct RingLogger;

impl log::Log for RingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Warnings from anyone; chatter only from this repository. Setting a
        // global Info level turned out to wake zbus's tracing, which narrates
        // every D-Bus dispatch at INFO — 500 lines of that is a ring buffer
        // remembering nothing.
        metadata.level() <= log::Level::Warn
            || (metadata.level() == log::Level::Info
                && metadata.target().starts_with("tunante"))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "[{}] {}: {}",
            record.level(),
            record.target().split("::").next().unwrap_or(""),
            record.args()
        );
        eprintln!("{line}");
        if let Ok(mut ring) = RING.lock() {
            if ring.len() >= CAPACITY {
                ring.pop_front();
            }
            ring.push_back(line);
        }
    }

    fn flush(&self) {}
}

pub fn install() {
    if log::set_logger(&RingLogger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// Empty the ring — the sheet's Limpiar.
pub fn clear() {
    if let Ok(mut ring) = RING.lock() {
        ring.clear();
    }
}

/// The newest lines first — the sheet reads top-down and the fresh entry is
/// what somebody opened it for.
pub fn lines() -> Vec<String> {
    RING.lock()
        .map(|r| r.iter().rev().cloned().collect())
        .unwrap_or_default()
}
