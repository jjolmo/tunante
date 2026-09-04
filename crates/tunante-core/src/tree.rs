//! Deriving a browsable tree from the paths the library already knows.
//!
//! Not from `read_dir`. `tunante` walks the disk for its tree, which is
//! honest and needs the files to still be mounted; this builds the same shape
//! out of what was scanned, so the library still browses with the SD card out
//! and a folder that vanished still shows until the next scan prunes it.
//!
//! Pure string work on purpose: it is the fiddliest part of the library screen
//! and the only part that can be tested without a phone.

/// A folder in the tree, with enough to draw a row or a tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub path: String,
    /// What to show. See [`level`] for why this is not always the last
    /// component of `path`.
    pub name: String,
    pub count: usize,
    /// A track inside it, for the cover. A folder has no art of its own.
    pub first_track: String,
}

/// One level of the tree: the folders under `parent`, and the tracks in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Level {
    pub folders: Vec<Branch>,
    pub here: Vec<String>,
}

/// The directory a track path lives in, ignoring any `#subsong` suffix.
fn dir_of(path: &str) -> &str {
    let file = path.split('#').next().unwrap_or(path);
    match file.rfind('/') {
        Some(0) => "/",
        Some(i) => &file[..i],
        None => "",
    }
}

/// The longest directory prefix every one of `dirs` starts with.
///
/// Compared component by component rather than by bytes: `/Music/Sonic` and
/// `/Music/Sonic 2` share the byte prefix `/Music/Sonic`, which is a real
/// directory and the wrong answer — it would hide `Sonic 2` inside `Sonic`.
fn common_ancestor(dirs: &[&str]) -> String {
    let Some(first) = dirs.first() else {
        return String::new();
    };
    let mut shared: Vec<&str> = first.split('/').collect();
    for d in &dirs[1..] {
        let parts: Vec<&str> = d.split('/').collect();
        let keep = shared
            .iter()
            .zip(parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        shared.truncate(keep);
    }
    shared.join("/")
}

/// Build one level of the tree from every track path in the library.
///
/// An empty `parent` asks for the roots, and the roots are **one level below
/// whatever all the tracks have in common** — not every directory that happens
/// to contain a file. A library under `Music/Rock/Beatles/Abbey Road` should
/// open on `Rock`, not on `Abbey Road`; showing the leaves flat also makes two
/// folders called `Disc 1` in different albums indistinguishable, because all
/// a row has to show is the last component.
pub fn level(track_paths: &[String], parent: &str) -> Level {
    let dirs: Vec<&str> = track_paths.iter().map(|p| dir_of(p)).collect();

    let base = if parent.is_empty() {
        common_ancestor(&dirs)
    } else {
        parent.to_string()
    };
    // The prefix a child of `base` starts with. `/` is its own parent, so it
    // must not become `//`.
    let prefix = if base == "/" { "/".to_string() } else { format!("{base}/") };

    let mut folders: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    let mut here = Vec::new();

    for (path, dir) in track_paths.iter().zip(dirs.iter()) {
        if *dir == base {
            here.push(path.clone());
            continue;
        }
        let Some(rest) = dir.strip_prefix(&prefix) else {
            continue;
        };
        let child = rest.split('/').next().unwrap_or(rest);
        if child.is_empty() {
            continue;
        }
        let entry = folders
            .entry(format!("{prefix}{child}"))
            .or_insert((0, path.clone()));
        entry.0 += 1;
    }

    Level {
        folders: folders
            .into_iter()
            .map(|(path, (count, first_track))| {
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                Branch { path, name, count, first_track }
            })
            .collect(),
        here,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_flat_library_opens_on_its_folders() {
        let p = paths(&["/m/Zelda/a.psf", "/m/Zelda/b.psf", "/m/Sonic/c.psf"]);
        let l = level(&p, "");
        assert_eq!(
            l.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["Sonic", "Zelda"],
            "sorted, so the same library always looks the same"
        );
        assert_eq!(l.folders[1].count, 2);
        assert!(l.here.is_empty());
    }

    /// The bug this module exists to fix.
    #[test]
    fn a_nested_library_opens_on_the_top_level_not_on_the_leaves() {
        let p = paths(&[
            "/m/Rock/Beatles/Abbey Road/1.mp3",
            "/m/Rock/Beatles/Revolver/1.mp3",
            "/m/Chip/Zelda/1.psf",
        ]);
        let l = level(&p, "");
        assert_eq!(
            l.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["Chip", "Rock"],
            "the roots are one level below what everything shares, not every \
             directory that happens to hold a file"
        );
        assert_eq!(l.folders[1].count, 2, "a branch counts everything beneath it");
    }

    /// Two folders whose names share a prefix are two folders.
    #[test]
    fn a_sibling_is_not_swallowed_by_its_shorter_neighbour() {
        let p = paths(&["/m/Sonic/a.psf", "/m/Sonic 2/b.psf"]);
        let l = level(&p, "/m");
        assert_eq!(
            l.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["Sonic", "Sonic 2"],
        );
    }

    /// And the same thing one level up, where the common ancestor is computed.
    #[test]
    fn the_common_ancestor_is_a_whole_directory_not_a_byte_prefix() {
        assert_eq!(common_ancestor(&["/m/Sonic", "/m/Sonic 2"]), "/m");
        assert_eq!(common_ancestor(&["/m/a/b", "/m/a/c"]), "/m/a");
        assert_eq!(common_ancestor(&["/m/a"]), "/m/a");
    }

    #[test]
    fn descending_shows_the_tracks_that_live_there() {
        let p = paths(&[
            "/m/Zelda/a.psf",
            "/m/Zelda/OST/b.psf",
        ]);
        let l = level(&p, "/m/Zelda");
        assert_eq!(l.here, ["/m/Zelda/a.psf"], "only what is directly in it");
        assert_eq!(l.folders.len(), 1);
        assert_eq!(l.folders[0].path, "/m/Zelda/OST");
    }

    /// A subsong address is several tracks over one file, and `#3` is not a
    /// directory.
    #[test]
    fn a_subsong_suffix_is_not_part_of_the_path() {
        let p = paths(&["/m/GB/pokemon.gbs#1", "/m/GB/pokemon.gbs#2"]);
        let l = level(&p, "/m/GB");
        assert_eq!(l.here.len(), 2);
        assert!(l.folders.is_empty(), "no folder called `pokemon.gbs#1`");
    }

    #[test]
    fn an_empty_library_is_an_empty_level() {
        assert_eq!(level(&[], ""), Level::default());
    }

    /// One track and nothing else: the ancestor is its own folder, so the
    /// sensible answer is to show the track rather than an empty screen.
    #[test]
    fn a_library_of_one_folder_shows_that_folder_s_tracks() {
        let p = paths(&["/m/Zelda/a.psf"]);
        let l = level(&p, "");
        assert_eq!(l.here, ["/m/Zelda/a.psf"]);
        assert!(l.folders.is_empty());
    }
}
