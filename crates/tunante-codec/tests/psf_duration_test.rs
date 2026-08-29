//! PSF timestamp parsing — the `length`/`fade` tags that drive track duration,
//! fade-out timing and the seek bar.
//!
//! These go through sexypsf's C tag parser (`sexypsf/Misc.c::TimeToMS`), reached from
//! Rust via `hepsf_rs::read_psf_tags`. That parser is `static`, so rather than calling it
//! directly each case rewrites the tag block of the committed `sample.psf` fixture and
//! reads it back through the real code path.
//!
//! Run: cargo test --test psf_duration_test

use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.psf")
}

/// Rewrite the fixture's tag block with `tags` and write it to `dest`.
///
/// PSF layout: `"PSF\x01"`, 4-byte reserved length, 4-byte program length, 4-byte CRC,
/// then the reserved and program blocks, then an optional `[TAG]` area. Only the tag
/// area is replaced, so the program and its CRC stay valid.
fn write_psf_with_tags(dest: &Path, tags: &str) {
    let data = std::fs::read(fixture()).expect("read fixture");
    let reserved_len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let program_len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let tag_start = 16 + reserved_len + program_len;

    let mut out = data[..tag_start].to_vec();
    if !tags.is_empty() {
        out.extend_from_slice(b"[TAG]");
        out.extend_from_slice(tags.as_bytes());
    }
    std::fs::write(dest, out).expect("write tagged psf");
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("tunante-psfdur-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Read back the `length` a PSF tagged with `timestamp` reports, in milliseconds.
fn length_ms_for(dir: &TempDir, label: &str, timestamp: &str) -> u64 {
    let path = dir.0.join(format!("{label}.psf"));
    write_psf_with_tags(&path, &format!("length={timestamp}\nfade=0\n"));
    hepsf_rs::read_psf_tags(&path)
        .unwrap_or_else(|e| panic!("read_psf_tags failed for length={timestamp}: {e}"))
        .length_ms
}

/// The fractional part of a PSF timestamp is a decimal fraction of a second, so how much
/// it contributes depends on the number of digits written — `.9` is 900ms, `.974` is
/// 974ms. sexypsf used to read it as a fixed count of tenths, making every timestamp with
/// more than one decimal digit far too long: Shin Megami Tensei's `0:28.974162` reported
/// 27 hours instead of 29 seconds, so the track never faded out and the seek bar was
/// unusable (each drag fast-forwarded through hours of emulation while holding the audio
/// lock).
#[test]
fn psf_length_tag_parses_to_correct_milliseconds() {
    let dir = TempDir::new("length");

    let cases: &[(&str, u64)] = &[
        // Plain seconds and mm:ss — the formats that always worked.
        ("45", 45_000),
        ("0:15", 15_000),
        ("2:30", 150_000),
        // One decimal digit — tenths.
        ("0:15.9", 15_900),
        // Two digits — hundredths. Used to come out 10x too long (0:37).
        ("1:23.45", 83_450),
        // Three digits, the usual mm:ss.mmm. Used to come out 100x too long.
        ("2:30.000", 150_000),
        ("0:28.974", 28_974),
        // The real-world file: six digits. Used to report 1624:04.
        ("0:28.974162", 28_974),
        // Comma is accepted as a decimal separator too.
        ("0:15,5", 15_500),
        // hh:mm:ss, for the rare long track.
        ("1:02:03", 3_723_000),
    ];

    for (timestamp, expected) in cases {
        let got = length_ms_for(&dir, &timestamp.replace([':', '.', ','], "_"), timestamp);
        assert_eq!(
            got, *expected,
            "length={timestamp} parsed as {got}ms, expected {expected}ms"
        );
    }
}

/// `fade` runs through the same parser, and feeds the fade-out ramp.
#[test]
fn psf_fade_tag_parses_to_correct_milliseconds() {
    let dir = TempDir::new("fade");
    let path = dir.0.join("fade.psf");
    write_psf_with_tags(&path, "length=0:30\nfade=10.5\n");
    let tags = hepsf_rs::read_psf_tags(&path).expect("read tags");
    assert_eq!(tags.length_ms, 30_000);
    assert_eq!(tags.fade_ms, 10_500);
}

/// A PSF with no `length` tag plays forever, which sexypsf signals by setting its `stop`
/// field to `~0`. Passed through untouched that reads as a 49-day track — a number
/// enormous enough to slip past every `length > 0` check upstream, so the 2.5-minute
/// default for unknown-length tracks never applied. It must surface as "unknown" (0).
#[test]
fn psf_without_length_tag_reports_unknown_not_infinity() {
    let dir = TempDir::new("nolength");

    let untagged = dir.0.join("untagged.psf");
    write_psf_with_tags(&untagged, "");
    let tags = hepsf_rs::read_psf_tags(&untagged).expect("read tags (no tag block)");
    assert_eq!(
        tags.length_ms, 0,
        "PSF with no tag block must report unknown length, got {}ms",
        tags.length_ms
    );

    let other_tags_only = dir.0.join("titled.psf");
    write_psf_with_tags(&other_tags_only, "title=Untimed\nartist=Nobody\n");
    let tags = hepsf_rs::read_psf_tags(&other_tags_only).expect("read tags (no length)");
    assert_eq!(
        tags.length_ms, 0,
        "PSF with tags but no length must report unknown length, got {}ms",
        tags.length_ms
    );
    assert_eq!(tags.title, "Untimed", "unrelated tags must still be read");
}
