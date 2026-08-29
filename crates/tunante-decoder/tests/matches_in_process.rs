//! The helper process must decode a file to exactly the same PCM the in-process
//! decoder produces. If it does not, moving the emulator cores out of process
//! changes what the user hears, and the whole design is not worth having.
//!
//! Run: cargo test -p tunante-decoder --release
//!
//! Release mode matters — the emulator cores are far too slow in debug.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rodio::Source;

/// Backends that decode synchronously on the calling thread. For these the same
/// input must give the same bytes in either process, so we compare bit for bit.
const SYNCHRONOUS_FIXTURES: &[&str] = &[
    "sine.mp3",
    "sine.flac",
    "sine.ogg",
    "sine.wav",
    "sine.m4a",
    "sine.opus",
    "sample.nsf",
    "sample.psf",
    "sample.psf2",
    "sample.bcstm",
    "gsf/sample.minigsf",
    "twosf/sample.mini2sf",
];

/// Backends that decode on a background thread and return silence-filler until
/// it catches up — see the note at the top of `tunante_codec`'s `format_smoke`.
///
/// How much leading silence you get therefore depends on how fast you pull,
/// which makes a bit-exact comparison between two processes meaningless: the
/// subprocess and this one are not pulling at the same rate. What must hold is
/// that the stream is described identically and that real audio does arrive.
const ASYNC_FIXTURES: &[&str] = &["usf/sample.miniusf"];

/// How much of each track to compare. Long enough to get past any decoder
/// warm-up and well into steady state, short enough that the N64 core — the
/// slowest by a distance — does not make the suite drag.
const COMPARE_SAMPLES: usize = 32_768;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tunante-codec/tests/fixtures")
}

fn decoder_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tunante-decoder")
}

struct Decoded {
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
}

/// Decode through the helper process, exactly as tunante-mini will.
///
/// Reads at most `max_samples`, stopping early once `stop` says the samples so
/// far are enough — that is how the async backends get given the wall-clock time
/// their background thread needs instead of being raced past.
fn decode_out_of_process(
    path: &Path,
    max_samples: usize,
    stop: impl Fn(&[f32]) -> bool,
) -> Decoded {
    let mut child = Command::new(decoder_bin())
        .arg("play")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tunante-decoder");

    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    let mut header_line = String::new();
    reader.read_line(&mut header_line).expect("read header");
    let header: serde_json::Value =
        serde_json::from_str(header_line.trim()).unwrap_or_else(|e| {
            panic!("header for {} was not JSON ({e}): {header_line:?}", path.display())
        });

    let sample_rate = header["sample_rate"].as_u64().expect("sample_rate") as u32;
    let channels = header["channels"].as_u64().expect("channels") as u16;

    let mut bytes = vec![0u8; max_samples * 4];
    let mut filled = 0;
    while filled < bytes.len() {
        match reader.read(&mut bytes[filled..]) {
            Ok(0) => break, // track is shorter than the window; that is fine
            Ok(n) => {
                filled += n;
                let whole = filled / 4 * 4;
                let so_far: Vec<f32> = bytes[..whole]
                    .chunks_exact(4)
                    .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                if stop(&so_far) {
                    filled = whole;
                    break;
                }
            }
            Err(e) => panic!("reading PCM for {}: {e}", path.display()),
        }
    }

    // We have what we need; the child dies with the pipe, which is the same way
    // a track change will end it in the real player.
    let _ = child.kill();
    let _ = child.wait();

    let samples = bytes[..filled / 4 * 4]
        .chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    Decoded { sample_rate, channels, samples }
}

fn is_audible(samples: &[f32]) -> bool {
    samples.iter().any(|s| s.abs() > 1e-6)
}

/// Decode in this process, the way the desktop app does today.
fn decode_in_process(path: &Path) -> Decoded {
    let source = tunante_codec::open_source(path, 0).expect("open_source");
    let sample_rate = source.sample_rate().get();
    let channels = source.channels().get();
    let samples: Vec<f32> = source.take(COMPARE_SAMPLES).collect();

    Decoded { sample_rate, channels, samples }
}

