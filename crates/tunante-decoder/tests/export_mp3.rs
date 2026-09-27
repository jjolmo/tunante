//! The export subcommand, end to end: convert real fixtures — an emulated
//! format and an ordinary one — and read the MP3 back through the decoder.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tunante-codec/tests/fixtures")
        .join(name)
}

fn out_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tunante-export-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn decoder() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tunante-decoder"))
}

/// Export `source` and return the MP3's own probe: its tracks as JSON.
fn export_and_probe(source: &str, tag: &str, title: &str) -> (PathBuf, serde_json::Value) {
    let out = out_dir(tag).join(format!("{tag}.mp3"));
    let run = decoder()
        .args(["export", source, &out.to_string_lossy(), "0", "--loops", "1", "--fade", "1000"])
        .args(["--title", title, "--artist", "Tunante", "--track", "3"])
        .args(["--album", "Juego", "--album-artist", "Compositora", "--disc", "2"])
        .output()
        .unwrap();
    assert!(run.status.success(), "export failed: {run:?}");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let last: serde_json::Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(last["ok"], true, "{stdout}");
    assert!(out.exists(), "no MP3 at {}", out.display());
    let mut part = out.clone().into_os_string();
    part.push(".part");
    assert!(!Path::new(&part).exists(), "the .part was left behind");

    let probe = decoder().args(["probe", &out.to_string_lossy()]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    (out, v)
}

/// An emulated format — NSF, through GME — comes out as a playable MP3 with
/// every tag it was given, a Japanese title included.
#[test]
fn an_nsf_subsong_becomes_a_tagged_mp3() {
    let src = format!("{}#0", fixture("sample.nsf").display());
    let (_, v) = export_and_probe(&src, "nsf", "ランニング");
    let t = &v["tracks"][0];
    assert_eq!(t["title"], "ランニング");
    assert_eq!(t["artist"], "Tunante");
    assert_eq!(t["album"], "Juego");
    assert_eq!(t["album_artist"], "Compositora");
    assert_eq!(t["track_number"], 3);
    assert_eq!(t["disc_number"], 2);
    assert_eq!(t["has_artwork"], false, "the fixture folder has no cover to carry");
    assert_eq!(t["codec"].as_str().map(str::to_lowercase).as_deref(), Some("mp3"));
    assert!(t["duration_ms"].as_i64().unwrap_or(0) > 500, "{t}");
}

/// An ordinary file keeps its length through the conversion.
#[test]
fn a_flac_keeps_its_length() {
    let (_, v) = export_and_probe(&fixture("sine.flac").to_string_lossy(), "flac", "Sine");
    let flac = decoder().args(["probe", &fixture("sine.flac").to_string_lossy()]).output().unwrap();
    let flac: serde_json::Value = serde_json::from_slice(&flac.stdout).unwrap();
    let (a, b) = (
        flac["tracks"][0]["duration_ms"].as_i64().unwrap(),
        v["tracks"][0]["duration_ms"].as_i64().unwrap(),
    );
    // An MP3 pads to whole frames: within a tenth of a second, not exact.
    assert!((a - b).abs() < 100, "flac {a} ms, mp3 {b} ms");
}

/// A file that cannot be read fails with a reason and leaves nothing behind.
#[test]
fn a_missing_file_fails_cleanly() {
    let dir = out_dir("missing");
    let out = dir.join("x.mp3");
    let run = decoder()
        .args(["export", "/nonexistent/track.flac", &out.to_string_lossy()])
        .output()
        .unwrap();
    assert!(!run.status.success());
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&run.stdout).lines().last().unwrap()).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "something was left in {}", dir.display());
}

/// The N64 core runs on a thread of its own and is the slowest there is; read
/// faster than real time — as a conversion always is — it used to hand out
/// silence whenever it fell behind, and the MP3 came out full of stops (537
/// gaps, 89 s of silence in a 125 s Banjo-Kazooie track). No gaps now.
#[test]
fn an_n64_track_converts_without_gaps() {
    let src = fixture("usf/sample.miniusf");
    let out = out_dir("usf").join("usf.mp3");
    let run = decoder()
        .args(["export", &src.to_string_lossy(), &out.to_string_lossy(), "0", "--loops", "1", "--fade", "0"])
        .output()
        .unwrap();
    assert!(run.status.success(), "export failed: {run:?}");

    // Read the MP3 back as PCM through the decoder's own `play`.
    let play = decoder().args(["play", &out.to_string_lossy()]).output().unwrap();
    let body = &play.stdout[play.stdout.iter().position(|&b| b == b'\n').unwrap() + 1..];
    let samples: Vec<f32> = body
        .chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let rate = 44_100 * 2;
    assert!(samples.len() > rate * 2, "too short to judge: {} samples", samples.len());

    // Runs of silence longer than 20 ms, away from the start and the end.
    let middle = &samples[rate..samples.len() - rate];
    let (mut gaps, mut run_len) = (0, 0);
    for s in middle {
        if s.abs() < 1e-4 {
            run_len += 1;
        } else {
            if run_len > rate / 50 {
                gaps += 1;
            }
            run_len = 0;
        }
    }
    assert_eq!(gaps, 0, "the N64 track has {gaps} gaps of silence");
}
