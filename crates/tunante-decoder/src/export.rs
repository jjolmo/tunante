//! `tunante-decoder export`: a track, whatever its format, as an MP3 file.
//!
//! Everything this binary plays comes out of `open_source_with` as PCM — an SPC
//! as much as a FLAC — so converting is that same stream handed to an encoder
//! instead of the speakers, with the same loop count and fade the player uses:
//! the file lasts exactly what the track sounds like in the app.
//!
//! LAME does the encoding (LGPL, built from source by `mp3lame-sys`). The tags
//! are written afterwards with lofty rather than by LAME, whose ID3 writer takes
//! Latin-1 and would mangle every Japanese title in a VGM library.

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use mp3lame_encoder::{Builder, FlushNoGap, InterleavedPcm, MonoPcm, Quality, VbrMode};
use rodio::Source;

/// What goes into the file's ID3 tag. Handed down by the caller, which has the
/// library's view of the track (a chiptune's own tags are often empty and the
/// database's title came from the folder or the playlist).
#[derive(Default)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
}

/// Samples per encoder call. Also how often progress is considered — a line is
/// only printed when the whole percent changes.
const CHUNK_FRAMES: usize = 4096;

pub fn export(
    path: &str,
    out: &Path,
    duration_hint_ms: i64,
    opts: tunante_codec::PlaybackOptions,
    tags: Tags,
) -> Result<(), String> {
    let mut source = tunante_codec::open_source_with(Path::new(path), duration_hint_ms, opts)
        .map_err(|e| e.to_string())?;
    let rate = source.sample_rate().get();
    let channels = source.channels().get() as usize;
    let total_frames = source
        .total_duration()
        .map(|d| (d.as_secs_f64() * rate as f64) as u64)
        .filter(|&n| n > 0);

    // Mono stays mono; anything wider than stereo is folded down to it, which
    // LAME cannot take and nobody expects from a VGM rip anyway.
    let out_channels: u8 = if channels == 1 { 1 } else { 2 };
    let mut builder = Builder::new().ok_or("the MP3 encoder could not start")?;
    builder.set_num_channels(out_channels).map_err(|e| format!("encoder: {e:?}"))?;
    builder.set_sample_rate(rate).map_err(|e| format!("encoder: {e:?}"))?;
    // V0: the best VBR setting, transparent for anything this library holds.
    builder.set_vbr_mode(VbrMode::Mtrh).map_err(|e| format!("encoder: {e:?}"))?;
    builder.set_vbr_quality(Quality::Best).map_err(|e| format!("encoder: {e:?}"))?;
    builder.set_quality(Quality::NearBest).map_err(|e| format!("encoder: {e:?}"))?;
    builder.set_to_write_vbr_tag(true).map_err(|e| format!("encoder: {e:?}"))?;
    let mut encoder = builder.build().map_err(|e| format!("encoder: {e:?}"))?;

    // Written beside the target and renamed at the end: a cancelled or failed
    // export never leaves a half file under the name the user chose.
    let part = part_path(out);
    let result = encode(&mut source, &mut encoder, channels, out_channels, total_frames, &part);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }

    if let Err(e) = tag(&part, path, &tags) {
        // Untagged is still the music; say so, but keep the file.
        eprintln!("tunante-decoder: tagging the MP3: {e}");
    }
    std::fs::rename(&part, out).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("saving {}: {e}", out.display())
    })?;
    print_line(&serde_json::json!({ "ok": true }));
    Ok(())
}

/// Where the file is written until it is complete.
pub fn part_path(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    out.with_file_name(name)
}