#[test]
fn synchronous_backends_decode_identically_out_of_process() {
    let fx = fixtures_dir();
    assert!(
        fx.is_dir(),
        "fixtures missing at {} — they live in the tunante-codec crate",
        fx.display()
    );

    for name in SYNCHRONOUS_FIXTURES {
        let path = fx.join(name);
        assert!(path.exists(), "fixture {name} is missing at {}", path.display());

        let out = decode_out_of_process(&path, COMPARE_SAMPLES, |_| false);
        let inp = decode_in_process(&path);

        assert_eq!(
            out.sample_rate, inp.sample_rate,
            "{name}: sample rate differs between processes"
        );
        assert_eq!(
            out.channels, inp.channels,
            "{name}: channel count differs between processes"
        );

        // The helper reads in whole 4-byte samples but may stop on a pipe
        // boundary, so compare the overlap rather than demanding equal lengths.
        let n = out.samples.len().min(inp.samples.len());
        assert!(
            n > 0,
            "{name}: decoded nothing out of process ({} bytes in process)",
            inp.samples.len()
        );

        // Bit-exact: same code, same input, no resampling on either path.
        if let Some(i) = (0..n).find(|&i| out.samples[i] != inp.samples[i]) {
            panic!(
                "{name}: PCM diverges at sample {i} — out-of-process {}, in-process {}",
                out.samples[i], inp.samples[i]
            );
        }

        // Two identical streams of silence would satisfy the comparison above
        // while proving nothing, which is the failure the codec smoke test
        // exists to catch. Guard it here too, cheaply.
        assert!(
            is_audible(&out.samples),
            "{name}: out-of-process output is entirely silent"
        );
    }
}

#[test]
fn async_backends_reach_real_audio_out_of_process() {
    let fx = fixtures_dir();

    for name in ASYNC_FIXTURES {
        let path = fx.join(name);
        assert!(path.exists(), "fixture {name} is missing at {}", path.display());

        // Give the background emulator thread room to get past its warm-up
        // filler. ~11 s of 44.1 kHz stereo is far more than it needs, and we
        // stop the moment real audio shows up, so the usual cost is much less.
        let out = decode_out_of_process(&path, 1_000_000, |so_far| is_audible(so_far));
        let inp = decode_in_process(&path);

        assert_eq!(
            out.sample_rate, inp.sample_rate,
            "{name}: sample rate differs between processes"
        );
        assert_eq!(
            out.channels, inp.channels,
            "{name}: channel count differs between processes"
        );
        assert!(
            is_audible(&out.samples),
            "{name}: never produced real audio out of process — read {} samples, all silence",
            out.samples.len()
        );
    }
}

#[test]
fn probe_reports_the_same_tracks_as_an_in_process_read() {
    let fx = fixtures_dir();

    for name in ["sample.nsf", "sample.psf", "sine.flac"] {
        let path = fx.join(name);

        let output = Command::new(decoder_bin())
            .arg("probe")
            .arg(&path)
            .output()
            .expect("run tunante-decoder probe");

        assert!(
            output.status.success(),
            "{name}: probe failed — {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("probe emitted JSON");
        assert_eq!(parsed["ok"], true, "{name}: probe reported failure");

        let from_child = parsed["tracks"].as_array().expect("tracks array").len();
        let in_process = tunante_codec::metadata::read_metadata_all(&path)
            .expect("in-process metadata read")
            .len();

        assert_eq!(
            from_child, in_process,
            "{name}: probe found {from_child} tracks, in-process read found {in_process}"
        );
    }
}

#[test]
fn a_missing_file_fails_loudly_instead_of_hanging() {
    let output = Command::new(decoder_bin())
        .arg("play")
        .arg("/nonexistent/does-not-exist.psf")
        .output()
        .expect("run tunante-decoder");

    assert!(!output.status.success(), "expected a non-zero exit");
    assert!(
        !output.stderr.is_empty(),
        "expected the reason on stderr, got nothing"
    );
}
