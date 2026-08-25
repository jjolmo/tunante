//! ¿Suena GME al tono correcto?
//!
//! Dos comprobaciones distintas sobre el mismo fichero:
//!
//! 1. **Consistencia.** El tono no debe depender de a qué frecuencia se
//!    renderice. Si pedirle a GME 44100 Hz y 48000 Hz da tonos distintos, hay un
//!    fallo de remuestreo dentro del emulador.
//!
//! 2. **Tono absoluto, voz por voz.** Un tema de Mega Drive mezcla voces FM y un
//!    canal DAC de percusión, y separarlos mirando el espectro es inútil: el
//!    bombo tapa al bajo. Silenciando todas las voces menos una, el pico que
//!    queda es de esa voz y ya se puede comparar con la nota que el fichero
//!    escribe en los registros del chip.
//!
//!     cargo run --release -p tunante-codec --example gme_pitch_check -- fichero.vgm [Hz esperados]

use game_music_emu::GameMusicEmu;

const SKIP_SECS: f64 = 0.8;
const ANALYSE_SECS: f64 = 2.2;

fn render(path: &str, rate: u32, solo_voice: Option<i32>) -> Vec<f64> {
    let emu = GameMusicEmu::from_file(std::path::Path::new(path), rate)
        .unwrap_or_else(|e| panic!("no se pudo abrir: {e:?}"));

    if let Some(v) = solo_voice {
        // Máscara con todos los bits salvo el de la voz que queremos oír.
        let all = (1i32 << emu.voice_count()) - 1;
        emu.mute_voices(all & !(1 << v));
    }

    emu.start_track(0).expect("start_track");

    let total = ((SKIP_SECS + ANALYSE_SECS) * rate as f64) as usize;
    let mut buf = vec![0i16; total * 2];
    emu.play(buf.len(), &mut buf).expect("play");

    let skip = (SKIP_SECS * rate as f64) as usize;
    buf[skip * 2..]
        .chunks_exact(2)
        .map(|c| (c[0] as f64 + c[1] as f64) / 2.0)
        .collect()
}

/// Pico espectral por barrido directo. Sin FFT: buscamos un único máximo en un
/// rango conocido, y un barrido es más corto de escribir que de explicar.
fn peak_hz(samples: &[f64], rate: u32, lo: f64, hi: f64, step: f64) -> (f64, f64) {
    let mut best = (0.0f64, 0.0f64);
    let mut f = lo;
    while f <= hi {
        let w = std::f64::consts::TAU * f / rate as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in samples.iter().enumerate() {
            let a = w * i as f64;
            re += s * a.cos();
            im += s * a.sin();
        }
        let mag = (re * re + im * im).sqrt() / samples.len() as f64;
        if mag > best.1 {
            best = (f, mag);
        }
        f += step;
    }
    best
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| {
        eprintln!("uso: gme_pitch_check <fichero.vgm|.spc|.nsf> [Hz esperados]");
        std::process::exit(2);
    });
    let expected: Option<f64> = args.next().and_then(|s| s.parse().ok());

    // --- 1. ¿depende el tono de la frecuencia de salida? --------------------
    println!("consistencia entre frecuencias de salida:");
    let mut rates = Vec::new();
    for rate in [44100u32, 48000, 32000] {
        let s = render(&path, rate, None);
        let (hz, _) = peak_hz(&s, rate, 60.0, 1200.0, 0.25);
        println!("  {rate:>5} Hz  ->  pico en {hz:8.2} Hz");
        rates.push(hz);
    }
    let base = rates[0];
    let drift = rates
        .iter()
        .map(|h| ((h - base) / base * 100.0).abs())
        .fold(0.0f64, f64::max);
    println!("  desviación: {drift:.2} %\n");

    // --- 2. voz por voz -----------------------------------------------------
    let rate = 44100u32;
    let probe = GameMusicEmu::from_file(std::path::Path::new(&path), rate).unwrap();
    let voices = probe.voice_count();
    drop(probe);

    println!("tono de cada voz por separado (el resto silenciadas):");
    for v in 0..voices {
        let s = render(&path, rate, Some(v));
        let rms = (s.iter().map(|x| x * x).sum::<f64>() / s.len() as f64).sqrt();
        if rms < 20.0 {
            println!("  voz {v}: en silencio en esta ventana");
            continue;
        }

        match expected {
            // Con una nota esperada, buscamos alrededor de ella y de sus dos
            // primeros armónicos. Hace falta: una voz FM brillante suele tener
            // el pico en el segundo o el tercero, no en el fundamental, y un
            // barrido ancho encuentra el armónico de otra voz y miente.
            Some(e) => {
                let mut best = (0.0f64, 0.0f64, 1usize);
                for h in 1..=3usize {
                    let c = e * h as f64;
                    let (hz, mag) = peak_hz(&s, rate, c * 0.90, c * 1.10, 0.02);
                    if mag > best.1 {
                        best = (hz, mag, h);
                    }
                }
                let implied = best.0 / best.2 as f64;
                println!(
                    "  voz {v}: pico {:8.2} Hz (armónico {})  ->  fundamental {:7.2} Hz   {:+.2} %",
                    best.0,
                    best.2,
                    implied,
                    (implied - e) / e * 100.0
                );
            }
            None => {
                let (hz, _) = peak_hz(&s, rate, 60.0, 1200.0, 0.10);
                println!("  voz {v}: {hz:8.2} Hz");
            }
        }
    }

    if drift > 0.5 {
        println!("\nEL TONO DEPENDE DE LA FRECUENCIA DE SALIDA — fallo de remuestreo");
        std::process::exit(1);
    }
}
