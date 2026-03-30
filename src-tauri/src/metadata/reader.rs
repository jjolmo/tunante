use crate::audio::vgm_path::{is_gme_file, is_gsf_file, is_psf_file, is_psf2_file, is_twosf_file, is_usf_file};
use crate::db::models::Track;
use crate::metadata::gme_reader;
use crate::metadata::gsf_reader;
use crate::metadata::psf_reader;
use crate::metadata::psf2_reader;
use crate::metadata::twosf_reader;
use crate::metadata::usf_reader;
use crate::metadata::vgmstream_reader;
use lofty::file::AudioFile;
use lofty::file::TaggedFileExt;
use lofty::tag::{Accessor, ItemKey};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

/// Global timeout for reading metadata from any single file (seconds).
/// Prevents hanging on problematic files (corrupt USF, raw DSP, etc.).
const METADATA_READ_TIMEOUT_SECS: u64 = 10;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("Lofty error: {0}")]
    Lofty(#[from] lofty::error::LoftyError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("GME error: {0}")]
    Gme(String),
    #[error("Vgmstream error: {0}")]
    Vgmstream(String),
    #[error("GSF error: {0}")]
    Gsf(String),
    #[error("2SF error: {0}")]
    TwoSf(String),
    #[error("PSF error: {0}")]
    Psf(String),
    #[error("PSF2 error: {0}")]
    Psf2(String),
    #[error("USF error: {0}")]
    Usf(String),
    #[error("Metadata read timeout: {0}")]
    Timeout(String),
}

/// Check if a file is a vgmstream-only format (not handled by GME or standard decoders)
fn is_vgmstream_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // Standard audio formats handled by symphonia/lofty
    let standard = [
        "mp3", "flac", "ogg", "wav", "aac", "aiff", "wma", "m4a", "opus", "ape", "wv",
    ];
    if standard.contains(&ext.as_str()) {
        return false;
    }
    // GME formats handled by game-music-emu
    if is_gme_file(path) {
        return false;
    }
    // Check if vgmstream recognizes this extension
    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");
    vgmstream_rs::Vgmstream::is_valid(filename)
}

/// Inner implementation without timeout.
fn read_metadata_all_inner(path: &Path) -> Result<Vec<Track>, MetadataError> {
    read_metadata_all_inner_opts(path, false)
}

fn read_metadata_all_inner_opts(path: &Path, fast_scan: bool) -> Result<Vec<Track>, MetadataError> {
    if is_gme_file(path) {
        return gme_reader::read_gme_metadata_with_opts(path, fast_scan).map_err(MetadataError::Gme);
    }
    if is_gsf_file(path) {
        return gsf_reader::read_gsf_metadata(path).map_err(MetadataError::Gsf);
    }
    if is_usf_file(path) {
        return usf_reader::read_usf_metadata(path).map_err(MetadataError::Usf);
    }
    if is_twosf_file(path) {
        return twosf_reader::read_twosf_metadata(path).map_err(MetadataError::TwoSf);
    }
    if is_psf2_file(path) {
        return psf2_reader::read_psf2_metadata(path).map_err(MetadataError::Psf2);
    }
    if is_psf_file(path) {
        return psf_reader::read_psf_metadata(path).map_err(MetadataError::Psf);
    }
    if is_vgmstream_file(path) {
        return vgmstream_reader::read_vgmstream_metadata(path)
            .map_err(MetadataError::Vgmstream);
    }
    // Standard format via lofty; if lofty fails, use a fallback based on filename/fs metadata.
    match read_metadata(path) {
        Ok(t) => Ok(vec![t]),
        Err(_) => read_metadata_fallback(path).map(|t| vec![t]),
    }
}

/// Read metadata, returning potentially multiple tracks for multi-track VGM files.
/// Wraps the inner reader with a global timeout to prevent hanging on problematic files.
pub fn read_metadata_all(path: &Path) -> Result<Vec<Track>, MetadataError> {
    read_metadata_all_with_opts(path, false)
}

