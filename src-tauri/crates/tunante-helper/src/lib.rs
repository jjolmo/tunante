//! Talking to the `tunante-decoder` helper process.
//!
//! The emulator cores are not linked into the caller. To play a track we spawn
//! the helper, read a JSON header line describing the stream, and then treat the
//! rest of its stdout as raw PCM — which is exactly what a [`rodio::Source`]
//! needs to yield. Killing the child on drop is what returns its memory, which
//! is the whole point of the arrangement: an NDS core costs ~43 MB while it
//! plays and nothing at all a moment later.
//!
//! Shared by `tunante-mini` and `tunante-android`, which need the same client
//! but find the helper in very different places — see [`set_decoder_path`].

use std::io::{BufRead, BufReader, Read};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use rodio::Source;

pub mod art;
pub mod scan;

static DECODER: OnceLock<PathBuf> = OnceLock::new();

/// Name the helper explicitly, once, before anything else calls in.
///
/// Android has to: `current_exe()` there is `/system/bin/app_process64`, an app
/// has no useful `PATH`, and the helper lives in `nativeLibraryDir` under a name
/// it does not choose — `lib*.so`, because only files matching that are unpacked
/// into the one directory an app is allowed to `execve` from.
///
/// Returns whether it was accepted; a later call is ignored rather than swapping
/// the decoder under a playing track.
pub fn set_decoder_path(path: impl Into<PathBuf>) -> bool {
    DECODER.set(path.into()).is_ok()
}

/// A `Command` for the helper, with whatever the platform needs around it.
fn decoder_command() -> Command {
    let path = decoder_path();

    // Android only, and not optional there: without it the child dies before
    // main() with
    //
    //     CANNOT LINK EXECUTABLE: library "libc++_shared.so" not found
    //
    // even though libc++_shared.so sits in the very same directory. The app's
    // own libraries load because ART builds a linker namespace for us with
    // nativeLibraryDir in it; a process started with execve inherits none of
    // that, only the environment — and the environment does not name it.
    //
    // Deliberately not done elsewhere: prepending a directory to the library
    // search path on a desktop could shadow a system library for no reason.
    #[cfg(target_os = "android")]
    {
        let mut cmd = Command::new(&path);
        if let Some(dir) = path.parent() {
            cmd.env("LD_LIBRARY_PATH", dir);
        }
        return cmd;
    }
    #[cfg(not(target_os = "android"))]
    Command::new(&path)
}

/// Where the helper binary lives.
///
/// An explicit [`set_decoder_path`] wins, then `TUNANTE_DECODER`, then a sibling
/// of this executable — which covers both `cargo run` and an installed package,
/// since the two binaries ship together — and finally bare `tunante-decoder` for
/// whatever `PATH` offers.
pub fn decoder_path() -> PathBuf {
    if let Some(p) = DECODER.get() {
        return p.clone();
    }
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
/// Run a helper to completion and hand back everything it printed.
///
/// The pipe has to be drained while the child is still running. A pipe holds
/// 64 KB; write more than that with nobody reading and the writer blocks, so it
/// never exits, so a wait-then-read caller sits until its own deadline and
/// kills a child that was doing what it was told.
///
/// The failure is size-dependent, which is what makes it nasty: everything that
/// fits in the buffer works, and only the big answers vanish. It cost an
/// afternoon here — folder covers of a megabyte or two came back empty and
/// looked like files that simply had no artwork.
fn capture(mut child: Child, timeout: Duration) -> Result<String, String> {
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let mut stdout = stdout;
        let _ = stdout.read_to_string(&mut out);
        out
    });

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() > deadline => {
                // Killing it also ends the reader: its end of the pipe closes.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                let _ = child.kill();
                return Err(e.to_string());
            }
        }
    }

    reader.join().map_err(|_| "the reader thread panicked".to_string())
}

