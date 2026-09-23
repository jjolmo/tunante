//! KSS (MSX), via the vendored libkss.
//!
//! GME claims this format and compiles a backend for it, and that backend plays
//! silence: it emulates six hand-written bytes of MSX BIOS, and a real Konami
//! driver expects a real MSX. `vendor/libkss/README.upstream.md` has the
//! measurements. Everything else GME handles still goes to GME.
//!
//! # Which song is `#3`?
//!
//! A KSS holds up to 256 songs numbered the way the sound driver numbers them —
//! Metal Gear 2 uses `$99`, `$9A`, `$6A` — and those numbers are not positions.
//! The library stores `file.kss#N` where N is a position, so N has to be mapped
//! back through the same `.m3u` the scanner read: entry N names the song. With
//! no playlist beside the file there is nothing to map through, and N is the
//! song number itself.

use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::time::Duration;

use libkss_rs::KssPlayer;

const SAMPLE_RATE: u32 = 44100;
const CHANNELS: u16 = 2;
/// Fallback play time for a song with no declared length.
const DEFAULT_DURATION_MS: i64 = 150_000;
/// Fade appended after the play portion, matching the GME backend.
const FADE_MS: i64 = 10_000;
/// Decoded at a time, in interleaved samples.
const CHUNK: usize = 2048;
/// Output gain, in decibels.
///
/// libkss mixes for a machine that could have five sound chips going at once,
/// so one PSG on its own lands far below everything else in a library. Measured
/// over 301 real MSX tracks: rms 0.023, against 0.137 for the SNES rips filed
/// next to them.
///
/// The ceiling is the crest factor, not the average. Chip music is square
/// waves and peaks around fifteen times its own rms, so matching that average
/// would need +15 dB and would clip every attack. +8 dB is what fits: it takes
/// the loudest peak in those 301 tracks from 0.336 to 0.843 — still short of
/// the rail — and lifts everything else two and a half times with it.
///
/// GME does the same for the formats it does play, and for the same reason:
/// `Kss_Emu::update_gain` multiplies by 1.4, and by 2.1 once a track has
/// touched the SCC.
const GAIN_DB: f32 = 8.0;

/// The song number that position `index` refers to, read from the companion
/// playlist. See the module note.
pub(crate) fn song_number_for(path: &Path, index: usize) -> u32 {
    let m3u = crate::metadata::gme_reader::parse_gme_m3u(&path.with_extension("m3u"))
        .or_else(|| {
            let mut name = path.file_name()?.to_os_string();
            name.push(".m3u");
            crate::metadata::gme_reader::parse_gme_m3u(&path.with_file_name(name))
        });
    match m3u.as_ref().and_then(|m| m.entries.get(index)) {
        Some(entry) if entry.track >= 0 => entry.track as u32,
        _ => index as u32,
    }
}

pub struct KssSource {
    player: KssPlayer,
    buffer: Vec<f32>,
    buf_pos: usize,
    /// Frames emitted so far, for deciding when the fade starts.
    frames_out: u64,
    fade_at_frame: u64,
    fading: bool,
    total_duration: Option<Duration>,
    finished: bool,
}

impl KssSource {
    /// - `path`: the `.kss` itself, with no `#subsong` suffix
    /// - `index`: the position stored in the library path
    /// - `duration_hint_ms`: what the database holds, fade included
    pub fn new(path: &Path, index: usize, duration_hint_ms: i64) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("KSS read error: {e}"))?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string());
        let song = song_number_for(path, index);

        let mut player = KssPlayer::open(
            &data,
            name.as_deref().unwrap_or("track.kss"),
            SAMPLE_RATE,
            CHANNELS,
            song,
        )
        .map_err(|e| format!("KSS load error: {e}"))?;

        // A song that simply stops should stop, rather than run to whatever
        // length the playlist claimed. Generous on purpose: real MSX tracks
        // hold rests of a second or more, and cutting one short is worse than
        // trailing a little silence.
        player.set_silent_limit(5_000);
        player.set_gain_db(GAIN_DB);

        // The stored duration already includes the fade, the way the GME
        // backend reports it, so the play portion is what is left of it.
        let play_ms = if duration_hint_ms > FADE_MS {
            duration_hint_ms - FADE_MS
        } else {
            DEFAULT_DURATION_MS
        };

        Ok(Self {
            player,
            buffer: Vec::new(),
            buf_pos: 0,
            frames_out: 0,
            fade_at_frame: (play_ms as u64 * SAMPLE_RATE as u64) / 1000,
            fading: false,
            total_duration: Some(Duration::from_millis((play_ms + FADE_MS) as u64)),
            finished: false,
        })
    }

    fn decode_next_chunk(&mut self) -> bool {
        if self.player.ended() {
            self.finished = true;
            return false;
        }
        if !self.fading && self.frames_out >= self.fade_at_frame {
            self.player.fade_start(FADE_MS as u32);
            self.fading = true;
        }

        let mut pcm = vec![0i16; CHUNK];
        self.player.render(&mut pcm);
        self.frames_out += (CHUNK / CHANNELS as usize) as u64;

        self.buffer.clear();
        self.buffer.extend(pcm.iter().map(|&s| s as f32 / 32768.0));
        self.buf_pos = 0;

        if self.player.ended() {
            self.finished = true;
        }
        true
    }
}

impl Iterator for KssSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.finished && self.buf_pos >= self.buffer.len() {
            return None;
        }
        if self.buf_pos >= self.buffer.len() && !self.decode_next_chunk() {
            return None;
        }
        let sample = self.buffer[self.buf_pos];
        self.buf_pos += 1;
        Some(sample)
    }
}

impl Source for KssSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.buf_pos < self.buffer.len() {
            Some(self.buffer.len() - self.buf_pos)
        } else {
            None
        }
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(CHANNELS).unwrap()
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, _pos: Duration) -> Result<(), SeekError> {
        // libkss has no seek: reaching a position means emulating up to it, and
        // this backend would have to restart the song and render in silence to
        // get there. Refused rather than faked — the player treats an
        // unseekable source as unseekable, which is the truth here.
        Err(SeekError::NotSupported {
            underlying_source: "KSS",
        })
    }
}