/// Read metadata with configurable fast_scan mode.
/// When fast_scan is true, skip silence detection for unknown-duration GME tracks.
pub fn read_metadata_all_with_opts(path: &Path, fast_scan: bool) -> Result<Vec<Track>, MetadataError> {
    let path_buf = path.to_path_buf();
    // Silence detection can take a while per track, so give more time when not fast-scanning
    let timeout = if fast_scan { METADATA_READ_TIMEOUT_SECS } else { METADATA_READ_TIMEOUT_SECS * 6 };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = read_metadata_all_inner_opts(&path_buf, fast_scan);
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(timeout)) {
        Ok(result) => result,
        Err(_) => {
            log::warn!(
                "Metadata read timeout ({}s) for: {}",
                timeout,
                path.display()
            );
            Err(MetadataError::Timeout(path.display().to_string()))
        }
    }
}

pub fn read_metadata(path: &Path) -> Result<Track, MetadataError> {
    let tagged_file = lofty::read_from_path(path)?;
    let properties = tagged_file.properties();
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let (title, artist, album, album_artist, track_number, disc_number) = if let Some(tag) = tag {
        (
            tag.title().map(|s| s.to_string()),
            tag.artist().map(|s| s.to_string()),
            tag.album().map(|s| s.to_string()),
            tag.get_string(&lofty::tag::ItemKey::AlbumArtist)
                .map(|s| s.to_string()),
            tag.track().map(|n| n as i32),
            tag.disk().map(|n| n as i32),
        )
    } else {
        (None, None, None, None, None, None)
    };

    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let has_artwork = tag
        .map(|t| !t.pictures().is_empty())
        .unwrap_or(false);

    // Read rating from metadata (Vorbis RATING, MP4 rate, RIFF IRTD, or ID3v2 POPM)
    let rating = tag
        .and_then(|t| {
            // Text value: Vorbis "RATING", MP4 "rate", RIFF "IRTD"
            if let Some(s) = t.get_string(&ItemKey::Popularimeter) {
                return parse_rating_string(s);
            }
            // Binary value: ID3v2 POPM frame (email\0 + rating_byte + counter)
            if let Some(bytes) = t.get_binary(&ItemKey::Popularimeter, false) {
                if let Some(null_pos) = bytes.iter().position(|&b| b == 0) {
                    if null_pos + 1 < bytes.len() {
                        return Some(popm_byte_to_stars(bytes[null_pos + 1]));
                    }
                }
            }
            None
        })
        .unwrap_or(0);

    let file_meta = std::fs::metadata(path)?;

    Ok(Track {
        id: Uuid::new_v4().to_string(),
        path: path.to_string_lossy().to_string(),
        title: title.unwrap_or(file_name),
        artist: artist.unwrap_or_default(),
        album: album.filter(|a| !a.trim().is_empty()).unwrap_or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        }),
        album_artist: album_artist.unwrap_or_default(),
        track_number,
        disc_number,
        duration_ms: properties.duration().as_millis() as i64,
        sample_rate: properties.sample_rate().map(|r| r as i32),
        channels: properties.channels().map(|c| c as i32),
        bitrate: properties.audio_bitrate().map(|b| b as i32),
        codec: detect_codec(path),
        file_size: file_meta.len() as i64,
        has_artwork,
        rating,
        modified_at: file_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

pub fn extract_artwork_base64(path: &Path) -> Result<Option<String>, MetadataError> {
    // 1. Try embedded artwork from file tags
    if let Ok(tagged_file) = lofty::read_from_path(path) {
        let tag = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag());

        if let Some(tag) = tag {
            if let Some(picture) = tag.pictures().first() {
                use base64::Engine;
                let mime = match picture.mime_type() {
                    Some(lofty::picture::MimeType::Png) => "image/png",
                    Some(lofty::picture::MimeType::Jpeg) => "image/jpeg",
                    Some(lofty::picture::MimeType::Bmp) => "image/bmp",
                    Some(lofty::picture::MimeType::Gif) => "image/gif",
                    _ => "image/jpeg",
                };
                let b64 = base64::engine::general_purpose::STANDARD.encode(picture.data());
                return Ok(Some(format!("data:{};base64,{}", mime, b64)));
            }
        }
    }

    // 2. Fallback: look for cover image in the same folder
    if let Some(parent) = path.parent() {
        if let Some(art) = find_folder_artwork(parent) {
            return Ok(Some(art));
        }
    }

    Ok(None)
}

