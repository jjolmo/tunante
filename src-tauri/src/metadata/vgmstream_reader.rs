use crate::audio::vgm_path::build_vgm_path;
use crate::db::models::Track;
use std::path::Path;
use uuid::Uuid;
use vgmstream_rs::Vgmstream;

/// Read metadata from a vgmstream-supported file.
/// Handles subsongs (multiple streams within a single file).
pub fn read_vgmstream_metadata(path: &Path) -> Result<Vec<Track>, String> {
    // Open with subsong 0 (default) to get subsong count
    let vgm = Vgmstream::open(path, 0)?;
    let initial_info = vgm.info();

    let file_meta = std::fs::metadata(path).map_err(|e| format!("IO error: {}", e))?;
    let file_path_str = path.to_string_lossy().to_string();
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let codec_ext = path
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

    let subsong_count = initial_info.subsong_count;

    // If no subsong concept or only 1, read the already-opened stream
    if subsong_count <= 1 {
        let info = initial_info;
        let duration_ms = if info.play_samples > 0 {
            (info.play_samples as f64 / info.sample_rate as f64 * 1000.0) as i64
        } else if info.stream_samples > 0 {
            (info.stream_samples as f64 / info.sample_rate as f64 * 1000.0) as i64
        } else {
            0
        };

        let title = if !info.stream_name.is_empty() {
            info.stream_name.clone()
        } else {
            file_name.clone()
        };

        // Always use file extension as codec (consistent with other readers),
        // so the console browser can map extensions to platforms
        let codec_name = codec_ext.clone();

        return Ok(vec![Track {
            id: Uuid::new_v4().to_string(),
            path: file_path_str,
            title,
            artist: String::new(),
            album: file_name,
            album_artist: info.meta_name.clone(),
            track_number: None,
            disc_number: None,
            duration_ms,
            sample_rate: Some(info.sample_rate),
            channels: Some(info.channels),
            bitrate: if info.bitrate > 0 {
                Some(info.bitrate)
            } else {
                None
            },
            codec: codec_name,
            file_size,
            has_artwork: false,
            rating: 0,
            modified_at,
        }]);
    }

    // Multiple subsongs — open each one separately
    drop(vgm);
    let mut tracks = Vec::with_capacity(subsong_count as usize);

    for i in 1..=subsong_count {
        let vgm = match Vgmstream::open(path, i) {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    "vgmstream: failed to open subsong {} of {}: {}",
                    i,
                    file_path_str,
                    e
                );
                continue;
            }
        };
        let info = vgm.info();

        let duration_ms = if info.play_samples > 0 {
            (info.play_samples as f64 / info.sample_rate as f64 * 1000.0) as i64
        } else if info.stream_samples > 0 {
            (info.stream_samples as f64 / info.sample_rate as f64 * 1000.0) as i64
        } else {
            0
        };

        let title = if !info.stream_name.is_empty() {
            info.stream_name.clone()
        } else {
            format!("{} - Stream {}", file_name, i)
        };

        // Always use file extension as codec (consistent with other readers),
        // so the console browser can map extensions to platforms
        let codec_name = codec_ext.clone();

        // Virtual path: path#N (1-based subsong index)
        let virtual_path = build_vgm_path(&file_path_str, i as usize);

        tracks.push(Track {
            id: Uuid::new_v4().to_string(),
            path: virtual_path,
            title,
            artist: String::new(),
            album: file_name.clone(),
            album_artist: info.meta_name.clone(),
            track_number: Some(i),
            disc_number: None,
            duration_ms,
            sample_rate: Some(info.sample_rate),
            channels: Some(info.channels),
            bitrate: if info.bitrate > 0 {
                Some(info.bitrate)
            } else {
                None
            },
            codec: codec_name,
            file_size,
            has_artwork: false,
            rating: 0,
            modified_at,
        });
    }

    Ok(tracks)
}
