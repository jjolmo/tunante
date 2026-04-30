//! Resolve track ratings from folder-level `_ratings.m3u` files at read time.
//!
//! Ratings can come from two sources:
//! - Database (set in-app or imported during scan)
//! - `_ratings.m3u` next to the audio file (often synced from other machines)
//!
//! When a track has DB rating 0 but its `_ratings.m3u` lists a rating, we use
//! the file value. The DB always wins when non-zero, so user changes are
//! preserved.

use crate::audio::vgm_path::{is_gme_file, parse_vgm_path};
use crate::db::models::Track;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// For every track with `rating == 0`, look up its folder's `_ratings.m3u`
/// and set the rating in-place if found. Returns `(track_id, new_rating)` for
/// every track that was updated, so callers can persist back to the DB.
pub fn apply_file_ratings(tracks: &mut [Track]) -> Vec<(String, i32)> {
    let mut cache: HashMap<PathBuf, HashMap<(String, i32), i32>> = HashMap::new();
    let mut updates = Vec::new();

    for track in tracks.iter_mut() {
        if track.rating != 0 {
            continue;
        }
        let (file_path, idx) = parse_vgm_path(&track.path);
        let path = Path::new(file_path);
        let folder = match path.parent() {
            Some(p) => p,
            None => continue,
        };
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s,
            None => continue,
        };

        // GME virtual paths use 0-based subtrack index; vgmstream uses 1-based.
        // Single-track files have no `#N` suffix and key 1.
        let track_num = match idx {
            Some(n) => {
                if is_gme_file(path) {
                    (n as i32) + 1
                } else {
                    n as i32
                }
            }
            None => 1,
        };

        let map = cache
            .entry(folder.to_path_buf())
            .or_insert_with(|| parse_all_folder_ratings(&folder.join("_ratings.m3u")));

        if let Some(&r) = map.get(&(filename.to_string(), track_num)) {
            if r > 0 {
                track.rating = r;
                updates.push((track.id.clone(), r));
            }
        }
    }

    updates
}

fn parse_all_folder_ratings(m3u_path: &Path) -> HashMap<(String, i32), i32> {
    let mut out = HashMap::new();
    let content = match std::fs::read_to_string(m3u_path) {
        Ok(c) => c,
        Err(_) => return out,
    };
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("#RATING:") {
            let parts: Vec<&str> = rest.rsplitn(3, ':').collect();
            if parts.len() == 3 {
                let filename = parts[2].to_string();
                if let (Ok(track_num), Ok(rating_val)) =
                    (parts[1].parse::<i32>(), parts[0].parse::<i32>())
                {
                    if (0..=5).contains(&rating_val) && track_num > 0 {
                        out.insert((filename, track_num), rating_val);
                    }
                }
            }
        }
    }
    out
}
