//! Minimal PSF crash test — no code between renders
//!
//! Run: cargo test --manifest-path src-tauri/Cargo.toml --test psf_seek_test -- --nocapture

use std::path::Path;

const TEST_PSF: &str = "/media/cidwel/storage/Seafile/Cidwel/Musica/OST juegos/PSX/FF7_psf/FF7 104 Anxious Heart.minipsf";

#[test]
fn test_psf_bare_renders() {
    let path = Path::new(TEST_PSF);
    if !path.exists() {
        eprintln!("SKIP: test PSF file not found");
        return;
    }

    eprintln!("[1] Loading PSF...");
    let (mut decoder, _) = hepsf_rs::PsfDecoder::new(path).expect("Failed to load");
    eprintln!("[2] Loaded OK");

    let mut buf = vec![0i16; 1024 * 2];

    // Render 10 chunks back-to-back with NO code in between
    eprintln!("[3] Rendering 10 consecutive chunks with NO code between...");
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    decoder.render(&mut buf, 1024);
    eprintln!("[4] ✅ All 10 chunks rendered OK");

    decoder.close();
    eprintln!("[5] ✅ Done");
}
