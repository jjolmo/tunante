//! Talking to the `tunante-decoder` helper process.
//!
//! The emulator cores are not linked into this program. To play a track we spawn
//! the helper, read a JSON header line describing the stream, and then treat the
//! rest of its stdout as raw PCM — which is exactly what a [`rodio::Source`]
//! needs to yield. Killing the child on drop is what returns its memory, which
//! is the whole point of the arrangement: an NDS core costs ~43 MB while it
//! plays and nothing at all a moment later.

use std::io::{BufRead, BufReader, Read};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

use rodio::Source;

/// Where the helper binary lives.
///
/// `TUNANTE_DECODER` wins, then a sibling of this executable — which covers both
/// `cargo run` and an installed package, since the two binaries ship together —
/// and finally bare `tunante-decoder` for whatever `PATH` offers.
pub fn decoder_path() -> PathBuf {
    if let Ok(p) = std::env::var("TUNANTE_DECODER") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("tunante-decoder");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("tunante-decoder")
}

/// Read every track a file contains, by asking the helper.
///
/// Out of process on purpose: the emulator-backed readers can hang, and a
/// timeout cannot interrupt a loop running in C — it can only abandon the
/// thread. Here the scanner can kill the child and move on.
///
/// `fast` skips the silence detection GME uses to infer a length for tracks that
/// declare none. It decodes the track in full, so it costs over a second a file;
/// a library scan cannot afford that and does not need it.
pub fn probe(path: &Path, timeout: Duration, fast: bool) -> Result<Vec<serde_json::Value>, String> {
    let mut cmd = Command::new(decoder_path());
    cmd.arg("probe").arg(path);
    if fast {
        cmd.arg("--fast");
    }

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawning the decoder: {e}"))?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    let mut out = String::new();
    child
        .stdout
        .take()
        .ok_or("no stdout")?
        .read_to_string(&mut out)
        .map_err(|e| e.to_string())?;

    let parsed: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|e| format!("decoder said: {e}"))?;

    if parsed["ok"] != true {
        return Err(parsed["error"].as_str().unwrap_or("unknown").to_string());
    }

    Ok(parsed["tracks"].as_array().cloned().unwrap_or_default())
}

/// The cover art a file carries, as a `data:` URI, by asking the helper.
///
/// Asked for once per playing track rather than folded into `probe`, which runs
/// on every file of a library scan: carrying the artwork of thousands of tracks
/// through a pipe would slow the scan down for nobody's benefit.
pub fn artwork(path: &Path, timeout: Duration) -> Option<String> {
    let mut child = Command::new(decoder_path())
        .arg("art")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => return None,
        }
    }

    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(out.trim()).ok()?;
    parsed["art"].as_str().map(|s| s.to_string())
}

/// A decoder running in another process, seen as an audio source.
pub struct PipeSource {
    child: Child,
    reader: BufReader<ChildStdout>,
    sample_rate: NonZeroU32,
    channels: NonZeroU16,
    total: Option<Duration>,
    /// Commands go back up the pipe: `seek <ms>`, `stop`.
    control: Option<std::process::ChildStdin>,
}

impl PipeSource {
    /// Spawn the helper on `path` and read its header.
    ///
    /// `duration_hint_ms` is only consulted by GME, whose files often carry no
    /// length of their own; pass what the library database knows, or 0.
    ///
    /// `loops` and `fade_ms` decide how long a track that never ends lasts —
    /// console music mostly loops by design, so somebody has to choose. Only the
    /// backends with a notion of length honour them.
    pub fn open(
        path: &Path,
        duration_hint_ms: i64,
        loops: u32,
        fade_ms: u64,
    ) -> Result<Self, String> {
        let mut child = Command::new(decoder_path())
            .arg("play")
            .arg(path)
            .arg(duration_hint_ms.to_string())
            .arg("--loops")
            .arg(loops.max(1).to_string())
            .arg("--fade")
            .arg(fade_ms.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawning the decoder: {e}"))?;

        let control = child.stdin.take();
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut reader = BufReader::with_capacity(64 * 1024, stdout);

        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|e| format!("reading the header: {e}"))?;

        if header.trim().is_empty() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the decoder produced no header — it could not open the file".into());
        }

        let h: serde_json::Value = serde_json::from_str(header.trim())
            .map_err(|e| format!("the header was not JSON ({e}): {header:?}"))?;

        let sample_rate = h["sample_rate"]
            .as_u64()
            .and_then(|v| NonZeroU32::new(v as u32))
            .ok_or("the header carried no usable sample rate")?;
        let channels = h["channels"]
            .as_u64()
            .and_then(|v| NonZeroU16::new(v as u16))
            .ok_or("the header carried no usable channel count")?;
        let total = h["duration_ms"].as_u64().map(Duration::from_millis);

        Ok(Self { child, reader, sample_rate, channels, total, control })
    }

    /// Ask the decoder to jump.
    ///
    /// Fire and forget: the helper keeps only the newest request, so dragging a
    /// progress bar does not queue up every position the finger passed over.
    /// Samples already in the pipe still arrive, which is a few tens of
    /// milliseconds of the old position — not worth draining.
    pub fn seek(&mut self, ms: u64) {
        if let Some(stdin) = self.control.as_mut() {
            use std::io::Write;
            let _ = writeln!(stdin, "seek {ms}");
            let _ = stdin.flush();
        }
    }
}

impl Iterator for PipeSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let mut b = [0u8; 4];
        // A short read means the track ended or the helper died. Either way this
        // source is finished, and rodio moves on.
        match self.reader.read_exact(&mut b) {
            Ok(()) => Some(f32::from_ne_bytes(b)),
            Err(_) => None,
        }
    }
}

impl Source for PipeSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        self.channels
    }

    fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total
    }

    /// Seeking is a message down the pipe; the helper does the work.
    ///
    /// Every backend implements `try_seek`, so this is supported for every
    /// format — but note what "supported" means for an emulated one: the core
    /// is run forward from the start at full speed, so a jump deep into a long
    /// PSF2 is not instant. That happens in the helper, off this thread.
    ///
    /// Samples already in the pipe are not drained. A few tens of milliseconds
    /// of the old position still play, which is cheaper and less glitchy than
    /// tearing down the stream.
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.seek(pos.as_millis() as u64);
        Ok(())
    }
}

impl Drop for PipeSource {
    /// Kill the helper rather than waiting for it to finish.
    ///
    /// This is what makes the memory come back on a track change: the process
    /// dies and the kernel reclaims every page of console RAM it had touched,
    /// including the parts held in C globals that no `free` would ever return.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
