use crate::audio::vgm_path::build_vgm_path;
use crate::db::models::Track;
use game_music_emu::GameMusicEmu;
use std::collections::HashMap;
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

/// Parsed entry from a GME-style .m3u playlist.
struct M3uEntry {
    track: i32,        // 1-based track number
    title: String,
    length_ms: i64,    // -1 if not specified
    fade_ms: i64,      // -1 if not specified
}

/// Parse a GME-style .m3u file and return entries keyed by 1-based track number.
/// Format: `filename::TYPE,track,title,length,,fade`
/// or:     `filename,track,title,length,,fade`
fn parse_gme_m3u(path: &Path) -> Option<HashMap<i32, M3uEntry>> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut entries = HashMap::new();

    for line in content.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Skip the filename part: either `name::TYPE,rest` or `name,rest`
        let rest = if let Some(pos) = line.find("::") {
            // `filename::TYPE,rest` — skip past the type and comma
            let after_type = &line[pos + 2..];
            after_type.find(',').map(|p| &after_type[p + 1..])
        } else {
            // `filename,rest` — skip past first comma, but only if followed by a digit
            line.find(',').and_then(|p| {
                let after = line[p + 1..].trim_start();
                if after.starts_with(|c: char| c.is_ascii_digit() || c == '$') {
                    Some(after)
                } else {
                    None
                }
            })
        };

        let rest = match rest {
            Some(r) => r,
            None => continue,
        };

        // Parse: track,title,length,,fade
        let fields: Vec<&str> = split_m3u_fields(rest);
        if fields.is_empty() {
            continue;
        }

        let track: i32 = match fields[0].trim().parse() {
            Ok(t) => t,
            Err(_) => continue,
        };

        let title = if fields.len() > 1 {
            // Unescape \, sequences
            fields[1].replace("\\,", ",").trim().to_string()
        } else {
            String::new()
        };

        let length_ms = if fields.len() > 2 {
            parse_m3u_time(fields[2])
        } else {
            -1
        };

        // fields[3] is usually empty (loop intro), skip it
        let fade_ms = if fields.len() > 4 {
            parse_m3u_time(fields[4])
        } else {
            -1
        };

        entries.insert(track, M3uEntry {
            track,
            title,
            length_ms,
            fade_ms,
        });
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Split m3u fields by comma, respecting `\,` escapes.
fn split_m3u_fields(s: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b',' && (i == 0 || bytes[i - 1] != b'\\') {
            fields.push(&s[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    fields.push(&s[start..]);
    fields
}

/// Parse GME m3u time format: `M:SS`, `M:SS.mmm`, or just seconds `SS`.
fn parse_m3u_time(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() || s == "-" {
        return -1;
    }

    let (main_part, frac_ms) = if let Some(dot_pos) = s.find('.') {
        let frac_str = &s[dot_pos + 1..];
        let frac = match frac_str.len() {
            0 => 0i64,
            1 => frac_str.parse::<i64>().unwrap_or(0) * 100,
            2 => frac_str.parse::<i64>().unwrap_or(0) * 10,
            3 => frac_str.parse::<i64>().unwrap_or(0),
            _ => frac_str[..3].parse::<i64>().unwrap_or(0),
        };
        (&s[..dot_pos], frac)
    } else {
        (s, 0i64)
    };

    let parts: Vec<&str> = main_part.split(':').collect();
    let seconds: i64 = match parts.len() {
        1 => parts[0].parse::<i64>().unwrap_or(0),
        2 => {
            let min = parts[0].parse::<i64>().unwrap_or(0);
            let sec = parts[1].parse::<i64>().unwrap_or(0);
            min * 60 + sec
        }
        3 => {
            let hr = parts[0].parse::<i64>().unwrap_or(0);
            let min = parts[1].parse::<i64>().unwrap_or(0);
            let sec = parts[2].parse::<i64>().unwrap_or(0);
            hr * 3600 + min * 60 + sec
        }
        _ => 0,
    };

    seconds * 1000 + frac_ms
}

/// Detect the actual play duration of a GME track by emulating until silence.
/// Returns the duration in milliseconds, or None if the track loops past the max limit.
fn detect_duration_by_silence(path: &Path, track_index: usize) -> Option<i64> {
    let emu = GameMusicEmu::from_file(path, 44100).ok()?;
    emu.start_track(track_index).ok()?;
    emu.set_fade(MAX_DETECT_DURATION_MS);

    let mut buf = vec![0i16; DETECT_CHUNK_SAMPLES];
    loop {
        if emu.track_ended() {
            let ms = emu.tell() as i64;
            return Some(ms);
        }
        if emu.tell() as i32 >= MAX_DETECT_DURATION_MS {
            return None;
        }
        if emu.play(DETECT_CHUNK_SAMPLES, &mut buf).is_err() {
            return None;
        }
    }
}

/// Read all sub-tracks from a GME file, returning one Track per sub-track.
/// Parses matching .m3u playlist in Rust for track names, durations, and ordering.
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

    // Try to load matching .m3u playlist (parsed in Rust — GME's C m3u loader is broken)
    let m3u_path = path.with_extension("m3u");
    let m3u_entries = parse_gme_m3u(&m3u_path);

    // If m3u defines track order, use it; otherwise iterate 0..track_count
    let track_indices: Vec<usize> = if let Some(ref m3u) = m3u_entries {
        // Use m3u track order (1-based → 0-based index for GME)
        let mut indices: Vec<i32> = m3u.keys().copied().collect();
        indices.sort();
        indices.iter().map(|&t| (t - 1).max(0) as usize).collect()
    } else {
        (0..track_count).collect()
    };

    let mut tracks = Vec::with_capacity(track_indices.len());

    for (seq, &i) in track_indices.iter().enumerate() {
        if i >= track_count {
            continue;
        }

        let info = match emu.track_info(i) {
            Ok(info) => info,
            Err(e) => {
                log::warn!("GME track info error for track {}: {}", i, e);
                continue;
            }
        };

        // M3u entry for this track (1-based key)
        let m3u_entry = m3u_entries.as_ref().and_then(|m| m.get(&((i + 1) as i32)));

        // Title: prefer m3u title, then GME song name, then filename
        let title = if let Some(entry) = m3u_entry {
            if !entry.title.is_empty() {
                // Strip "Game - Author - " prefix that Zophar's m3u often has
                let t = &entry.title;
                // Find last " - " and use everything after it as the actual title
                // But only if there are at least 2 " - " separators (game - author - title)
                let dashes: Vec<usize> = t.match_indices(" - ").map(|(pos, _)| pos).collect();
                if dashes.len() >= 2 {
                    t[dashes[dashes.len() - 1] + 3..].to_string()
                } else {
                    entry.title.clone()
                }
            } else if !info.song.is_empty() {
                info.song.clone()
            } else {
                format!("{} - Track {}", file_name, seq + 1)
            }
        } else if !info.song.is_empty() {
            info.song.clone()
        } else if track_count == 1 {
            file_name.clone()
        } else {
            format!("{} - Track {}", file_name, i + 1)
        };

        // Duration: prefer m3u length, then GME play_length, then silence detection, then default
        let duration_ms = if let Some(entry) = m3u_entry {
            if entry.length_ms > 0 {
                let fade = if entry.fade_ms > 0 { entry.fade_ms } else { FADE_MS };
                entry.length_ms + fade
            } else if info.play_length > 0 {
                info.play_length as i64 + FADE_MS
            } else if !fast_scan {
                detect_duration_by_silence(path, i).unwrap_or(DEFAULT_DURATION_MS + FADE_MS)
            } else {
                DEFAULT_DURATION_MS + FADE_MS
            }
        } else if info.play_length > 0 {
            info.play_length as i64 + FADE_MS
        } else if !fast_scan {
            detect_duration_by_silence(path, i).unwrap_or(DEFAULT_DURATION_MS + FADE_MS)
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
            track_number: Some((seq + 1) as i32),
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
