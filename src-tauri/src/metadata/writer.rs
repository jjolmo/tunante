//! Write metadata back to audio files.
//!
//! Supports:
//! - PSF-family files (GSF, PSF, 2SF): Modify the [TAG] section at end of file
//! - Standard audio files (MP3, FLAC, OGG, etc.): Write via lofty (Vorbis RATING)
//! - GME chiptune files (NSF, SPC, GBS, etc.): Write #RATING to folder-level `_ratings.m3u`
//! - vgmstream/unknown formats: Write #RATING to folder-level `_ratings.m3u`

use crate::audio::vgm_path::{is_gme_file, is_gsf_file, is_psf_file, is_twosf_file, is_usf_file};
use std::io::Write;
use std::path::Path;

/// Write a rating value (0-5) to a file's metadata.
///
/// Returns Ok(true) if the rating was written (to file tags or companion .m3u),
/// Ok(false) if no action was taken (e.g., rating 0 with no existing M3U),
/// or Err on I/O failure.
pub fn write_rating_to_file(path_str: &str, rating: i32) -> Result<bool, String> {
    // Handle virtual paths (e.g., "/path/to/file.nsf#3") — extract the real file path
    let real_path_str = if let Some(hash_pos) = path_str.rfind('#') {
        let after_hash = &path_str[hash_pos + 1..];
        if after_hash.chars().all(|c| c.is_ascii_digit()) {
            &path_str[..hash_pos]
        } else {
            path_str
        }
    } else {
        path_str
    };

    let path = Path::new(real_path_str);

    // PSF-family formats: write to [TAG] section
    if is_gsf_file(path) || is_psf_file(path) || is_twosf_file(path) || is_usf_file(path) {
        write_psf_tag_rating(path, rating)?;
        return Ok(true);
    }

    // GME formats: write rating to folder-level _ratings.m3u
    if is_gme_file(path) {
        // Extract 1-based track number from virtual path (e.g., file.nsf#0 → track 1)
        let track_number = if let Some(hash_pos) = path_str.rfind('#') {
            let after_hash = &path_str[hash_pos + 1..];
            after_hash.parse::<i32>().unwrap_or(0) + 1
        } else {
            1
        };
        return write_folder_m3u_rating(path, track_number, rating);
    }

    // Standard audio formats: try lofty
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let standard = [
        "mp3", "flac", "ogg", "wav", "aac", "aiff", "wma", "m4a", "opus", "ape", "wv",
    ];
    if standard.contains(&ext.as_str()) {
        write_lofty_rating(path, rating)?;
        return Ok(true);
    }

    // vgmstream or unknown format: write to folder-level _ratings.m3u
    // Extract track number from virtual path (#N is 1-based for vgmstream, absent for single tracks)
    let track_number = if let Some(hash_pos) = path_str.rfind('#') {
        let after_hash = &path_str[hash_pos + 1..];
        after_hash.parse::<i32>().unwrap_or(1)
    } else {
        1
    };
    write_folder_m3u_rating(path, track_number, rating)
}

/// Write rating to a PSF-family file's [TAG] section.
///
/// PSF tag format:
/// - Starts with "[TAG]" marker (5 bytes)
/// - Followed by key=value pairs separated by 0x0A (newline)
/// - Located at the end of the file, after compressed program data
fn write_psf_tag_rating(path: &Path, rating: i32) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    // Find [TAG] section
    let tag_marker = b"[TAG]";
    let tag_pos = data
        .windows(tag_marker.len())
        .position(|w| w == tag_marker);

    if let Some(pos) = tag_pos {
        // Parse existing tags after "[TAG]"
        let tag_bytes = &data[pos + tag_marker.len()..];
        let tag_str = String::from_utf8_lossy(tag_bytes).to_string();

        // Rebuild tags, updating or removing the rating line
        let mut new_lines: Vec<String> = Vec::new();
        let mut found_rating = false;

        for line in tag_str.split('\n') {
            let trimmed = line.trim_end_matches('\r');
            if trimmed.is_empty() {
                continue;
            }
            if let Some(eq_pos) = trimmed.find('=') {
                let key = &trimmed[..eq_pos];
                if key.eq_ignore_ascii_case("rating") {
                    found_rating = true;
                    if rating > 0 {
                        new_lines.push(format!("rating={}", rating));
                    }
                    // If rating == 0, skip (remove the tag)
                } else {
                    new_lines.push(trimmed.to_string());
                }
            } else {
                // Non key=value line — preserve
                new_lines.push(trimmed.to_string());
            }
        }

        if !found_rating && rating > 0 {
            new_lines.push(format!("rating={}", rating));
        }

        // Rebuild the file: original data up to [TAG] + new tag section
        let mut new_data = data[..pos].to_vec();
        if !new_lines.is_empty() {
            new_data.extend_from_slice(b"[TAG]");
            for line in &new_lines {
                new_data.push(b'\n');
                new_data.extend_from_slice(line.as_bytes());
            }
        }
        // If all tags were removed (including rating=0 removal), don't write [TAG] at all

        std::fs::write(path, &new_data)
            .map_err(|e| format!("Failed to write file: {}", e))?;
    } else {
        // No [TAG] section exists — append one if rating > 0
        if rating > 0 {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .map_err(|e| format!("Failed to open file for append: {}", e))?;
            let tag_section = format!("[TAG]\nrating={}", rating);
            file.write_all(tag_section.as_bytes())
                .map_err(|e| format!("Failed to append tags: {}", e))?;
        }
    }

    Ok(())
}

