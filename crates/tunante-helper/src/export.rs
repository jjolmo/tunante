//! Converting a track to MP3 through `tunante-decoder export`.
//!
//! The encoder is a codec, so it lives with the others on the far side of the
//! pipe; this only starts it, relays its progress and, to cancel, kills it —
//! the same way a skipped track's decoder goes.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// One conversion: the track as the player would play it, and its tags.
#[derive(Clone, Debug, Default)]
pub struct ExportRequest {
    /// The track's path, `#n` subsong suffix included.
    pub path: String,
    pub out: PathBuf,
    /// What the library knows of the length; only GME consults it.
    pub duration_hint_ms: i64,
    /// The player's own loop count, fade and vgmstream loops, so the file
    /// lasts what the track lasts in the app.
    pub loops: u32,
    pub fade_ms: u64,
    pub vgm_loop_count: Option<f64>,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
}

/// How a conversion ended when it did not succeed.
#[derive(Debug, PartialEq)]
pub enum ExportError {
    /// `cancel` was raised; the half-written file is already gone.
    Cancelled,
    Failed(String),
}

/// Run the conversion to the end, or until `cancel` is raised.
///
/// `on_progress` hears a fraction from 0 to 1 whenever the encoder moves on a
/// whole percent; a track whose length nobody knows reports none, and the
/// caller shows it as indeterminate.
pub fn export_mp3(
    req: &ExportRequest,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f32),
) -> Result<(), ExportError> {
    let mut cmd = crate::decoder_command();
    cmd.arg("export")
        .arg(&req.path)
        .arg(&req.out)
        .arg(req.duration_hint_ms.to_string())
        .arg("--loops")
        .arg(req.loops.max(1).to_string())
        .arg("--fade")
        .arg(req.fade_ms.to_string());
    if let Some(v) = req.vgm_loop_count {
        cmd.arg("--vgm-loops").arg(v.to_string());
    }
    for (flag, value) in [
        ("--title", &req.title),
        ("--artist", &req.artist),
        ("--album-artist", &req.album_artist),
        ("--album", &req.album),
    ] {
        if !value.is_empty() {
            cmd.arg(flag).arg(value);
        }
    }
    for (flag, value) in [("--track", req.track), ("--disc", req.disc)] {
        if let Some(n) = value {
            cmd.arg(flag).arg(n.to_string());
        }
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ExportError::Failed(format!("spawning the decoder: {e}")))?;

    // Lines arrive on their own thread so cancelling does not wait for the
    // next one: a track of unknown length prints nothing until it is done.
    let stdout = child.stdout.take().ok_or(ExportError::Failed("no stdout".into()))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut outcome: Option<Result<(), String>> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(part_path(&req.out));
            return Err(ExportError::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                if let Some(p) = v["progress"].as_f64() {
                    on_progress(p as f32);
                } else if v["ok"] == true {
                    outcome = Some(Ok(()));
                } else if v["ok"] == false {
                    outcome = Some(Err(v["error"].as_str().unwrap_or("unknown error").to_string()));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().map_err(|e| ExportError::Failed(e.to_string()))?;
    match outcome {
        Some(Ok(())) if status.success() => {
            on_progress(1.0);
            Ok(())
        }
        Some(Err(e)) => Err(ExportError::Failed(e)),
        _ => {
            let _ = std::fs::remove_file(part_path(&req.out));
            Err(ExportError::Failed(format!("the decoder stopped ({status})")))
        }
    }
}

/// Where the decoder writes until the file is complete. Must match
/// `tunante-decoder`'s own `export::part_path`.
fn part_path(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    out.with_file_name(name)
}
