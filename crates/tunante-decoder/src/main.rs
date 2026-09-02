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
//! tunante-decoder probe <path> [--fast] [--loop-max-ms N] [--vgm-loops F] [--caps-all]
//!     stdout: one line of JSON — {"ok":true,"tracks":[…]} or {"ok":false,"error":"…"}
//!     --fast skips silence detection for GME tracks with no declared length.
//!            Those are decoded in full to find where they stop, which costs
//!            seconds per file — the difference between a scan of minutes and
//!            one of half an hour.
//!     The rest are the library's scan knobs, travelling down the pipe so a
//!     pipe-based app's durations agree with what its player will do:
//!     --loop-max-ms caps the endless tracks, --vgm-loops must match playback,
//!     --caps-all applies the cap over declared lengths too.
//!
//! tunante-decoder play <path> [duration_hint_ms] [--loops N] [--fade MS] [--vgm-loops F]
//!     stdout: one line of JSON header, then raw PCM until EOF
//!             {"sample_rate":44100,"channels":2,"duration_ms":123456}
//!             <f32 native-endian, interleaved>
//!     stdin:  newline-separated commands
//!             seek <ms>     jump, then keep streaming from there
//!
//! tunante-decoder art <path>
//!     stdout: one line of JSON — {"ok":true,"art":"data:image/jpeg;base64,…"}
//!             or {"ok":true,"art":null} when the file carries none
//!
//! tunante-decoder rate <path> <rating> [--order db,file,folder]
//!     stdout: one line of JSON — {"ok":true,"stored_in":"file","skipped":[…]}
//!     Writes the rating to disk following the priority order (the caller's
//!     database half is the caller's business; an order starting with db
//!     means "don't touch the disk", same as everywhere else).
//! ```
//!
//! The header is a text line so the stream can be inspected with `head -1`; the
//! PCM that follows is raw because it is the hot path.
//!
//! Exit code is 0 on success, 1 on failure, and the error goes to stderr as well
//! as into the JSON, so a caller that only wants a status does not have to parse.

