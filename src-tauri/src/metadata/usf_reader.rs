use crate::db::models::Track;
use lazyusf2_rs::read_usf_tags;
use std::path::Path;
use uuid::Uuid;

const DEFAULT_FADE_MS: u64 = 10_000;

/// Read metadata from a USF/miniusf file using psflib PSF tag extraction.
pub fn read_usf_metadata(path: &Path) -> Result<Vec<Track>, String> {
    let tags = read_usf_tags(path)?;

    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let file_meta = std::fs::metadata(path).map_err(|e| format!("IO error: {}", e))?;

    let (title, track_number) = if !tags.title.is_empty() {
        (tags.title.clone(), extract_track_number(&file_name))
    } else {
        parse_title_and_track_number(&file_name)
    };

    let album = if !tags.game.is_empty() {
        tags.game.clone()
    } else {
        path.parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };

    let length_ms = tags.length_ms;
    let fade_ms = if tags.fade_ms > 0 {
        tags.fade_ms
    } else if length_ms > 0 {
        DEFAULT_FADE_MS
    } else {
        0
    };
    let duration_ms = if length_ms > 0 {
        (length_ms + fade_ms) as i64
    } else {
        0
    };

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

    Ok(vec![Track {
        id: Uuid::new_v4().to_string(),
        path: path.to_string_lossy().to_string(),
        title,
        artist: tags.artist,
        album,
        album_artist: String::new(),
        track_number,
        disc_number: None,
        duration_ms,
        sample_rate: Some(44100),
        channels: Some(2),
        bitrate: None,
        codec,
        file_size: file_meta.len() as i64,
        has_artwork: false,
        rating: tags.rating,
        modified_at,
    }])
}

fn extract_track_number(filename: &str) -> Option<i32> {
    let trimmed = filename.trim();
    if let Some(first_non_digit) = trimmed.find(|c: char| !c.is_ascii_digit()) {
        let num_part = &trimmed[..first_non_digit];
        if !num_part.is_empty() {
            return num_part.parse::<i32>().ok();
        }
    }
    None
}

fn parse_title_and_track_number(filename: &str) -> (String, Option<i32>) {
    let trimmed = filename.trim();
    if let Some(first_non_digit) = trimmed.find(|c: char| !c.is_ascii_digit()) {
        let num_part = &trimmed[..first_non_digit];
        if !num_part.is_empty() {
            if let Ok(num) = num_part.parse::<i32>() {
                let rest = trimmed[first_non_digit..].trim_start_matches([' ', '-', '_', '.']);
                if !rest.is_empty() {
                    return (rest.to_string(), Some(num));
                }
            }
        }
    }
    (trimmed.to_string(), None)
}