fn encode(
    source: &mut (impl Source<Item = f32> + ?Sized),
    encoder: &mut mp3lame_encoder::Encoder,
    channels: usize,
    out_channels: u8,
    total_frames: Option<u64>,
    part: &Path,
) -> Result<(), String> {
    let mut file = File::create(part).map_err(|e| format!("creating {}: {e}", part.display()))?;
    let mut pcm: Vec<f32> = Vec::with_capacity(CHUNK_FRAMES * out_channels as usize);
    let mut mp3: Vec<u8> = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(CHUNK_FRAMES));
    let mut done: u64 = 0;
    let mut last_percent = -1i64;
    let mut frame = vec![0f32; channels];

    let mut ended = false;
    while !ended {
        pcm.clear();
        let mut frames = 0usize;
        while frames < CHUNK_FRAMES {
            let mut got = 0;
            for s in frame.iter_mut() {
                match source.next() {
                    Some(v) => {
                        *s = v;
                        got += 1;
                    }
                    None => break,
                }
            }
            if got < channels {
                ended = true;
                break;
            }
            match out_channels {
                1 => pcm.push(frame[0]),
                _ if channels == 2 => pcm.extend_from_slice(&frame),
                _ => {
                    let (l, r) = downmix(&frame);
                    pcm.push(l);
                    pcm.push(r);
                }
            }
            frames += 1;
        }
        if frames == 0 {
            break;
        }
        done += frames as u64;

        mp3.clear();
        let written = if out_channels == 1 {
            encoder.encode_to_vec(MonoPcm(&pcm[..]), &mut mp3)
        } else {
            encoder.encode_to_vec(InterleavedPcm(&pcm[..]), &mut mp3)
        }
        .map_err(|e| format!("encoding: {e:?}"))?;
        file.write_all(&mp3[..written]).map_err(|e| format!("writing: {e}"))?;

        if let Some(total) = total_frames {
            let percent = (done * 100 / total.max(1)).min(99) as i64;
            if percent != last_percent {
                last_percent = percent;
                print_line(&serde_json::json!({ "progress": percent as f64 / 100.0 }));
            }
        }
    }

    mp3.clear();
    mp3.reserve(7200);
    let written = encoder
        .flush_to_vec::<FlushNoGap>(&mut mp3)
        .map_err(|e| format!("encoding: {e:?}"))?;
    file.write_all(&mp3[..written]).map_err(|e| format!("writing: {e}"))?;

    // The Xing/LAME frame LAME left room for at the start: without it a player
    // guesses a VBR file's length from its first frame and gets it wrong.
    let mut lametag = Vec::with_capacity(encoder.lame_tag_size().max(1));
    if encoder.lame_tag_encode_to_vec(&mut lametag).is_some() {
        file.seek(SeekFrom::Start(0)).map_err(|e| format!("writing: {e}"))?;
        file.write_all(&lametag).map_err(|e| format!("writing: {e}"))?;
    }
    file.flush().map_err(|e| format!("writing: {e}"))?;
    Ok(())
}

/// More than two channels folded into two: the first pair carries front left
/// and right in every layout rodio produces, and the rest are mixed into both.
fn downmix(frame: &[f32]) -> (f32, f32) {
    let rest: f32 = frame[2..].iter().sum::<f32>() / (frame.len() - 2) as f32;
    ((frame[0] + rest * 0.5).clamp(-1.0, 1.0), (frame[1] + rest * 0.5).clamp(-1.0, 1.0))
}

/// Title, artist, album artist, album, track and disc number, and the cover
/// the player shows for the source (embedded, or the folder's image).
fn tag(mp3: &Path, source_path: &str, tags: &Tags) -> Result<(), String> {
    let real = tunante_codec::metadata::real_path_of(source_path);
    let cover = tunante_codec::metadata::extract_artwork_base64(Path::new(real))
        .ok()
        .flatten()
        .and_then(|uri| decode_data_uri(&uri));
    tunante_codec::metadata::write_export_tags(
        mp3,
        &tunante_codec::metadata::ExportTags {
            title: &tags.title,
            artist: &tags.artist,
            album_artist: &tags.album_artist,
            album: &tags.album,
            track: tags.track,
            disc: tags.disc,
            cover: cover.as_ref().map(|(bytes, mime)| (bytes.as_slice(), mime.as_str())),
        },
    )
}

fn decode_data_uri(uri: &str) -> Option<(Vec<u8>, String)> {
    let (head, b64) = uri.strip_prefix("data:")?.split_once(',')?;
    let mime = head.split(';').next().unwrap_or("image/jpeg").to_string();
    Some((tunante_codec::metadata::decode_base64(b64)?, mime))
}

fn print_line(v: &serde_json::Value) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}
