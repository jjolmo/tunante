//! PSF bare-render crash test — exercises the raw `hepsf_rs` decoder (render/close)
//! directly, complementing the higher-level format smoke test in
//! `src/audio/format_smoke.rs` (which goes through the app's `PsfSource` wrapper).
//!
//! Uses the committed fixture so it always runs (no external file needed).
//! Run: cargo test --test psf_seek_test -- --nocapture

use std::path::PathBuf;

fn test_psf() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.psf")
}

#[test]
fn psf_bare_renders_without_crashing() {
    let path = test_psf();
    assert!(
        path.exists(),
        "committed PSF fixture missing: {}",
        path.display()
    );

    let (mut decoder, _tags) = hepsf_rs::PsfDecoder::new(path.as_path()).expect("Failed to load PSF");

    // Render several chunks back-to-back with no work in between — this pattern used
    // to expose a crash in the sexypsf C globals.
    let mut buf = vec![0i16; 1024 * 2];
    let mut peak: i16 = 0;
    for _ in 0..10 {
        decoder.render(&mut buf, 1024);
        for &s in &buf {
            peak = peak.max(s.abs());
        }
    }
    assert!(peak > 0, "PSF rendered only silence — decoder likely broken");

    decoder.close();
}
