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
use std::sync::Mutex;
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

/// Serializes every test in this file. The PSF-family decoders wrap C libraries with
/// process-global state, so two tests building decoders at once would corrupt each other.
/// `lock()` is used with `unwrap_or_else(|e| e.into_inner())` so one failing test doesn't
/// poison the mutex and cascade into bogus failures in the rest.
static DECODER_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn all_supported_formats_decode() {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

// ============================================================================
// PSF library reference drift
// ============================================================================

/// A throwaway directory under the OS temp dir, removed when the guard drops.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        // No `tempfile` dev-dependency in this crate; the tag is unique per call site,
        // and a leftover from a killed run is simply reused after being cleared.
        let dir = std::env::temp_dir().join(format!("tunante-smoke-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copy the committed GSF fixture into `dir`, naming the library `lib_name` while the
/// minigsf's `_lib` tag keeps pointing at the original `mmbn.gsflib`.
fn stage_gsf_with_lib_named(dir: &Path, lib_name: &str) -> PathBuf {
    let fx = fixtures_dir().join("gsf");
    let song = dir.join("song.minigsf");
    std::fs::copy(fx.join("sample.minigsf"), &song).expect("copy minigsf");
    std::fs::copy(fx.join("mmbn.gsflib"), dir.join(lib_name)).expect("copy gsflib");
    song
}

/// A minigsf is a few hundred bytes of tags; all the audio lives in the `.gsflib` its
/// `_lib` tag names. That tag is a plain filename baked in at rip time, so real rips
/// routinely disagree with what's on disk — differing only in case, or naming another
/// region's library entirely. Case-insensitive filesystems hide this; on Linux the load
/// fails outright with `psf_load failed (code=-1)` and the track simply won't play.
///
/// Both recoveries live in `viogsf-rs`'s `psf_fopen` fallback (mirrored in the 2SF/USF/PSF
/// crates, which have the same tag-vs-disk problem).
#[test]
fn gsf_library_reference_survives_case_and_name_drift() {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Sanity check: exact name still works, so a failure below is about the fallback.
    {
        let dir = TempDir::new("gsf-exact");
        let song = stage_gsf_with_lib_named(dir.path(), "mmbn.gsflib");
        let src = GsfSource::new(&song).expect("GSF with exactly-named lib failed to load");
        let (n, p) = drain(src);
        assert_ok("gsf/exact-name", n, p);
    }
    settle();

    // Case drift — the failure actually hit in the wild (tag `AGB-A3UJ-JPN.gsflib`
    // against `agb-a3uj-jpn.gsflib` on disk).
    {
        let dir = TempDir::new("gsf-case");
        let song = stage_gsf_with_lib_named(dir.path(), "MMBN.GSFLIB");
        let src = GsfSource::new(&song)
            .expect("GSF lib differing only in case failed to load — case-insensitive fallback broken");
        let (n, p) = drain(src);
        assert_ok("gsf/case-drift", n, p);
    }
    settle();

    // Lib in a parent directory (minigsfs sorted into subfolders).
    {
        let dir = TempDir::new("gsf-parent");
        std::fs::copy(
            fixtures_dir().join("gsf/mmbn.gsflib"),
            dir.path().join("mmbn.gsflib"),
        )
        .expect("copy gsflib");
        let sub = dir.path().join("disc1");
        std::fs::create_dir_all(&sub).expect("create subdir");
        let song = sub.join("song.minigsf");
        std::fs::copy(fixtures_dir().join("gsf/sample.minigsf"), &song).expect("copy minigsf");
        let src = GsfSource::new(&song).expect("GSF lib in parent directory failed to load");
        let (n, p) = drain(src);
        assert_ok("gsf/parent-dir", n, p);
    }
    settle();

    // Tag names a library that isn't present under any casing (typically a different
    // region's filename). One lib in the folder → unambiguous, so use it.
    {
        let dir = TempDir::new("gsf-wrongname");
        let song = stage_gsf_with_lib_named(dir.path(), "AGB-B4ZJ-JPN.gsflib");
        let src = GsfSource::new(&song)
            .expect("GSF with a single differently-named lib failed to load");
        let (n, p) = drain(src);
        assert_ok("gsf/sole-lib", n, p);
    }
    settle();

    // Two candidate libs and no name match — guessing would be wrong as often as right,
    // so this must stay a clean error rather than silently playing the wrong ROM.
    {
        let dir = TempDir::new("gsf-ambiguous");
        let song = stage_gsf_with_lib_named(dir.path(), "AGB-B4ZJ-JPN.gsflib");
        std::fs::copy(
            fixtures_dir().join("gsf/mmbn.gsflib"),
            dir.path().join("AGB-XXXX-USA.gsflib"),
        )
        .expect("copy second gsflib");
        assert!(
            GsfSource::new(&song).is_err(),
            "GSF with two non-matching libs must refuse to guess"
        );
    }
}

// ============================================================================
// Real-library sweep (opt-in)
// ============================================================================

/// Pull a short burst and report (sample_count, peak) without waiting out a long intro.
/// Used by the sweep, where the question is "does this open and decode at all?" —
/// judging silence would need the 30s-per-track budget `drain` allows.
fn drain_briefly(mut it: impl Iterator<Item = f32>) -> (usize, f32) {
    const TARGET: usize = 44_100 * 2 * 2; // ~2s stereo
    let mut count = 0usize;
    let mut peak = 0.0f32;
    let deadline = Instant::now() + Duration::from_secs(10);
    while count < TARGET {
        match it.next() {
            Some(s) => {
                count += 1;
                let a = s.abs();
                if a > peak {
                    peak = a;
                }
            }
            None => break,
        }
        if count % 8192 == 0 && Instant::now() >= deadline {
            break;
        }
    }
    (count, peak)
}

/// What one sweep file yielded: decoded samples, peak amplitude, and the duration the
/// decoder reports (which is what the seek bar and the fade-out are driven from).
struct Decoded {
    count: usize,
    peak: f32,
    duration: Option<Duration>,
}

/// Pull the burst and keep the reported duration alongside it.
fn take(src: impl rodio::Source<Item = f32>) -> Decoded {
    let duration = src.total_duration();
    let (count, peak) = drain_briefly(src);
    Decoded { count, peak, duration }
}

/// Open a real library file through the same decoder `engine.rs::play_file_at_volume`
/// would pick for it.
fn decode_one(path: &Path) -> Result<Decoded, String> {
    use super::vgm_path::{is_gme_format, is_gsf_format, is_twosf_format, is_usf_format};

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lower = ext.to_ascii_lowercase();

    if is_gme_format(ext) {
        Ok(take(GmeSource::new(path, 0, 0)?))
    } else if is_gsf_format(ext) {
        Ok(take(GsfSource::new(path)?))
    } else if is_usf_format(ext) {
        Ok(take(UsfSource::new(path)?))
    } else if is_twosf_format(ext) {
        Ok(take(TwoSfSource::new(path)?))
    } else if matches!(lower.as_str(), "psf2" | "minipsf2") {
        Ok(take(Psf2Source::new(path)?))
    } else if matches!(lower.as_str(), "psf" | "minipsf") {
        Ok(take(PsfSource::new(path)?))
    } else if lower == "opus" {
        let file = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
        Ok(take(OggOpusSource::new(file)?))
    } else if matches!(
        lower.as_str(),
        "mp3" | "flac" | "ogg" | "wav" | "aac" | "aiff" | "wma" | "m4a" | "ape" | "wv"
    ) {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let src = rodio::Decoder::try_from(file).map_err(|e| e.to_string())?;
        Ok(take(src))
    } else {
        Ok(take(VgmstreamSource::new(path, 0)?))
    }
}

/// Emulated formats are looped rips cut to a play length from their tags — a couple of
/// minutes, occasionally ten for a long boss theme. A reported duration far past that
/// means the length tag was misparsed, which is not a cosmetic problem: the track never
/// reaches its fade-out, and every seek fast-forwards through the bogus timeline while
/// holding the audio lock. Streamed formats (vgmstream, mp3/flac) really can be
/// hour-long rips, so this only applies to the emulated ones.
const EMULATED_MAX_SANE: Duration = Duration::from_secs(30 * 60);

fn is_emulated_format(ext: &str) -> bool {
    use super::vgm_path::{is_gme_format, is_gsf_format, is_twosf_format, is_usf_format};
    is_gme_format(ext)
        || is_gsf_format(ext)
        || is_usf_format(ext)
        || is_twosf_format(ext)
        || matches!(
            ext.to_ascii_lowercase().as_str(),
            "psf" | "minipsf" | "psf2" | "minipsf2"
        )
}

/// Decode real tracks straight out of the user's music library — the check the committed
/// fixtures can't make, since a fixture only ever proves the decoder works on the one rip
/// that was committed with it. Broken sets in the wild (a `.gsflib` whose name drifted
/// from the tag, a truncated rip, an exotic vgmstream container) only surface here.
///
/// Opt-in, since it needs a library on disk and CI has none:
///
/// ```sh
/// TUNANTE_MUSIC_DIR="/path/to/Musica" \
///   cargo test --manifest-path src-tauri/Cargo.toml \
///   real_library_sweep -- --ignored --nocapture
/// ```
///
/// `TUNANTE_SWEEP_PER_EXT` (default 3) caps how many files are tried per extension, so a
/// large library still finishes quickly. Only failing to *open/decode* fails the test;
/// tracks that decode to silence are listed as warnings, because plenty of real tracks
/// genuinely open with a quiet intro.
#[test]
#[ignore = "needs a real music library; set TUNANTE_MUSIC_DIR"]
fn real_library_sweep() {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let Ok(root) = std::env::var("TUNANTE_MUSIC_DIR") else {
        panic!("set TUNANTE_MUSIC_DIR to the music folder to sweep");
    };
    let root = PathBuf::from(root);
    assert!(root.is_dir(), "TUNANTE_MUSIC_DIR is not a directory: {}", root.display());

    let per_ext: usize = std::env::var("TUNANTE_SWEEP_PER_EXT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    // Container/library files are loaded *by* a track, never played on their own.
    const NOT_PLAYABLE: &[&str] = &[
        "gsflib", "psflib", "psf2lib", "2sflib", "usflib", "ssflib", "dsflib", "txt", "m3u",
        "jpg", "jpeg", "png", "cue", "log", "nfo", "pdf", "sf2", "ini",
    ];

    // Group by extension and cap, so one huge folder can't crowd out rarer formats.
    let mut by_ext: std::collections::BTreeMap<String, Vec<PathBuf>> = Default::default();
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        if NOT_PLAYABLE.contains(&ext.as_str()) || ext.starts_with('.') {
            continue;
        }
        let bucket = by_ext.entry(ext).or_default();
        if bucket.len() < per_ext {
            bucket.push(path.to_path_buf());
        }
    }

    assert!(!by_ext.is_empty(), "no playable files found under {}", root.display());

    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    let mut silent: Vec<PathBuf> = Vec::new();
    let mut ok = 0usize;

    for (ext, paths) in &by_ext {
        for path in paths {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_one(path))) {
                Ok(Ok(d)) if d.count > 0 => {
                    ok += 1;
                    if d.peak <= 1e-4 {
                        silent.push(path.clone());
                    }
                    let secs = d.duration.map(|x| x.as_secs()).unwrap_or(0);
                    if is_emulated_format(ext) && d.duration.is_some_and(|x| x > EMULATED_MAX_SANE) {
                        failures.push((
                            path.clone(),
                            format!("implausible duration {}:{:02} — length tag misparsed?", secs / 60, secs % 60),
                        ));
                        eprintln!("[sweep] {ext:<10} FAIL    duration {}:{:02}  {}", secs / 60, secs % 60, path.display());
                        settle();
                        continue;
                    }
                    eprintln!(
                        "[sweep] {ext:<10} OK      {:>9} samples peak {:.4} dur {}:{:02}  {}",
                        d.count, d.peak, secs / 60, secs % 60, path.display()
                    );
                }
                Ok(Ok(_)) => {
                    failures.push((path.clone(), "decoder yielded 0 samples".into()));
                    eprintln!("[sweep] {ext:<10} FAIL    0 samples  {}", path.display());
                }
                Ok(Err(e)) => {
                    failures.push((path.clone(), e.clone()));
                    eprintln!("[sweep] {ext:<10} FAIL    {e}  {}", path.display());
                }
                Err(_) => {
                    // A panicking decoder would otherwise abort the whole sweep and hide
                    // every file after it.
                    failures.push((path.clone(), "decoder panicked".into()));
                    eprintln!("[sweep] {ext:<10} PANIC   {}", path.display());
                }
            }
            settle();
        }
    }

    eprintln!(
        "\n[sweep] {} extensions, {} decoded, {} failed, {} silent",
        by_ext.len(),
        ok,
        failures.len(),
        silent.len()
    );
    for path in &silent {
        eprintln!("[sweep] warning: decoded but silent — {}", path.display());
    }

    assert!(
        failures.is_empty(),
        "{} file(s) failed to decode:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(p, e)| format!("  {} — {e}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
