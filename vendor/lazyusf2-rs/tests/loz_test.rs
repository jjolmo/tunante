use lazyusf2_rs::UsfDecoder;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn test_loz03_abort() {
    let path = std::path::Path::new(
        "/media/cidwel/storage/Seafile/Cidwel/Musica/OST juegos/N64/Ocarina of Time/LOZ03.miniusf",
    );
    if !path.exists() {
        eprintln!("SKIP: file not found");
        return;
    }

    eprintln!("Loading LOZ03.miniusf...");
    let (mut dec, tags) = UsfDecoder::new(path, 44100).expect("Failed to load");
    eprintln!("  Title: {}, EnableCompare: {}", tags.title, tags.enable_compare);

    // Start render in a thread and abort after 2 seconds
    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();
    let state_ptr = dec.state_ptr();
    
    let handle = std::thread::spawn(move || {
        let mut buf = vec![0i16; 2048 * 2];
        eprintln!("  Rendering (may block if stuck)...");
        let start = std::time::Instant::now();
        let result = dec.render(&mut buf, 2048);
        let elapsed = start.elapsed();
        eprintln!("  Render returned after {:.2}s: {:?}", elapsed.as_secs_f64(), result.is_ok());
        done2.store(true, Ordering::Relaxed);
    });

    // Wait 2 seconds, then abort if still running
    std::thread::sleep(std::time::Duration::from_secs(2));
    if !done.load(Ordering::Relaxed) {
        eprintln!("  Setting abort flag...");
        extern "C" {
            fn usf_set_abort_flag(state: *mut std::ffi::c_void, abort: i32);
        }
        unsafe { usf_set_abort_flag(state_ptr, 1); }
    }

    // Should join quickly now
    let join_result = handle.join();
    eprintln!("  Thread joined: {}", join_result.is_ok());
    assert!(join_result.is_ok(), "Decode thread should exit after abort");
}
