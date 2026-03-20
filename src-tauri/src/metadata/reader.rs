use crate::audio::vgm_path::is_gme_file;
use crate::db::models::Track;
use crate::metadata::gme_reader;
use lofty::file::AudioFile;
use lofty::file::TaggedFileExt;
use lofty::tag::Accessor;
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("Lofty error: {0}")]
    Lofty(#[from] lofty::error::LoftyError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("GME error: {0}")]
    Gme(String),
}

/// Read metadata, returning potentially multiple tracks for multi-track VGM files.
pub fn read_metadata_all(path: &Path) -> Result<Vec<Track>, MetadataError> {
    if is_gme_file(path) {
        return gme_reader::read_gme_metadata(path).map_err(MetadataError::Gme);
    }
    // Standard format: single track
    read_metadata(path).map(|t| vec![t])
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

    let file_meta = std::fs::metadata(path)?;

    Ok(Track {
        id: Uuid::new_v4().to_string(),
        path: path.to_string_lossy().to_string(),
        title: title.unwrap_or(file_name),
        artist: artist.unwrap_or_default(),
        album: album.unwrap_or_default(),
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
        modified_at: file_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

pub fn extract_artwork_base64(path: &Path) -> Result<Option<String>, MetadataError> {
    let tagged_file = lofty::read_from_path(path)?;
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

    Ok(None)
}

fn detect_codec(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_uppercase())
        .unwrap_or_else(|| "Unknown".to_string())
}
