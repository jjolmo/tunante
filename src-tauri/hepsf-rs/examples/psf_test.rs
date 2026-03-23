//! PSF test — render + seek on a separate thread (mimicking rodio's audio thread)
//! Run: cargo run --manifest-path src-tauri/hepsf-rs/Cargo.toml --example psf_test

fn main() {
    let path = std::path::Path::new("/tmp/test_psf/FF7 104 Anxious Heart.minipsf");

    eprintln!("[1] Loading on main thread...");
    let (mut decoder, tags) = hepsf_rs::PsfDecoder::new(path).unwrap();
    eprintln!("[2] Loaded. title='{}', length={}ms, fade={}ms", tags.title, tags.length_ms, tags.fade_ms);
    eprintln!("[2] Moving to render thread...");

    // Mimic rodio: move decoder to another thread for rendering
    let handle = std::thread::Builder::new()
        .name("audio-render".to_string())
        .spawn(move || {
            eprintln!("[3] Render thread started");
            let mut buf = vec![0i16; 1024 * 2];

            // Render 50 chunks
            for i in 0..50 {
                decoder.render(&mut buf, 1024);
                if i == 0 || i == 1 || i == 9 || i == 49 {
                    let has_audio = buf.iter().any(|&s| s != 0);
                    eprintln!("[4] Chunk {} OK, has_audio={}", i, has_audio);
                }
            }
            eprintln!("[5] 50 chunks rendered OK");

            // Now test seeking to 30 seconds
            eprintln!("[6] Seeking to 30000ms...");
            let start = std::time::Instant::now();
            decoder.seek(30_000).unwrap();
            let elapsed = start.elapsed();
            eprintln!("[7] Seek took {:?}", elapsed);

            // Render 50 more chunks after seek
            for i in 0..50 {
                decoder.render(&mut buf, 1024);
                if i == 0 || i == 1 || i == 49 {
                    let has_audio = buf.iter().any(|&s| s != 0);
                    eprintln!("[8] Post-seek chunk {} OK, has_audio={}", i, has_audio);
                }
            }
            eprintln!("[9] 50 post-seek chunks rendered OK");

            // Seek to 60 seconds
            eprintln!("[10] Seeking to 60000ms...");
            let start = std::time::Instant::now();
            decoder.seek(60_000).unwrap();
            let elapsed = start.elapsed();
            eprintln!("[11] Seek took {:?}", elapsed);

            // Render a few more
            for i in 0..10 {
                decoder.render(&mut buf, 1024);
                if i == 0 || i == 9 {
                    let has_audio = buf.iter().any(|&s| s != 0);
                    eprintln!("[12] Post-seek-2 chunk {} OK, has_audio={}", i, has_audio);
                }
            }

            decoder.close();
            eprintln!("[13] ✅ ALL PASSED");
        })
        .unwrap();

    handle.join().unwrap();
}
