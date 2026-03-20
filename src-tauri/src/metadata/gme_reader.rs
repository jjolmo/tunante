use crate::audio::vgm_path::build_vgm_path;
use crate::db::models::Track;
use game_music_emu::GameMusicEmu;
use std::path::Path;
use uuid::Uuid;

/// Default play duration for tracks with unknown length (2.5 minutes)
const DEFAULT_DURATION_MS: i64 = 150_000;
/// Fade duration appended after play_length
const FADE_MS: i64 = 10_000;

/// Read all sub-tracks from a GME file, returning one Track per sub-track.
/// For single-track files (e.g., most SPC files), returns a single Track without #N suffix.
/// For multi-track files (e.g., NSF with 30 tracks), returns one Track per sub-track with #N suffix.
pub fn read_gme_metadata(path: &Path) -> Result<Vec<Track>, String> {
    // Use gme_info_only sample rate (-1) — we just need metadata, not audio
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
        let play_ms = if info.play_length > 0 {
            info.play_length as i64
        } else {
            DEFAULT_DURATION_MS
        };
        let duration_ms = play_ms + FADE_MS;

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
            modified_at,
        });
    }

    Ok(tracks)
}
