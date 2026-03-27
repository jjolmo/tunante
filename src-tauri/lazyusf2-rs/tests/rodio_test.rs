use lazyusf2_rs::UsfDecoder;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::time::Duration;

/// Minimal rodio-compatible Source for testing
struct TestUsfSource {
    decoder: UsfDecoder,
    buffer: Vec<f32>,
    buf_pos: usize,
    frame_no: u64,
    frame_total: u64,
    finished: bool,
}

impl TestUsfSource {
    fn new(path: &Path) -> Self {
        let (decoder, tags) = UsfDecoder::new(path, 44100).unwrap();
        let total_ms = if tags.length_ms > 0 { tags.length_ms } else { 5000 }; // 5s max for test
        let total_ms = total_ms.min(5000);
        let frame_total = total_ms * 44100 / 1000;
        Self {
            decoder,
            buffer: Vec::new(),
            buf_pos: 0,
            frame_no: 0,
            frame_total,
            finished: false,
        }
    }
}

impl Iterator for TestUsfSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.finished && self.buf_pos >= self.buffer.len() {
            return None;
        }
        if self.buf_pos >= self.buffer.len() {
            if self.frame_no >= self.frame_total {
                self.finished = true;
                return None;
            }
            let chunk = 2048usize;
            let remaining = (self.frame_total - self.frame_no) as usize;
            let n = chunk.min(remaining);
            let mut i16_buf = vec![0i16; n * 2];
            if self.decoder.render(&mut i16_buf, n).is_err() {
                self.finished = true;
                return None;
            }
            self.buffer.clear();
            for s in &i16_buf[..n * 2] {
                self.buffer.push(*s as f32 / 32768.0);
            }
            self.frame_no += n as u64;
            self.buf_pos = 0;
        }
        let sample = self.buffer[self.buf_pos];
        self.buf_pos += 1;
        Some(sample)
    }
}

impl rodio::Source for TestUsfSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.buf_pos < self.buffer.len() {
            Some(self.buffer.len() - self.buf_pos)
        } else {
            None
        }
    }
    fn channels(&self) -> NonZeroU16 { NonZeroU16::new(2).unwrap() }
    fn sample_rate(&self) -> NonZeroU32 { NonZeroU32::new(44100).unwrap() }
    fn total_duration(&self) -> Option<Duration> { None }
    fn try_seek(&mut self, _: Duration) -> Result<(), rodio::source::SeekError> { Ok(()) }
}

#[test]
fn test_usf_with_rodio_sequential() {
    let base = "/media/cidwel/storage/Seafile/Cidwel/Musica/OST juegos/N64/Star Fox 64/";
    let tracks = ["09 Corneria.miniusf", "02 Title.miniusf", "04 Map.miniusf"];

    // Create a rodio output via DeviceSinkBuilder (rodio 0.22)
    let device = rodio::DeviceSinkBuilder::from_default_device().expect("No audio device").build().expect("No audio output");
    let player = rodio::Player::connect_new(&device.mixer());

    for name in &tracks {
        let path = Path::new(base).join(name);
        if !path.exists() {
            eprintln!("SKIP: {}", name);
            continue;
        }

        eprintln!("Playing: {}", name);
        let source = TestUsfSource::new(&path);
        player.append(source);
        player.play();

        // Wait for it to finish (max 6 seconds)
        let start = std::time::Instant::now();
        while !player.empty() {
            std::thread::sleep(Duration::from_millis(100));
            if start.elapsed() > Duration::from_secs(6) {
                eprintln!("  TIMEOUT - audio thread may be blocked!");
                break;
            }
        }
        eprintln!("  Done ({:.1}s)", start.elapsed().as_secs_f64());
    }
    eprintln!("All tracks played!");
}