/// Search for common cover art filenames in a folder.
fn find_folder_artwork(folder: &Path) -> Option<String> {
    use base64::Engine;

    const NAMES: &[&str] = &[
        "cover", "folder", "front", "album", "albumart", "art", "thumb",
    ];
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "bmp", "gif", "webp"];

    // First pass: check common names (cover.jpg, folder.png, etc.)
    for name in NAMES {
        for ext in EXTS {
            let candidate = folder.join(format!("{}.{}", name, ext));
            if candidate.is_file() {
                return read_image_as_data_uri(&candidate);
            }
            // Also try uppercase extension
            let candidate_upper = folder.join(format!("{}.{}", name, ext.to_uppercase()));
            if candidate_upper.is_file() {
                return read_image_as_data_uri(&candidate_upper);
            }
        }
    }

    // Second pass: use first image file found in the folder
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                if EXTS.contains(&ext.to_ascii_lowercase().as_str()) && p.is_file() {
                    return read_image_as_data_uri(&p);
                }
            }
        }
    }

    None
}

fn read_image_as_data_uri(path: &Path) -> Option<String> {
    use base64::Engine;

    let data = std::fs::read(path).ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    Some(format!("data:{};base64,{}", mime, b64))
}

fn detect_codec(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_uppercase())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Fallback metadata reader for files that no specialized reader supports.
/// Creates a basic Track from filename and filesystem metadata.
fn read_metadata_fallback(path: &Path) -> Result<Track, MetadataError> {
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let codec = detect_codec(path);
    let file_meta = std::fs::metadata(path)?;

    // Try to extract a track number from the filename prefix (e.g., "33 Enemy Deleted")
    let (title, track_number) = parse_title_and_track_number(&file_name);

    // Infer album from parent directory name
    let album = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(Track {
        id: Uuid::new_v4().to_string(),
        path: path.to_string_lossy().to_string(),
        title,
        artist: String::new(),
        album,
        album_artist: String::new(),
        track_number,
        disc_number: None,
        duration_ms: 0,
        sample_rate: None,
        channels: None,
        bitrate: None,
        codec,
        file_size: file_meta.len() as i64,
        has_artwork: false,
        rating: 0,
        modified_at: file_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

/// Convert an ID3v2 POPM rating byte (0-255) to a 0-5 star rating.
/// Mapping per the WMP/foobar2000 convention.
fn popm_byte_to_stars(byte: u8) -> i32 {
    match byte {
        0 => 0,
        1..=31 => 1,
        32..=95 => 2,
        96..=159 => 3,
        160..=223 => 4,
        _ => 5, // 224-255
    }
}

/// Parse a rating string from metadata into a 0-5 integer.
/// Handles various formats:
///   - "0"-"5" (direct 0-5 scale, common in Vorbis RATING)
///   - "0"-"255" (ID3v2 POPM byte, WMP mapping: 1→1★, 64→2★, 128→3★, 196→4★, 255→5★)
///   - "0.0"-"1.0" (normalized float scale)
fn parse_rating_string(s: &str) -> Option<i32> {
    // Try as float first (handles both integer and decimal strings)
    let val: f64 = s.trim().parse().ok()?;
    if val <= 0.0 {
        return Some(0);
    }
    let rating = if val <= 1.0 {
        // Normalized 0.0-1.0 scale → multiply by 5
        (val * 5.0).round() as i32
    } else if val <= 5.0 {
        // Direct 0-5 scale
        val.round() as i32
    } else {
        // 0-255 scale (POPM): map to 1-5 stars
        match val as i32 {
            0 => 0,
            1..=31 => 1,
            32..=95 => 2,
            96..=159 => 3,
            160..=223 => 4,
            _ => 5,
        }
    };
    Some(rating.clamp(0, 5))
}

/// Parse a filename like "33 Enemy Deleted" into (title: "Enemy Deleted", track_number: Some(33)).
/// If no leading number is found, returns the full filename as title.
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
