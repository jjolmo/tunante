//! ¿Suena GME al tono correcto?
//!
//! El tono de una pieza no debe depender de a qué frecuencia se renderice. Si
//! pedirle a GME 44100 Hz y 48000 Hz da dos tonos distintos, hay un fallo de
//! remuestreo dentro del emulador — y eso es exactamente lo que se oye como
//! "va un poco rápido y más agudo".
//!
//! Mide el pico espectral de los mismos segundos de música a las dos
//! frecuencias. Si el emulador es correcto, las dos cifras coinciden.
//!
//!     cargo run --release -p tunante-codec --example gme_pitch_check -- fichero.vgm

use game_music_emu::GameMusicEmu;

/// Cuántos segundos analizar, saltándose el arranque.
const SKIP_SECS: f64 = 1.0;
const ANALYSE_SECS: f64 = 2.0;

/// Barrido de frecuencias candidatas, en Hz. Cubre de un bajo grave a un
/// armónico agudo con resolución de un cuarto de Hz, de sobra para ver una
/// desviación del uno por ciento.
const F_MIN: f64 = 60.0;
const F_MAX: f64 = 1200.0;
const F_STEP: f64 = 0.25;

fn render(path: &str, rate: u32) -> Vec<f32> {
    let emu = GameMusicEmu::from_file(std::path::Path::new(path), rate)
        .unwrap_or_else(|e| panic!("no se pudo abrir: {e:?}"));
    emu.start_track(0).expect("start_track");

    let total = ((SKIP_SECS + ANALYSE_SECS) * rate as f64) as usize;
    let mut buf = vec![0i16; total * 2];
    emu.play(buf.len(), &mut buf).expect("play");

    // Sólo la parte a analizar, y en mono: el tono es el mismo en los dos
    // canales y sumar reduce el ruido.
    let skip = (SKIP_SECS * rate as f64) as usize;
    buf[skip * 2..]
        .chunks_exact(2)
        .map(|c| (c[0] as f32 + c[1] as f32) / 2.0)
        .collect()
}

/// Frecuencia con más energía, por barrido directo (no hace falta una FFT para
/// buscar un único pico en un rango estrecho).
fn peak_hz(samples: &[f32], rate: u32) -> f64 {
    let n = samples.len();
    let mut best = (0.0f64, 0.0f64);
    let mut f = F_MIN;
    while f <= F_MAX {
        let w = std::f64::consts::TAU * f / rate as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in samples.iter().enumerate() {
            let a = w * i as f64;
            re += *s as f64 * a.cos();
            im += *s as f64 * a.sin();
        }
        let mag = (re * re + im * im).sqrt() / n as f64;
        if mag > best.1 {
            best = (f, mag);
        }
        f += F_STEP;
    }
    best.0
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("uso: gme_pitch_check <fichero.vgm|.spc|.nsf>");
        std::process::exit(2);
    });

    println!("analizando {ANALYSE_SECS}s de {path}\n");

    let mut results = Vec::new();
    for rate in [44100u32, 48000, 32000] {
        let samples = render(&path, rate);
        let hz = peak_hz(&samples, rate);
        println!("  renderizado a {rate:>5} Hz  ->  pico en {hz:8.2} Hz");
        results.push(hz);
    }

    let base = results[0];
    let worst = results
        .iter()
        .map(|h| ((h - base) / base * 100.0).abs())
        .fold(0.0f64, f64::max);

    println!("\ndesviación máxima: {worst:.2} %");
    if worst > 0.5 {
        println!("EL TONO DEPENDE DE LA FRECUENCIA DE SALIDA — hay un fallo de remuestreo");
        std::process::exit(1);
    }
    println!("el tono no depende de la frecuencia de salida: el emulador es consistente");
}