use std::io::{self, BufRead, BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use rodio::Source;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    let usage = || {
        eprintln!("usage: tunante-decoder probe <path> [--fast] [--loop-max-ms N] [--vgm-loops F] [--caps-all]");
        eprintln!("       tunante-decoder play  <path> [hint_ms] [--loops N] [--fade MS] [--vgm-loops F]");
        eprintln!("       tunante-decoder art   <path>");
        eprintln!("       tunante-decoder rate  <path> <rating> [--order db,file,folder]");
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
        "probe" => {
            let flag = |name: &str| args.iter().skip_while(|a| *a != name).nth(1).cloned();
            probe(
                path,
                args.iter().any(|a| a == "--fast"),
                flag("--loop-max-ms").and_then(|s| s.parse().ok()),
                flag("--vgm-loops").and_then(|s| s.parse().ok()),
                args.iter().any(|a| a == "--caps-all"),
            )
        }
        "art" => art(path),
        "rate" => {
            let rating = args
                .get(3)
                .and_then(|s| s.parse::<i32>().ok())
                .filter(|r| (0..=5).contains(r));
            let order = args
                .iter()
                .skip_while(|a| *a != "--order")
                .nth(1)
                .map(String::as_str);
            match rating {
                Some(r) => rate(path, r, order),
                None => Err("rate needs a rating between 0 and 5".to_string()),
            }
        }
        "play" => {
            let hint = args
                .get(3)
                .filter(|a| !a.starts_with('-'))
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);

            let flag = |name: &str| -> Option<u64> {
                args.iter().skip_while(|a| *a != name).nth(1)?.parse().ok()
            };

            let mut opts = tunante_codec::PlaybackOptions::default();
            if let Some(n) = flag("--loops") {
                opts.loop_count = n as u32;
            }
            if let Some(ms) = flag("--fade") {
                opts.fade_ms = ms;
            }
            // A user setting, so it has to be able to travel down here: it must
            // match what the scanner used, or the progress bar disagrees with
            // what is heard.
            if let Some(v) = args
                .iter()
                .skip_while(|a| *a != "--vgm-loops")
                .nth(1)
                .and_then(|s| s.parse::<f64>().ok())
            {
                opts.vgm_loop_count = v;
            }

            play(path, hint, opts)
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
fn probe(
    path: &str,
    fast: bool,
    loop_max_ms: Option<i64>,
    vgm_loops: Option<f64>,
    caps_all: bool,
) -> Result<(), String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut opts = tunante_codec::metadata::ScanOpts {
        fast_scan: fast,
        loop_max_caps_all: caps_all,
        ..Default::default()
    };
    if let Some(ms) = loop_max_ms {
        opts.loop_max_ms = ms;
    }
    if let Some(v) = vgm_loops {
        opts.vgm_loop_count = v;
    }
    match tunante_codec::metadata::read_metadata_all_with_opts(Path::new(path), opts) {
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

/// The cover art a file carries, as a data URI.
///
/// Separate from `probe` because it is asked for once per *playing* track,
/// whereas probe runs on every file in a library scan — and a scan that also
/// carried the artwork of thousands of tracks through a pipe would be slower
/// for no one's benefit.
fn art(path: &str) -> Result<(), String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match tunante_codec::metadata::extract_artwork_base64(Path::new(path)) {
        Ok(art) => {
            let payload = serde_json::json!({ "ok": true, "art": art });
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

/// Write a rating to disk following the priority order, and report where it
/// landed. Lives here rather than in the apps because the writers are
/// tunante-codec's, and only this binary and the desktop link that crate —
/// this is how the pipe-based apps store a rating in the file or the folder's
/// `_ratings.m3u` without linking every vendored core.
fn rate(path: &str, rating: i32, order: Option<&str>) -> Result<(), String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let order = tunante_codec::metadata::rating_source::parse_order(order);
    let outcome = tunante_codec::metadata::rating_source::write_rating(path, rating, &order);

    let payload = serde_json::json!({
        "ok": true,
        "stored_in": outcome.stored_in.map(|s| s.as_key()),
        "skipped": outcome.skipped.iter().map(|s| s.as_key()).collect::<Vec<_>>(),
    });
    writeln!(out, "{payload}").map_err(|e| e.to_string())
}

/// Seek requests arriving on stdin, in milliseconds. -1 means nothing pending.
static SEEK_TO_MS: AtomicI64 = AtomicI64::new(-1);

/// Watch stdin for control commands.
///
/// A thread rather than polling: reading stdin blocks, and the decode loop must
/// not stall waiting for a command that may never come. Only the newest seek
/// survives, which is what dragging a progress bar should do — the intermediate
/// positions are not worth decoding.
fn watch_commands() {
    std::thread::spawn(|| {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { return };
            let mut parts = line.split_whitespace();
            match (parts.next(), parts.next()) {
                (Some("seek"), Some(ms)) => {
                    if let Ok(ms) = ms.parse::<i64>() {
                        SEEK_TO_MS.store(ms.max(0), Ordering::Relaxed);
                    }
                }
                (Some("stop"), _) => std::process::exit(0),
                _ => {}
            }
        }
    });
}

/// Decode a file to PCM on stdout, until it ends or the reader goes away.
fn play(
    path: &str,
    duration_hint_ms: i64,
    opts: tunante_codec::PlaybackOptions,
) -> Result<(), String> {
    let mut source = tunante_codec::open_source_with(Path::new(path), duration_hint_ms, opts)
        .map_err(|e| e.to_string())?;

    let header = serde_json::json!({
        "sample_rate": source.sample_rate().get(),
        "channels": source.channels().get(),
        "duration_ms": source.total_duration().map(|d| d.as_millis() as u64),
        "can_seek": true,
    });

    let stdout = io::stdout();
    // 64 KiB is ~0.19 s of 44.1 kHz stereo f32 — big enough that the write
    // syscall is not the bottleneck, small enough that stopping is responsive.
    let mut out = BufWriter::with_capacity(64 * 1024, stdout.lock());

    writeln!(out, "{header}").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;

    watch_commands();

    // Checked once per chunk rather than once per sample: a relaxed atomic load
    // is cheap, but not 44 100 times a second for nothing.
    const CHUNK: usize = 2048;

    loop {
        let pending = SEEK_TO_MS.swap(-1, Ordering::Relaxed);
        if pending >= 0 {
            // A backend that cannot seek says so; the position simply does not
            // move, which is better than dying mid-track.
            let _ = source.try_seek(Duration::from_millis(pending as u64));
        }

        let mut wrote_any = false;
        for _ in 0..CHUNK {
            let Some(sample) = source.next() else { break };
            wrote_any = true;
            // A closed pipe is the normal way this process ends: the player
            // dropped us because the user skipped. It is not an error.
            if out.write_all(&sample.to_ne_bytes()).is_err() {
                return Ok(());
            }
        }

        if !wrote_any {
            break;
        }
    }

    match out.flush() {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
