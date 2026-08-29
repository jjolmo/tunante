use lazyusf2_rs::UsfDecoder;
use std::io::Write;
use std::path::Path;

fn write_wav(path: &str, samples: &[i16], sample_rate: u32) {
    let mut f = std::fs::File::create(path).unwrap();
    let data_len = (samples.len() * 2) as u32;
    let channels: u16 = 2;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&channels.to_le_bytes()).unwrap();
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&(sample_rate * channels as u32 * 2).to_le_bytes()).unwrap();
    f.write_all(&(channels * 2).to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    for s in samples {
        f.write_all(&s.to_le_bytes()).unwrap();
    }
}

#[test]
fn render_usf_to_wav() {
    let usf = Path::new("/media/cidwel/storage/Seafile/Cidwel/Musica/OST juegos/N64/Star Fox 64/09 Corneria.miniusf");
    if !usf.exists() {
        eprintln!("SKIP: file not found");
        return;
    }

    let (mut dec, tags) = UsfDecoder::new(usf, 44100).expect("load failed");
    eprintln!("Loaded: {} - {} ({}ms)", tags.title, tags.game, tags.length_ms);

    // Render 5 seconds
    let frames = 44100 * 5;
    let mut all_samples = Vec::with_capacity(frames * 2);
    let chunk = 4096;
    let mut buf = vec![0i16; chunk * 2];
    let mut rendered = 0;
    while rendered < frames {
        let n = chunk.min(frames - rendered);
        dec.render(&mut buf[..n * 2], n).expect("render failed");
        all_samples.extend_from_slice(&buf[..n * 2]);
        rendered += n;
    }

    let max = all_samples.iter().map(|s| s.abs() as u32).max().unwrap_or(0);
    let rms = (all_samples
        .iter()
        .map(|s| (*s as f64).powi(2))
        .sum::<f64>()
        / all_samples.len() as f64)
        .sqrt();
    eprintln!("Rendered {} frames, max={}, RMS={:.1}", rendered, max, rms);

    write_wav("/tmp/starfox64_corneria.wav", &all_samples, 44100);
    eprintln!("Wrote /tmp/starfox64_corneria.wav — play it with: aplay /tmp/starfox64_corneria.wav");

    assert!(max > 100, "Audio is essentially silent (max={})", max);
    assert!(rms > 10.0, "Audio RMS too low ({})", rms);
}
