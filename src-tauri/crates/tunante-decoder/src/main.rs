//! Out-of-process decoder helper.
//!
//! Every vendored emulator core allocates the RAM of the console it emulates —
//! roughly 38 MB for the NDS core, 29 MB for the N64 one — and several of them
//! keep that state in C globals that are never handed back. Running them here,
//! in a process that is spawned per track and killed on track change, means the
//! kernel reclaims all of it and the UI process never grows.
//!
//! It buys three more things the in-process design could not have:
//!
//! - **No teardown wait.** The desktop engine sleeps 50 ms on every track change
//!   so the previous decoder's C globals are destroyed before the next one is
//!   built. A fresh process starts with clean globals; the wait disappears.
//! - **A metadata read that can actually be abandoned.** The library scanner
//!   wraps metadata reads in a timeout because emulator-backed readers hang. That
//!   timeout cannot interrupt a loop running in C — it can only abandon the
//!   thread. Killing a process works.
//! - **Containment.** A segfault in thirty-year-old emulator C takes down the
//!   helper, not the player.
//!
//! # Protocol
//!
//! ```text
//! tunante-decoder probe <path> [--fast]
//!     stdout: one line of JSON — {"ok":true,"tracks":[…]} or {"ok":false,"error":"…"}
//!     --fast skips silence detection for GME tracks with no declared length.
//!            Those are decoded in full to find where they stop, which costs
//!            seconds per file — the difference between a scan of minutes and
//!            one of half an hour.
//!
//! tunante-decoder play <path> [duration_hint_ms]
//!     stdout: one line of JSON header, then raw PCM until EOF
//!             {"sample_rate":44100,"channels":2,"duration_ms":123456}
//!             <f32 native-endian, interleaved>
//! ```
//!
//! The header is a text line so the stream can be inspected with `head -1`; the
//! PCM that follows is raw because it is the hot path.
//!
//! Exit code is 0 on success, 1 on failure, and the error goes to stderr as well
//! as into the JSON, so a caller that only wants a status does not have to parse.

use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;

use rodio::Source;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    let usage = || {
        eprintln!("usage: tunante-decoder probe <path> [--fast]");
        eprintln!("       tunante-decoder play <path> [duration_hint_ms]");
    };

    let Some(mode) = args.get(1) else {
        usage();
        return ExitCode::FAILURE;
    };

    let Some(path) = args.get(2) else {
        usage();
        return ExitCode::FAILURE;
    };

    let result = match mode.as_str() {
        "probe" => probe(path, args.iter().any(|a| a == "--fast")),
        "play" => {
            let hint = args.get(3).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
            play(path, hint)
        }
        other => Err(format!("unknown mode '{other}'")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tunante-decoder: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Read every track a file contains, and print them as JSON.
///
/// A single path can yield many tracks: a GME set or a vgmstream container holds
/// one per subsong, each addressed by the `path#n` scheme.
///
/// `fast` skips silence detection for GME tracks that declare no length. That
/// detection decodes the track in full to find where it goes quiet, which is
/// accurate and costs over a second per file — fine when a user asks about one
/// track, ruinous across a whole library.
fn probe(path: &str, fast: bool) -> Result<(), String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match tunante_codec::metadata::read_metadata_all_with_opts(Path::new(path), fast) {
        Ok(tracks) => {
            let payload = serde_json::json!({ "ok": true, "tracks": tracks });
            writeln!(out, "{payload}").map_err(|e| e.to_string())
        }
        Err(e) => {
            let msg = e.to_string();
            let payload = serde_json::json!({ "ok": false, "error": msg });
            writeln!(out, "{payload}").map_err(|e| e.to_string())?;
            Err(msg)
        }
    }
}

/// Decode a file to PCM on stdout, until it ends or the reader goes away.
fn play(path: &str, duration_hint_ms: i64) -> Result<(), String> {
    let source = tunante_codec::open_source(Path::new(path), duration_hint_ms)
        .map_err(|e| e.to_string())?;

    let header = serde_json::json!({
        "sample_rate": source.sample_rate().get(),
        "channels": source.channels().get(),
        "duration_ms": source.total_duration().map(|d| d.as_millis() as u64),
    });

    let stdout = io::stdout();
    // 64 KiB is ~0.19 s of 44.1 kHz stereo f32 — big enough that the write
    // syscall is not the bottleneck, small enough that stopping is responsive.
    let mut out = BufWriter::with_capacity(64 * 1024, stdout.lock());

    writeln!(out, "{header}").map_err(|e| e.to_string())?;

    for sample in source {
        // A closed pipe is the normal way this process ends: the player dropped
        // us because the user skipped. It is not an error.
        if out.write_all(&sample.to_ne_bytes()).is_err() {
            return Ok(());
        }
    }

    match out.flush() {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
