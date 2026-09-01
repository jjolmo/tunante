//! Hear the whole new chain once: AudioEngine → DspSource → PipeSource →
//! tunante-decoder → the default output device.
//!
//!     cargo build --release -p tunante-decoder
//!     TUNANTE_DECODER=target/release/tunante-decoder \
//!         cargo run -p tunante-audio --example play -- <file> [seconds]
//!
//! Prints position/duration twice a second and exits non-zero if the position
//! never advanced — which is what a dead pipe looks like from the outside.

use std::time::Duration;

struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        eprintln!("[{}] {}", record.level(), record.args());
    }
    fn flush(&self) {}
}

fn main() {
    let _ = log::set_logger(&StderrLog);
    log::set_max_level(log::LevelFilter::Info);

    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: play <file> [seconds]");
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);

    let mut engine = tunante_audio::AudioEngine::new().expect("audio output");
    engine
        .play_file(std::path::Path::new(&path), 0)
        .expect("play");

    for _ in 0..seconds * 2 {
        std::thread::sleep(Duration::from_millis(500));
        println!(
            "pos {} ms / dur {} ms, playing={}",
            engine.position_ms(),
            engine.duration_ms(),
            engine.is_playing()
        );
    }

    let advanced = engine.position_ms() > 1000;
    println!("{}", if advanced { "OK" } else { "FAILED: position never advanced" });
    std::process::exit(if advanced { 0 } else { 1 });
}
