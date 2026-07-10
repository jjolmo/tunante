//! Format decode smoke test — meant to run in CI *before* the release build.
//!
//! For every music format Tunante supports, this opens a real fixture file, pushes
//! it through the exact same decoder the app uses, and pulls decoded PCM samples in
//! memory. No audio device is touched, so it runs headless on Linux/Windows/macOS CI.
//!
//! It catches two failure modes:
//!   1. A decoder that *explodes* (panics / segfaults on FFI) for a given format.
//!   2. A decoder that silently breaks (opens the file but returns only silence),
//!      which usually means a broken build or platform-specific regression.
//!
//! One representative fixture per decoder backend (see tests/fixtures/). Everything
//! runs **sequentially in a single test on purpose**: PSF/GSF/2SF/USF wrap C
//! libraries with global state, so `cargo test`'s default parallelism would make
//! them clash. `play_file_at_volume` in engine.rs handles the same hazard by fully
//! dropping the previous decoder before creating the next one; we mirror that here.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::gme::GmeSource;
use super::gsf::GsfSource;
use super::opus::OggOpusSource;
use super::psf::PsfSource;
use super::psf2::Psf2Source;
use super::twosf::TwoSfSource;
use super::usf::UsfSource;
use super::vgmstream::VgmstreamSource;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Pull decoded samples until real (non-silent) audio appears, the stream ends, or a
/// wall-clock budget elapses. Returns (sample_count, peak_amplitude).
///
/// The wall-clock budget + micro-sleeps matter for async decoders: USF runs Mupen64 on
/// a background thread and its `next()` returns silence-filler (`Some(0.0)`) until the
/// thread produces samples (it must never block rodio's mixer). A tight pull loop would
/// otherwise race ahead and read only that warm-up silence.
fn drain(mut it: impl Iterator<Item = f32>) -> (usize, f32) {
    // Accumulate ~1s of audio before accepting, so a soft intro (e.g. a quiet PSF)
    // yields a representative peak with comfortable margin over the silence threshold.
    const TARGET: usize = 44_100 * 2;
    let mut count = 0usize;
    let mut peak = 0.0f32;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        for _ in 0..4096 {
            match it.next() {
                Some(s) => {
                    count += 1;
                    let a = s.abs();
                    if a > peak {
                        peak = a;
                    }
                }
                None => return (count, peak),
            }
        }
        // Heard enough real audio → done.
        if count >= TARGET && peak > 1e-4 {
            return (count, peak);
        }
        if Instant::now() >= deadline {
            return (count, peak);
        }
        // Only silence so far (async decoder warming up) — yield so its thread produces.
        if peak <= 1e-4 {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Assert a decoder produced real, non-silent audio.
fn assert_ok(label: &str, count: usize, peak: f32) {
    assert!(count > 0, "{label}: decoder yielded 0 samples (failed to decode)");
    assert!(
        peak > 1e-4,
        "{label}: decoded audio is silent (peak={peak:.6}) — decoder likely broken on this platform"
    );
    eprintln!("[format_smoke] {label:<9} OK — {count} samples, peak {peak:.4}");
}

/// Let a C-library-backed decoder's global state settle before building the next one
/// (mirrors the 50ms drop-delay in engine.rs::play_file_at_volume).
fn settle() {
    std::thread::sleep(Duration::from_millis(50));
}

#[test]
fn all_supported_formats_decode() {
    let fx = fixtures_dir();
    assert!(
        fx.join("sample.psf").exists(),
        "fixtures missing at {} — did you run with the committed tests/fixtures?",
        fx.display()
    );

    // --- Native FFI decoders (highest "explode" risk) ---------------------------
    {
        let src = PsfSource::new(&fx.join("sample.psf")).expect("PSF: open/decode failed");
        let (n, p) = drain(src);
        assert_ok("psf", n, p);
    }
    settle();
    {
        let src = Psf2Source::new(&fx.join("sample.psf2")).expect("PSF2: open/decode failed");
        let (n, p) = drain(src);
        assert_ok("psf2", n, p);
    }
    settle();
    {
        let src = GsfSource::new(&fx.join("gsf/sample.minigsf")).expect("GSF: open/decode failed");
        let (n, p) = drain(src);
        assert_ok("gsf", n, p);
    }
    settle();
    {
        let src =
            TwoSfSource::new(&fx.join("twosf/sample.mini2sf")).expect("2SF: open/decode failed");
        let (n, p) = drain(src);
        assert_ok("2sf", n, p);
    }
    settle();
    {
        let src = UsfSource::new(&fx.join("usf/sample.miniusf")).expect("USF: open/decode failed");
        let (n, p) = drain(src);
        assert_ok("usf", n, p);
    }
    settle();

    // --- GME chiptune backend (NSF/SPC/VGM/GBS/HES/KSS/AY/SAP/GYM) ---------------
    {
        let src = GmeSource::new(&fx.join("sample.nsf"), 0, 30_000).expect("GME: open/decode failed");
        let (n, p) = drain(src);
        assert_ok("gme/nsf", n, p);
    }
    settle();

    // --- vgmstream backend (ADX/HCA/BCSTM/... hundreds of stream formats) --------
    {
        let src = VgmstreamSource::new(&fx.join("sample.bcstm"), 0).expect("vgmstream: open/decode failed");
        let (n, p) = drain(src);
        assert_ok("vgmstream", n, p);
    }

    // --- Custom Ogg Opus decoder (symphonia doesn't handle Opus) ----------------
    {
        let file = BufReader::new(File::open(fx.join("sine.opus")).expect("opus: open failed"));
        let src = OggOpusSource::new(file).expect("opus: decode failed");
        let (n, p) = drain(src);
        assert_ok("opus", n, p);
    }

    // --- Standard formats via rodio/symphonia (MP3/FLAC/OGG/WAV/AAC/M4A) ---------
    for name in ["sine.wav", "sine.flac", "sine.ogg", "sine.mp3", "sine.m4a"] {
        let file = File::open(fx.join(name)).unwrap_or_else(|_| panic!("{name}: open failed"));
        let src = rodio::Decoder::try_from(file)
            .unwrap_or_else(|e| panic!("{name}: symphonia decode failed: {e}"));
        let (n, p) = drain(src);
        assert_ok(name, n, p);
    }
}
