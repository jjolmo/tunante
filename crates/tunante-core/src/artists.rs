//! The Artists view: one entry per artist, across every folder and console.
//!
//! The same shape as [`crate::games`]: an index built from the tags, so a
//! composer whose work sits in twenty folders is one row. The album artist is
//! preferred when the rip filled it — that is the field meant for "whose
//! record this is" — and the track artist otherwise. Tracks with neither are
//! left out of this view; they are still in every other one.

use crate::db::models::Track;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Artist {
    pub name: String,
    pub count: usize,
    /// A track of theirs, for the cover.
    pub first_track: String,
}

/// The artist a track is filed under, or empty when the tags say nothing.
pub fn artist_of(track: &Track) -> &str {
    let a = track.album_artist.trim();
    if !a.is_empty() {
        return a;
    }
    track.artist.trim()
}

/// Every artist with at least one track, sorted by name, case-folded.
pub fn index(tracks: &[Track]) -> Vec<Artist> {
    // Keyed by the folded name so "nobuo uematsu" and "Nobuo Uematsu" are
    // one artist; the first spelling seen is the one shown.
    let mut by: BTreeMap<String, Artist> = BTreeMap::new();
    for t in tracks {
        let name = artist_of(t);
        if name.is_empty() {
            continue;
        }
        let key = name.to_lowercase();
        let e = by.entry(key).or_insert_with(|| Artist {
            name: name.to_string(),
            count: 0,
            first_track: t.path.clone(),
        });
        e.count += 1;
    }
    by.into_values().collect()
}

/// The tracks filed under `name` (case-folded), in library order.
pub fn tracks_of<'a>(tracks: &'a [Track], name: &str) -> Vec<&'a Track> {
    let want = name.trim().to_lowercase();
    tracks
        .iter()
        .filter(|t| artist_of(t).to_lowercase() == want)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, artist: &str, album_artist: &str) -> Track {
        Track {
            path: path.to_string(),
            artist: artist.to_string(),
            album_artist: album_artist.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn the_album_artist_wins_and_spellings_fold() {
        let all = vec![
            track("/a/1.psf", "Nobuo Uematsu", ""),
            track("/b/2.psf", "nobuo uematsu", ""),
            track("/c/3.spc", "Koji Kondo", "Nintendo"),
            track("/d/4.nsf", "", ""),
        ];
        let idx = index(&all);
        let names: Vec<&str> = idx.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Nintendo", "Nobuo Uematsu"]);
        assert_eq!(idx[1].count, 2);
        assert_eq!(tracks_of(&all, "NOBUO UEMATSU").len(), 2);
        assert_eq!(tracks_of(&all, "Koji Kondo").len(), 0, "filed under the album artist");
    }
}
