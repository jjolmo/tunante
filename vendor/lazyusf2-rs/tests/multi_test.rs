use lazyusf2_rs::UsfDecoder;

#[test]
fn test_multiple_tracks_sequentially() {
    let base = "/media/cidwel/storage/Seafile/Cidwel/Musica/OST juegos/N64/Star Fox 64/";
    let tracks = ["01 Opening.miniusf", "09 Corneria.miniusf", "02 Title.miniusf"];

    for name in &tracks {
        let path = std::path::Path::new(base).join(name);
        if !path.exists() {
            eprintln!("SKIP: {} not found", name);
            continue;
        }

        eprintln!("Loading: {}", name);
        let (mut dec, tags) = UsfDecoder::new(&path, 44100).expect(&format!("Failed to load {}", name));
        eprintln!("  Title: {}, Length: {}ms", tags.title, tags.length_ms);

        // Render 1 second
        let mut buf = vec![0i16; 44100 * 2];
        dec.render(&mut buf, 44100).expect(&format!("Failed to render {}", name));
        let max = buf.iter().map(|s| s.abs() as u32).max().unwrap_or(0);
        eprintln!("  Max amplitude: {}", max);
        assert!(max > 0, "{} produced silence", name);

        // Drop decoder explicitly to test cleanup
        drop(dec);
        eprintln!("  OK - dropped");
    }
    eprintln!("All tracks decoded successfully!");
}
