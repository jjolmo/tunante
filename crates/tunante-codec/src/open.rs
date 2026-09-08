//! Format dispatch: path in, decoder out.
//!
//! This is the single place that decides which backend plays a given file. Both
//! the desktop playback engine and the `tunante-decoder` helper process go
//! through it, so a format only ever has to be wired up once.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rodio::{Decoder, Source};
use tunante_core::vgm_path::{
    is_gme_format, is_gsf_format, is_psf2_format, is_psf_format, is_standard_format,
    is_twosf_format, is_usf_format, parse_vgm_path,
};

use crate::{
    GmeSource, GsfSource, OggOpusSource, Psf2Source, PsfSource, TwoSfSource, UsfSource,
    VgmstreamSource,
};

/// A decoder, whichever backend produced it.
pub type BoxedSource = Box<dyn Source + Send>;

/// How long a track that never ends should last.
///
/// Console music mostly loops forever by design — the hardware just kept
/// playing until the level ended. A player has to decide when to stop, and the
/// convention rips use is: play the tagged length `loop_count` times, then fade
/// out over `fade_ms`.
///
/// Only the backends with a notion of length honour this — PSF, PSF2 and GME.
/// vgmstream computes its own from the loop points in the container, and the
/// standard formats have a real end.
#[derive(Clone, Copy, Debug)]
pub struct PlaybackOptions {
    /// How many times to play the tagged length. Clamped to at least 1.
    pub loop_count: u32,
    /// Fade at the end, in milliseconds. 0 stops abruptly.
    pub fade_ms: u64,
    /// How many times a looping vgmstream stream repeats.
    ///
    /// Separate from `loop_count` because it means a different thing to a
    /// different layer: vgmstream loops on the points recorded in the container
    /// and counts them itself, while `loop_count` is this crate replaying a
    /// tagged length for backends that have no notion of an ending.
    ///
    /// It has to match what the scanner used, or the progress bar disagrees
    /// with what is heard.
    pub vgm_loop_count: f64,
}

impl Default for PlaybackOptions {
    /// Two loops and an eight-second fade: what most rips and most players
    /// assume, and what makes an untagged track come out at a listenable length.
    fn default() -> Self {
        Self {
            loop_count: 2,
            fade_ms: 8_000,
            vgm_loop_count: vgmstream_rs::Vgmstream::DEFAULT_LOOP_COUNT,
        }
    }
}

impl PlaybackOptions {
    /// The total length of a track whose tagged body is `body_ms`.
    pub fn total_ms(&self, body_ms: u64) -> u64 {
        body_ms * self.loop_count.max(1) as u64 + self.fade_ms
    }

