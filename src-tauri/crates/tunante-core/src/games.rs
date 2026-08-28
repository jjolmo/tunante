//! Grouping a library by the game a track belongs to.
//!
//! # Why this is not the same as the folder view
//!
//! The album view answers "what directories hold music", which is the disk's
//! opinion. This answers "what does the tag say this is from", which is the
//! ripper's. They agree for a collection where one game is one directory and
//! disagree everywhere else — a rip split across `Disc 1` and `Disc 2`, a
//! folder holding several games' tracks, or loose files in a downloads folder
//! that are tagged correctly and filed nowhere.
//!
//! Neither is right. Two indexes over the same rows is the whole point of
//! having more than one tab.
//!
//! Untagged tracks fall back to the name of the folder they are in rather than
//! collecting in one "unknown" heap: for a console rip the directory is almost
//! always the game's name, so the fallback is usually the right answer and
//! never a useless one.

use crate::db::models::Track;

/// One game, and enough to draw a row or a tile for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub name: String,
    /// Composer or publisher, whichever the tags offered.
    pub by: String,
    pub count: usize,
    /// A track in it, for the cover.
    pub first_track: String,
}

/// What to file a track under.
///
/// `album` when there is one, else the folder's name, else the file's.
pub fn game_of(track: &Track) -> String {
    let album = track.album.trim();
    if !album.is_empty() {
        return album.to_string();
    }
    let file = track.path.split('#').next().unwrap_or(&track.path);
    let mut parts = file.rsplit('/');
    let name = parts.next().unwrap_or("");
    match parts.next() {
        Some(dir) if !dir.is_empty() => dir.to_string(),
        // A file at the root of the filesystem has no folder to be named after.
        _ => name.to_string(),
    }
}

/// Every game in the library, sorted by name.
pub fn index(tracks: &[Track]) -> Vec<Game> {
    let mut by_name: std::collections::BTreeMap<String, (usize, String, String)> =
        Default::default();

    for t in tracks {
        let name = game_of(t);
        let entry = by_name
            .entry(name)
            .or_insert_with(|| (0, t.path.clone(), String::new()));
        entry.0 += 1;
        // The first non-empty attribution wins, and the album artist is
        // preferred: for a soundtrack it names the composer, where `artist` is
        // often per-track and sometimes a performer.
        if entry.2.is_empty() {
            let by = if !t.album_artist.trim().is_empty() {
                t.album_artist.trim()
            } else {
                t.artist.trim()
            };
            entry.2 = by.to_string();
        }
    }

    by_name
        .into_iter()
        .map(|(name, (count, first_track, by))| Game { name, by, count, first_track })
        .collect()
}

/// The tracks of one game, in the order the library returned them.
pub fn tracks_of<'a>(tracks: &'a [Track], game: &str) -> Vec<&'a Track> {
    tracks.iter().filter(|t| game_of(t) == game).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, album: &str, album_artist: &str, artist: &str) -> Track {
        Track {
            id: path.into(),
            path: path.into(),
            title: path.into(),
            artist: artist.into(),
            album: album.into(),
            album_artist: album_artist.into(),
            track_number: None,
            disc_number: None,
            duration_ms: 1000,
            sample_rate: None,
            channels: None,
            bitrate: None,
            codec: "test".into(),
            file_size: 0,
            has_artwork: false,
            rating: 0,
            modified_at: 0,
            ..Default::default()
        }
    }

    #[test]
    fn the_album_tag_is_the_game() {
        let t = track("/m/whatever/a.psf", "Legend of Mana", "", "Yoko Shimomura");
        assert_eq!(game_of(&t), "Legend of Mana");
    }

    /// The point of this index: two folders, one game.
    #[test]
    fn a_game_split_across_folders_is_still_one_game() {
        let all = vec![
            track("/m/FF7 Disc 1/a.psf", "Final Fantasy VII", "Nobuo Uematsu", ""),
            track("/m/FF7 Disc 2/b.psf", "Final Fantasy VII", "Nobuo Uematsu", ""),
        ];
        let games = index(&all);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Final Fantasy VII");
        assert_eq!(games[0].count, 2);
        assert_eq!(games[0].by, "Nobuo Uematsu");
    }

    /// And the other way: one folder, two games. A downloads folder does this.
    #[test]
    fn one_folder_holding_two_games_is_two_games() {
        let all = vec![
            track("/m/rips/a.nsf", "Metroid", "", ""),
            track("/m/rips/b.nsf", "Kid Icarus", "", ""),
        ];
        assert_eq!(
            index(&all).iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            ["Kid Icarus", "Metroid"],
        );
    }

    /// An untagged rip must land somewhere recognisable, not in a heap called
    /// "unknown" with everything else that was never tagged.
    #[test]
    fn without_a_tag_the_folder_names_it() {
        let t = track("/m/Chrono Trigger/01.spc", "", "", "");
        assert_eq!(game_of(&t), "Chrono Trigger");
    }

    #[test]
    fn whitespace_is_not_a_tag() {
        let t = track("/m/Zelda/a.psf", "   ", "", "");
        assert_eq!(game_of(&t), "Zelda");
    }

    /// A subsong address is several tracks over one file, and `#3` is not part
    /// of any name.
    #[test]
    fn a_subsong_suffix_does_not_leak_into_the_name() {
        let t = track("/m/GB/pokemon.gbs#3", "", "", "");
        assert_eq!(game_of(&t), "GB");
    }

    /// album_artist over artist: on a soundtrack the first names the composer
    /// and the second is often per-track.
    #[test]
    fn the_album_artist_is_preferred_for_the_attribution() {
        let all = vec![track("/m/a.psf", "Xenogears", "Yasunori Mitsuda", "Someone Else")];
        assert_eq!(index(&all)[0].by, "Yasunori Mitsuda");
    }

    #[test]
    fn falling_back_to_artist_when_there_is_no_album_artist() {
        let all = vec![track("/m/a.psf", "Xenogears", "", "Yasunori Mitsuda")];
        assert_eq!(index(&all)[0].by, "Yasunori Mitsuda");
    }

    /// The first attribution found wins rather than the last, so one badly
    /// tagged track cannot rename the whole game's composer.
    #[test]
    fn a_later_blank_does_not_erase_the_attribution() {
        let all = vec![
            track("/m/a.psf", "Ico", "Michiru Oshima", ""),
            track("/m/b.psf", "Ico", "", ""),
        ];
        assert_eq!(index(&all)[0].by, "Michiru Oshima");
    }

    #[test]
    fn an_empty_library_has_no_games() {
        assert!(index(&[]).is_empty());
    }

    #[test]
    fn asking_for_one_game_gets_only_its_tracks() {
        let all = vec![
            track("/m/a.psf", "Ico", "", ""),
            track("/m/b.psf", "Shadow of the Colossus", "", ""),
            track("/m/c.psf", "Ico", "", ""),
        ];
        let got: Vec<&str> = tracks_of(&all, "Ico").iter().map(|t| t.path.as_str()).collect();
        assert_eq!(got, ["/m/a.psf", "/m/c.psf"]);
    }
}
