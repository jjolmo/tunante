use crate::audio::vgm_path::build_vgm_path;
use crate::db::models::Track;
use game_music_emu::GameMusicEmu;
use std::path::Path;
use uuid::Uuid;

/// Default play duration for tracks with unknown length in fast scan mode (2.5 minutes)
const DEFAULT_DURATION_MS: i64 = 150_000;
/// Fade duration appended after play_length
const FADE_MS: i64 = 10_000;
/// Maximum time to emulate when detecting duration by silence (5 minutes)
const MAX_DETECT_DURATION_MS: i32 = 300_000;
/// Chunk size for silence detection (stereo samples per iteration)
const DETECT_CHUNK_SAMPLES: usize = 2048;

/// Detect the actual play duration of a GME track by emulating until silence.
/// Returns the duration in milliseconds, or None if the track loops past the max limit.
fn detect_duration_by_silence(path: &Path, track_index: usize) -> Option<i64> {
    let emu = GameMusicEmu::from_file(path, 44100).ok()?;
    // Set a generous fade so track_ended() triggers on actual silence, not artificial cutoff
    emu.start_track(track_index).ok()?;
    emu.set_fade(MAX_DETECT_DURATION_MS);

    let mut buf = vec![0i16; DETECT_CHUNK_SAMPLES];
    loop {
        if emu.track_ended() {
            let ms = emu.tell() as i64;
            return Some(ms);
        }
        if emu.tell() as i32 >= MAX_DETECT_DURATION_MS {
            return None; // Loops forever, use default
        }
        if emu.play(DETECT_CHUNK_SAMPLES, &mut buf).is_err() {
            return None;
        }
    }
}

/// Read all sub-tracks from a GME file, returning one Track per sub-track.
/// For single-track files (e.g., most SPC files), returns a single Track without #N suffix.
/// For multi-track files (e.g., NSF with 30 tracks), returns one Track per sub-track with #N suffix.
///
/// When `fast_scan` is false, tracks without a known duration are emulated to detect
/// their actual length via silence detection. This is slower but gives accurate durations.
pub fn read_gme_metadata(path: &Path) -> Result<Vec<Track>, String> {
    read_gme_metadata_inner(path, false)
}

/// Same as read_gme_metadata but allows controlling fast_scan mode.
pub fn read_gme_metadata_with_opts(path: &Path, fast_scan: bool) -> Result<Vec<Track>, String> {
    read_gme_metadata_inner(path, fast_scan)
}

fn read_gme_metadata_inner(path: &Path, fast_scan: bool) -> Result<Vec<Track>, String> {
    let emu = GameMusicEmu::from_file(path, 44100)
        .map_err(|e| format!("GME error: {}", e))?;

    let track_count = emu.track_count();
    let file_meta = std::fs::metadata(path)
        .map_err(|e| format!("IO error: {}", e))?;

    let file_path_str = path.to_string_lossy().to_string();
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let codec = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_uppercase())
        .unwrap_or_default();

    let modified_at = file_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let file_size = file_meta.len() as i64;

    let mut tracks = Vec::with_capacity(track_count);

    for i in 0..track_count {
        let info = match emu.track_info(i) {
            Ok(info) => info,
            Err(e) => {
                log::warn!("GME track info error for track {}: {}", i, e);
                continue;
            }
        };

        // Title: use song name, or generate from filename
        let title = if !info.song.is_empty() {
            info.song.clone()
        } else if track_count == 1 {
            file_name.clone()
        } else {
            format!("{} - Track {}", file_name, i + 1)
        };

        // Duration: play_length already includes GME's logic
        // (length if available, else intro+loop*2, else default 150s)
        let duration_ms = if info.play_length > 0 {
            info.play_length as i64 + FADE_MS
        } else if !fast_scan {
            // No known duration — emulate to detect actual length via silence
            if let Some(detected) = detect_duration_by_silence(path, i) {
                detected
            } else {
                DEFAULT_DURATION_MS + FADE_MS
            }
        } else {
            DEFAULT_DURATION_MS + FADE_MS
        };

        // Virtual path: only add #N for multi-track files
        let virtual_path = if track_count == 1 {
            file_path_str.clone()
        } else {
            build_vgm_path(&file_path_str, i)
        };

        // Map GME game → album (so tracks from the same game group together)
        let game = info.game.clone();
        let album = if game.is_empty() {
            file_name.clone()
        } else {
            game.clone()
        };

        tracks.push(Track {
            id: Uuid::new_v4().to_string(),
            path: virtual_path,
            title,
            artist: info.author.clone(),
            album,
            album_artist: info.system.clone(),
            track_number: Some((i + 1) as i32),
            disc_number: None,
            duration_ms,
            sample_rate: Some(44100),
            channels: Some(2),
            bitrate: None,
            codec: codec.clone(),
            file_size,
            has_artwork: false,
            rating: 0,
            modified_at,
        });
    }

    Ok(tracks)
}