    /// Where the fade should begin, for a body of `body_ms`.
    pub fn fade_start_ms(&self, body_ms: u64) -> u64 {
        body_ms * self.loop_count.max(1) as u64
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Decoder error: {0}")]
    Decoder(String),
}

/// Open `path` with whichever backend handles its format.
///
/// `path` may carry a `#subsong` suffix — see [`tunante_core::vgm_path`]. Files
/// that hold many songs (GME sets, vgmstream containers) use it to pick one.
///
/// `duration_hint_ms` is only consulted by GME, whose files frequently carry no
/// length of their own and need one supplied from the library database.
///
/// Order matters: the emulator formats are matched first because some of their
/// extensions would otherwise be swallowed by the vgmstream fallback, and the
/// standard formats are matched before that same fallback because vgmstream can
/// mishandle them.
pub fn open_source(path: &Path, duration_hint_ms: i64) -> Result<BoxedSource, OpenError> {
    open_source_with(path, duration_hint_ms, PlaybackOptions::default())
}

/// As [`open_source`], with control over how long looping tracks last.
pub fn open_source_with(
    path: &Path,
    duration_hint_ms: i64,
    opts: PlaybackOptions,
) -> Result<BoxedSource, OpenError> {
    let path_str = path.to_string_lossy();
    let (actual_path_str, sub_track) = parse_vgm_path(&path_str);
    let actual_path = Path::new(actual_path_str);

    let ext = actual_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let decoder_err = |e: String| OpenError::Decoder(e);

    if is_gme_format(ext) {
        // GME chiptune formats (NSF, SPC, GBS, VGM, …)
        let track_index = sub_track.unwrap_or(0);
        Ok(Box::new(
            GmeSource::new(actual_path, track_index, duration_hint_ms).map_err(decoder_err)?,
        ))
    } else if is_gsf_format(ext) {
        // GBA, via VBA-M
        Ok(Box::new(GsfSource::new(actual_path).map_err(decoder_err)?))
    } else if is_usf_format(ext) {
        // N64, via lazyusf2 / Mupen64Plus
        Ok(Box::new(UsfSource::new(actual_path).map_err(decoder_err)?))
    } else if is_twosf_format(ext) {
        // NDS, via DeSmuME
        Ok(Box::new(TwoSfSource::new(actual_path).map_err(decoder_err)?))
    } else if is_psf2_format(ext) {
        // PS2, via Highly Experimental
        Ok(Box::new(Psf2Source::with_options(actual_path, opts).map_err(decoder_err)?))
    } else if is_psf_format(ext) {
        // PS1, via sexypsf
        Ok(Box::new(PsfSource::with_options(actual_path, opts).map_err(decoder_err)?))
    } else if ext.eq_ignore_ascii_case("opus") {
        // symphonia has no Opus support, so we carry our own decoder
        let file = BufReader::new(File::open(actual_path)?);
        Ok(Box::new(OggOpusSource::new(file).map_err(decoder_err)?))
    } else if is_standard_format(ext) {
        let file = File::open(actual_path)?;
        match Decoder::try_from(file) {
            Ok(d) => Ok(Box::new(d)),
            Err(e) => {
                // An MPEG stream inside a WAV container (`fmt` tag 0x0055),
                // often behind an ID3v2 tag: how some 2003-era OC ReMix rips
                // were made. symphonia recognises neither the wrapper nor the
                // tag in front of it, but the `data` chunk is a plain MPEG
                // stream, and that it decodes.
                if let Some(payload) = riff_mpeg_payload(actual_path) {
                    let d = Decoder::new(BufReader::new(std::io::Cursor::new(payload)))
                        .map_err(|e| OpenError::Decoder(e.to_string()))?;
                    return Ok(Box::new(d));
                }
                Err(OpenError::Decoder(e.to_string()))
            }
        }
    } else {
        // vgmstream covers 700+ containers (BCSTM, ADX, HCA, …). If it declines,
        // symphonia gets a last look — some files carry an unexpected extension.
        let subsong = sub_track.map(|s| s as i32).unwrap_or(0);
        match VgmstreamSource::new(actual_path, subsong, opts.vgm_loop_count) {
            Ok(source) => Ok(Box::new(source)),
            Err(_) => {
                let file = File::open(actual_path)?;
                Ok(Box::new(
                    Decoder::try_from(file).map_err(|e| OpenError::Decoder(e.to_string()))?,
                ))
            }
        }
    }
}

/// The MPEG frames inside a RIFF/WAVE file whose format tag says MPEG Layer 3
/// (0x0055) or MPEG (0x0050), with any ID3v2 tag in front of the RIFF header
/// skipped. `None` for anything else, so the caller's own error stands.
fn riff_mpeg_payload(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    let mut at = 0usize;
    // ID3v2: "ID3", version (2), flags (1), synchsafe size (4); a footer flag
    // adds ten more bytes after the tag.
    if bytes.len() > 10 && &bytes[..3] == b"ID3" {
        let size = bytes[6..10]
            .iter()
            .fold(0usize, |acc, b| (acc << 7) | (*b & 0x7f) as usize);
        at = 10 + size + if bytes[5] & 0x10 != 0 { 10 } else { 0 };
    }
    let riff = bytes.get(at..at + 12)?;
    if &riff[..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = at + 12;
    let mut is_mpeg = false;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            let tag = u16::from_le_bytes(bytes.get(body..body + 2)?.try_into().ok()?);
            is_mpeg = matches!(tag, 0x0055 | 0x0050);
        } else if id == b"data" {
            if !is_mpeg {
                return None;
            }
            let end = (body + len).min(bytes.len());
            return Some(bytes[body..end].to_vec());
        }
        pos = body + len + (len & 1);
    }
    None
}