/// Out of process on purpose: the emulator-backed readers can hang, and a
/// timeout cannot interrupt a loop running in C — it can only abandon the
/// thread. Here the scanner can kill the child and move on.
///
/// `fast` skips the silence detection GME uses to infer a length for tracks that
/// declare none. It decodes the track in full, so it costs over a second a file;
/// a library scan cannot afford that and does not need it.
pub fn probe(path: &Path, timeout: Duration, fast: bool) -> Result<Vec<serde_json::Value>, String> {
    let mut cmd = decoder_command();
    cmd.arg("probe").arg(path);
    if fast {
        cmd.arg("--fast");
    }

    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawning the decoder: {e}"))?;

    let out = capture(child, timeout)?;

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
    let child = decoder_command()
        .arg("art")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let out = capture(child, timeout).ok()?;
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
        let mut child = decoder_command()
            .arg("play")
            .arg(path)
            .arg(duration_hint_ms.to_string())
            .arg("--loops")
            .arg(loops.max(1).to_string())
            .arg("--fade")
            .arg(fade_ms.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // This used to go to /dev/null, which on a desktop is reasonable —
            // the helper's stderr lands in the terminal. On Android a child's
            // stderr goes nowhere at all, so a decoder that dies on startup is
            // completely silent and looks exactly like a track that ended. It
            // is drained to the log instead, and on a thread: a pipe nobody
            // reads fills at 64 KB and would block the decoder mid-track.
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawning the decoder: {e}"))?;

        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::warn!("decoder: {line}");
                }
            });
        }

        let control = child.stdin.take();
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut reader = BufReader::with_capacity(64 * 1024, stdout);

        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|e| format!("reading the header: {e}"))?;

        if header.trim().is_empty() {
            let _ = child.kill();
            let status = child.wait();
            // The exit status is the only other thing the child left behind, and
            // it separates "refused the file" from "died on a signal". The rest
            // is in the `decoder:` lines the thread above logged.
            return Err(format!(
                "the decoder produced no header for {} (exit: {status:?})",
                path.display()
            ));
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
    /// A finite span, and it has to be finite even though ours never changes.
    ///
    /// `None` reads as "these parameters are eternal", and rodio takes it at its
    /// word: `UniformSourceIterator::bootstrap` builds the resampler from
    /// `sample_rate()` once, wraps the source in a `Take` with no limit, and so
    /// never runs it out and never rebuilds. The rate captured at that moment is
    /// then applied to every track that follows, whatever its own rate is.
    ///
    /// PSF2 is the only format here that is not 44100 — the PS2's SPU2 runs at
    /// 48000 — so it was the only one that came out wrong, playing at
    /// 44100/48000 of its speed: 8.8% slow and a sixth of a semitone flat.
    ///
    /// Any finite answer fixes it, and rodio caps whatever it gets at 32768
    /// anyway, so it rebuilds on that period for every ordinary source too.
    /// This is the normal path, not a workaround.
    fn current_span_len(&self) -> Option<usize> {
        Some(32768)
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

#[cfg(test)]
mod tests {
    use rodio::source::UniformSourceIterator;
    use rodio::Source;
    use std::num::{NonZeroU16, NonZeroU32};
    use std::time::Duration;

    /// A source of a known rate that answers `current_span_len` however we ask.
    struct Tone {
        left: usize,
        rate: u32,
        span: Option<usize>,
    }

    impl Iterator for Tone {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.left == 0 {
                return None;
            }
            self.left -= 1;
            Some(0.0)
        }
    }

    impl Source for Tone {
        fn current_span_len(&self) -> Option<usize> {
            self.span
        }
        fn channels(&self) -> NonZeroU16 {
            NonZeroU16::new(2).unwrap()
        }
        fn sample_rate(&self) -> NonZeroU32 {
            NonZeroU32::new(self.rate).unwrap()
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    fn resampled(span: Option<usize>) -> usize {
        // One second of 48 kHz stereo, asked to come out at 44100.
        let tone = Tone { left: 48000 * 2, rate: 48000, span };
        UniformSourceIterator::new(
            tone,
            NonZeroU16::new(2).unwrap(),
            NonZeroU32::new(44100).unwrap(),
        )
        .count()
    }

    /// The bug that made every PS2 track play 8.8% slow.
    ///
    /// `None` means "my parameters never change", and rodio acts on it:
    /// `bootstrap` builds the resampler once, wraps the source in a `Take` with
    /// no limit, and so never runs it out and never rebuilds. One second of
    /// 48 kHz then comes out as 48000 frames at 44100, which takes 1.088
    /// seconds to play.
    ///
    /// This test does not assert the broken number, only that the two answers
    /// differ — if a future rodio makes `None` behave, the fix stops being
    /// needed rather than starting to fail.
    #[test]
    fn an_endless_span_defeats_the_resampler() {
        let sin_limite = resampled(None);
        let con_limite = resampled(Some(32768));
        assert_ne!(
            sin_limite, con_limite,
            "si estos coinciden, rodio ya no necesita un span finito"
        );
    }

    /// One second in has to be one second out, whatever the rates.
    #[test]
    fn a_finite_span_resamples_to_the_target_rate() {
        let frames = resampled(Some(32768)) / 2;
        let error = (frames as f64 - 44100.0).abs() / 44100.0;
        assert!(
            error < 0.001,
            "44100 esperados, {frames} obtenidos ({:.2}% de error)",
            error * 100.0
        );
    }
}