/// Write rating to a standard audio file using lofty.
///
/// For Vorbis/FLAC/OGG: writes "RATING" comment (0-5 direct scale)
/// For MP4: writes "rate" atom
/// For ID3v2/MP3: writes TXXX:RATING (text, not POPM binary)
fn write_lofty_rating(path: &Path, rating: i32) -> Result<(), String> {
    use lofty::file::TaggedFileExt;
    use lofty::tag::ItemKey;
    use lofty::tag::TagExt;

    let mut tagged_file =
        lofty::read_from_path(path).map_err(|e| format!("lofty read error: {}", e))?;

    // Get or create the primary tag
    let tag = if let Some(tag) = tagged_file.primary_tag_mut() {
        tag
    } else if let Some(tag) = tagged_file.first_tag_mut() {
        tag
    } else {
        return Ok(()); // No tag to write to — skip silently
    };

    if rating > 0 {
        // Insert rating as text value (Vorbis RATING / MP4 rate)
        tag.insert_text(ItemKey::Popularimeter, rating.to_string());
    } else {
        // Remove rating tag
        tag.remove_key(&ItemKey::Popularimeter);
    }

    tag.save_to_path(path, lofty::config::WriteOptions::default())
        .map_err(|e| format!("lofty write error: {}", e))?;

    Ok(())
}

/// Write a rating to a folder-level `_ratings.m3u` file using `#RATING:filename:N:R` comment lines.
/// Used for vgmstream and unknown formats that don't support embedded metadata tags.
///
/// `file_path` is the audio file path. The M3U is `file_path.parent()/_ratings.m3u`.
/// `track_number` is 1-based. Rating 0 removes the rating line.
/// If `_ratings.m3u` becomes empty after removal, it is deleted.
fn write_folder_m3u_rating(file_path: &Path, track_number: i32, rating: i32) -> Result<bool, String> {
    let folder = file_path
        .parent()
        .ok_or_else(|| "Cannot determine parent folder".to_string())?;
    let m3u_path = folder.join("_ratings.m3u");
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Cannot determine file name".to_string())?;

    if !m3u_path.exists() {
        if rating > 0 {
            let content = format!("#RATING:{}:{}:{}\n", file_name, track_number, rating);
            std::fs::write(&m3u_path, &content)
                .map_err(|e| format!("Failed to create _ratings.m3u: {}", e))?;
            log::info!(
                "Created _ratings.m3u with rating {} for {}:{} in {}",
                rating, file_name, track_number, folder.display()
            );
            return Ok(true);
        }
        return Ok(false);
    }

    let content = std::fs::read_to_string(&m3u_path)
        .map_err(|e| format!("Failed to read _ratings.m3u: {}", e))?;

    let mut rating_lines: Vec<String> = Vec::new();
    let mut other_lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("#RATING:") {
            // Format: filename:track_number:rating
            let parts: Vec<&str> = rest.rsplitn(3, ':').collect();
            // rsplitn(3, ':') on "file.adx:1:5" → ["5", "1", "file.adx"]
            if parts.len() == 3 {
                let existing_file = parts[2];
                let existing_track = parts[1].parse::<i32>().unwrap_or(0);
                if existing_file == file_name && existing_track == track_number {
                    // Skip — we'll add the updated line below
                    continue;
                }
            }
            rating_lines.push(line.to_string());
        } else {
            other_lines.push(line);
        }
    }

    if rating > 0 {
        rating_lines.push(format!("#RATING:{}:{}:{}", file_name, track_number, rating));
    }

    // Sort by filename then track number
    rating_lines.sort_by(|a, b| {
        let key = |line: &str| -> (String, i32) {
            if let Some(rest) = line.strip_prefix("#RATING:") {
                let parts: Vec<&str> = rest.rsplitn(3, ':').collect();
                if parts.len() == 3 {
                    return (parts[2].to_string(), parts[1].parse::<i32>().unwrap_or(0));
                }
            }
            (String::new(), 0)
        };
        key(a).cmp(&key(b))
    });

    // If no rating lines remain, delete the file
    if rating_lines.is_empty() {
        let _ = std::fs::remove_file(&m3u_path);
        log::info!("Removed empty _ratings.m3u in {}", folder.display());
        return Ok(true);
    }

    let mut output = String::new();
    for line in &rating_lines {
        output.push_str(line);
        output.push('\n');
    }
    for line in &other_lines {
        if !line.is_empty() {
            output.push_str(line);
            output.push('\n');
        }
    }

    std::fs::write(&m3u_path, &output)
        .map_err(|e| format!("Failed to write _ratings.m3u: {}", e))?;

    log::info!(
        "Rating {} written to _ratings.m3u for {}:{} in {}",
        rating, file_name, track_number, folder.display()
    );

    Ok(true)
}
