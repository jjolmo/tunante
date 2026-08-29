//! Deciding which machine a track came from and which game it belongs to.
//!
//! # Why this exists
//!
//! The old answer was "look at the extension, and if that fails look at the
//! grandparent folder and hope every track under it agrees". Measured against a
//! real library of 23,281 files that named the right console for 71% of them.
//! The same measurement with the rule in this module reaches 93%.
//!
//! The two things the old rule could not do:
//!
//! - **Depth.** `NDS/Rhythm Heaven (NDS)/Disc 1/track.mp3` has `Rhythm Heaven`
//!   as its grandparent, not `NDS`, and no chiptune file shares that
//!   grandparent — so nothing under it was ever classified. 3,379 files.
//! - **Consoles with no chiptune format.** A `PC/`, `Switch/`, `PS4/` or `psp/`
//!   folder is all `.mp3`. There is no extension to infer from, so the old rule
//!   returned nothing and the caller fell back to searching iTunes for an
//!   album, which is not a thing that finds game box art.
//!
//! Both are answered by the same observation: a library laid out as
//! `<root>/<console>/<game>/…` is *telling you* the answer in the path, and the
//! path is known without opening a single file. This module reads it, relative
//! to the roots the user actually registered, so the rule does not accidentally
//! fire on `/home/<user>/…` segments that happen to look like a console name.
//!
//! # What is not automatic
//!
//! Some top-level folders are franchises, not machines — `Megaten/` spans five
//! consoles, `Pokemon/` spans four, `OCRemixes/` spans everything. There is no
//! rule that gets those right, and inventing one would mean being confidently
//! wrong. They resolve to nothing, and the user flags them. That is what
//! `classification_overrides` is for, and an override always wins.

use crate::console::{self, Console};
use std::collections::HashMap;

/// Where a console id came from. Recorded so the UI can show what it inferred
/// and why, and so a wrong answer is debuggable without re-running the guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleSource {
    /// The user said so, for this exact track.
    TrackOverride,
    /// The user said so, for a folder this track is under.
    FolderOverride,
    /// The extension names the machine on its own.
    StrongCodec,
    /// The first path segment below a registered root is a console's name.
    RootSegment,
    /// The extension does not name a machine, but has an obvious default.
    WeakCodec,
    /// Nothing said anything.
    Unknown,
}

/// Where a game name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSource {
    TrackOverride,
    FolderOverride,
    /// The `album` tag. For rips this is the ripper's own answer, and it is the
    /// best one available: SPC ID666 headers carry the game title, so a folder
    /// abbreviated to `ct/` still yields "Chrono Trigger".
    AlbumTag,
    /// A folder between the root and the file.
    Folder,
    /// Nothing else was left.
    FileName,
}

impl ConsoleSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ConsoleSource::TrackOverride => "track_override",
            ConsoleSource::FolderOverride => "folder_override",
            ConsoleSource::StrongCodec => "strong_codec",
            ConsoleSource::RootSegment => "root_segment",
            ConsoleSource::WeakCodec => "weak_codec",
            ConsoleSource::Unknown => "unknown",
        }
    }
}

impl GameSource {
    pub fn as_str(self) -> &'static str {
        match self {
            GameSource::TrackOverride => "track_override",
            GameSource::FolderOverride => "folder_override",
            GameSource::AlbumTag => "album_tag",
            GameSource::Folder => "folder",
            GameSource::FileName => "file_name",
        }
    }
}

/// What a track was decided to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    /// `None` when nothing could name a machine. An honest blank, not "Otros".
    pub console_id: Option<&'static str>,
    pub console_source: ConsoleSource,
    /// Never empty: there is always a filename to fall back to.
    pub game: String,
    pub game_source: GameSource,
}

/// A user's correction. Either half may be `None`, meaning "leave that one to
/// the rules" — flagging `Megaten/Persona 5` as PS4 should not also freeze the
/// game name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Override {
    pub console_id: Option<String>,
    pub game_name: Option<String>,
}

/// The rules, plus the state they need: which roots are registered and what the
/// user has corrected.
///
/// Built once and reused across a whole library pass. Cheap to construct and
/// immutable afterwards.
pub struct Classifier {
    /// Normalised, longest first, so the most specific root wins when one root
    /// is nested inside another.
    roots: Vec<String>,
    track_overrides: HashMap<String, Override>,
    folder_overrides: HashMap<String, Override>,
}

/// Strip a trailing separator and any `#subsong` suffix, and unify separators.
///
/// Everything that compares paths goes through this. An override stored with a
/// trailing slash would otherwise match nothing, silently.
pub fn normalize_path(path: &str) -> String {
    let real = crate::vgm_path::parse_vgm_path(path).0;
    let unified: String = real.chars().map(|c| if c == '\\' { '/' } else { c }).collect();
    let trimmed = unified.trim_end_matches('/');
    if trimmed.is_empty() && unified.starts_with('/') {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Is this folder a disc or bonus subdivision rather than a game?
///
/// `Disc 1`, `CD2`, `Vol. 3`, `Bonus` — and, importantly, **`Disc 2 - Blazing
/// Stars`**. Discs very often carry a title of their own, and an earlier version
/// of this only recognised the bare form. The consequence was not a cosmetic
/// one: `Disc 1 - Fairytale of the Isles`, `Disc 2 - Blazing Stars` and
/// `Disc 3 - Roar of the Formidable` were each filed as a separate *game*, so
/// three thirds of one Genshin Impact soundtrack went looking for three
/// different covers. Nineteen folders in one real library were affected.
///
/// A multi-disc rip belongs under its game, and "Disc 1" as a game name would
/// collide across every multi-disc rip in the collection.
fn is_disc_folder(name: &str) -> bool {
    let n = console::normalize_segment(name);
    if matches!(n.as_str(), "bonus" | "extras" | "extra" | "bonus tracks" | "bonus disc") {
        return true;
    }
    let mut tokens = n.split(' ').filter(|t| !t.is_empty());
    let Some(first) = tokens.next() else { return false };

    // `Disc1` normalises to one token, `Disc 1` to two. Split a fused one.
    let split = first.find(|c: char| c.is_ascii_digit()).unwrap_or(first.len());
    let (word, fused_number) = first.split_at(split);
    let word = if word.is_empty() { first } else { word };
    if !matches!(word, "disc" | "disk" | "cd" | "dvd" | "vol" | "volume") {
        return false;
    }

    let number = if fused_number.is_empty() { tokens.next().unwrap_or("") } else { fused_number };
    // A plausible disc number, and nothing else. The upper bound is what keeps
    // a `CD32` folder — the Commodore machine — from being read as disc 32.
    // Whatever follows the number is the disc's own title and is ignored.
    number
        .parse::<u32>()
        .is_ok_and(|n| (1..=20).contains(&n))
}

/// Strip the noise commonly found in game folder and album names so downstream
/// lookups actually match.
///
/// Removes parenthesised metadata (`(1987)(Nintendo)`, `(USA)`), bracketed
/// alternate titles (`[Estpolis Denki II]`), curly annotations, and the stray
/// punctuation left behind. The real case this was written for:
/// `Lufia II - Rise of the Sinistrals [Estpolis Denki II] [Lufia] (1995)(Neverland)(Taito)`.
///
/// Depth-tracked rather than regex-based so nested groups do not leave a
/// dangling bracket.
pub fn sanitize_game_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let (mut paren, mut bracket, mut curly) = (0i32, 0i32, 0i32);
    for ch in raw.chars() {
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' => curly += 1,
            '}' => curly = (curly - 1).max(0),
            _ if paren == 0 && bracket == 0 && curly == 0 => out.push(ch),
            _ => {}
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim_matches(|c: char| c.is_whitespace() || c == '-' || c == ',' || c == '_' || c == '.')
        .to_string()
}

/// Sanitize, but never to nothing: a folder called `(1995)` keeps its raw name
/// rather than becoming an empty game.
fn sanitize_or_raw(raw: &str) -> String {
    let clean = sanitize_game_name(raw);
    if clean.is_empty() {
        raw.trim().to_string()
    } else {
        clean
    }
}

impl Classifier {
    /// `roots` are the registered library folders — see `monitored_folders`.
    pub fn new(
        roots: Vec<String>,
        track_overrides: HashMap<String, Override>,
        folder_overrides: HashMap<String, Override>,
    ) -> Self {
        let mut roots: Vec<String> = roots.iter().map(|r| normalize_path(r)).collect();
        roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
        roots.dedup();
        Self {
            roots,
            track_overrides: track_overrides
                .into_iter()
                .map(|(k, v)| (normalize_path(&k), v))
                .collect(),
            folder_overrides: folder_overrides
                .into_iter()
                .map(|(k, v)| (normalize_path(&k), v))
                .collect(),
        }
    }

    /// A classifier that knows no roots and no corrections. Extension-only.
    pub fn bare() -> Self {
        Self::new(Vec::new(), HashMap::new(), HashMap::new())
    }

    /// The longest registered root this path lives under, if any.
    fn root_of(&self, path: &str) -> Option<&str> {
        self.roots
            .iter()
            .find(|r| path.len() > r.len() && path.starts_with(r.as_str()) && path.as_bytes()[r.len()] == b'/')
            .map(|r| r.as_str())
    }

    /// The directory components between the root (or the filesystem root) and
    /// the file itself.
    fn dirs_below_root<'a>(&self, path: &'a str, root: Option<&str>) -> Vec<&'a str> {
        let rest = match root {
            Some(r) => &path[r.len() + 1..],
            None => path.trim_start_matches('/'),
        };
        let mut parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        parts.pop(); // the filename
        parts
    }

    /// Every folder override that applies to this path, nearest ancestor first.
    fn folder_override_for(&self, path: &str) -> Option<&Override> {
        let mut cursor = path;
        while let Some(idx) = cursor.rfind('/') {
            cursor = &cursor[..idx];
            if cursor.is_empty() {
                break;
            }
            if let Some(o) = self.folder_overrides.get(cursor) {
                return Some(o);
            }
        }
        None
    }

    /// Decide what a track is.
    ///
    /// `album` is the track's album tag and `codec` its extension or the
    /// database's `codec` column; either spelling works.
    pub fn classify(&self, path: &str, album: &str, codec: &str) -> Classification {
        let path = normalize_path(path);
        let track_override = self.track_overrides.get(&path);
        let folder_override = self.folder_override_for(&path);
        let root = self.root_of(&path);
        let dirs = self.dirs_below_root(&path, root);

        let ext = if codec.trim().is_empty() {
            console::extension_of(&path)
        } else {
            codec.trim().to_ascii_lowercase()
        };

        // Which console, and why.
        //
        // Every segment between the root and the file is examined, not just the
        // first. The first version only looked at `dirs[0]`, which assumed the
        // registered root *is* the folder holding the console folders. A real
        // library does not oblige: with the root at `…/Musica` and the music at
        // `…/Musica/OST juegos/Gba/riviera ost/…`, segment 0 is `OST juegos`,
        // which names no machine, and everything below it fell to Unknown —
        // hundreds of tracks in folders whose name says `Gba` in plain sight.
        //
        // First match from the root wins, not the nearest to the file: an
        // arrangement folder under `SNES/Chrono Trigger/PS1 arrangement/` is
        // filed under the machine the rip belongs to, not the one named last.
        //
        // Gated on there being a registered root. Outside one, `dirs` is every
        // component of an absolute path, and scanning all of them starts
        // matching the filesystem instead of the library: `/home/pc/Downloads/`
        // would file a track under "PC". Looking only at the first segment used
        // to hide that by accident.
        let segment_hit: Option<(usize, &'static Console)> = root.and_then(|_| {
            dirs.iter()
                .enumerate()
                .find_map(|(i, s)| console::by_folder_segment(s).map(|c| (i, c)))
        });
        let segment_console: Option<&'static Console> = segment_hit.map(|(_, c)| c);

        let (console_id, console_source) = if let Some(id) =
            track_override.and_then(|o| o.console_id.as_deref())
        {
            (console::by_id(id).map(|c| c.id), ConsoleSource::TrackOverride)
        } else if let Some(id) = folder_override.and_then(|o| o.console_id.as_deref()) {
            (console::by_id(id).map(|c| c.id), ConsoleSource::FolderOverride)
        } else if let Some(c) = console::by_codec(&ext) {
            (Some(c.id), ConsoleSource::StrongCodec)
        } else if let Some(c) = segment_console {
            (Some(c.id), ConsoleSource::RootSegment)
        } else if let Some(c) = console::by_weak_codec(&ext) {
            (Some(c.id), ConsoleSource::WeakCodec)
        } else {
            (None, ConsoleSource::Unknown)
        };

        let (game, game_source) =
            self.game_of(&path, album, root, &dirs, segment_hit.map(|(i, _)| i));

        Classification { console_id, console_source, game, game_source }
    }

    fn game_of(
        &self,
        path: &str,
        album: &str,
        root: Option<&str>,
        dirs: &[&str],
        console_at: Option<usize>,
    ) -> (String, GameSource) {
        if let Some(name) = self
            .track_overrides
            .get(path)
            .and_then(|o| o.game_name.as_deref())
            .filter(|n| !n.trim().is_empty())
        {
            return (name.trim().to_string(), GameSource::TrackOverride);
        }
        if let Some(name) = self
            .folder_override_for(path)
            .and_then(|o| o.game_name.as_deref())
            .filter(|n| !n.trim().is_empty())
        {
            return (name.trim().to_string(), GameSource::FolderOverride);
        }
        // The ripper's own answer, and the reason abbreviated folders still work.
        if !album.trim().is_empty() {
            return (sanitize_or_raw(album), GameSource::AlbumTag);
        }

        let had_dirs = !dirs.is_empty();
        let mut candidates: &[&str] = dirs;
        // A trailing `Disc 2` is a subdivision of the game, not the game.
        while let Some(last) = candidates.last() {
            if is_disc_folder(last) {
                candidates = &candidates[..candidates.len() - 1];
            } else {
                break;
            }
        }

        // Everything between the root and the file was a disc folder, so the
        // root itself is the game. This is the "user registered one game's
        // folder" layout, and it is the *only* reason to name a track after the
        // root — a file loose in a console folder must not become "OST juegos".
        if had_dirs && candidates.is_empty() {
            if let Some(base) = root.and_then(|r| r.rsplit('/').next()).filter(|b| !b.is_empty()) {
                return (sanitize_or_raw(base), GameSource::Folder);
            }
        }

        // The console folder names the machine and is never the game, and
        // neither is anything above it — `OST juegos` is a filing cabinet, not a
        // title. Drop everything up to and including it; being left with nothing
        // means there was no game folder at all.
        if let Some(i) = console_at {
            candidates = candidates.get(i + 1..).unwrap_or(&[]);
        }

        if let Some(name) = candidates.last() {
            return (sanitize_or_raw(name), GameSource::Folder);
        }
        let file = path.rsplit('/').next().unwrap_or(path);
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        (sanitize_or_raw(stem), GameSource::FileName)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/media/storage/Musica/OST juegos";

    /// The layout that exposed the bug: the registered root sits *above* the
    /// folder that holds the console folders.
    const DEEP_ROOT: &str = "/media/storage/Musica";

    #[test]
    fn the_console_folder_need_not_be_the_first_segment() {
        let c = Classifier::new(vec![DEEP_ROOT.to_string()], HashMap::new(), HashMap::new());
        let r = c.classify(
            "/media/storage/Musica/OST juegos/Gba/riviera ost/101 Overture.mp3",
            "",
            "mp3",
        );
        assert_eq!(r.console_id, Some("gba"));
        assert_eq!(r.console_source, ConsoleSource::RootSegment);
        assert_eq!(r.game, "riviera ost");
    }

    #[test]
    fn the_filing_cabinet_above_the_console_is_never_the_game() {
        let c = Classifier::new(vec![DEEP_ROOT.to_string()], HashMap::new(), HashMap::new());
        let r = c.classify(
            "/media/storage/Musica/OST juegos/PSX/Tales of Symphonia/BGM_B000.ADX",
            "",
            "adx",
        );
        assert_eq!(r.console_id, Some("ps1"));
        assert_eq!(r.game, "Tales of Symphonia", "must not be `PSX` or `OST juegos`");
    }

    #[test]
    fn a_disc_folder_still_resolves_under_a_deep_root() {
        let c = Classifier::new(vec![DEEP_ROOT.to_string()], HashMap::new(), HashMap::new());
        let r = c.classify(
            "/media/storage/Musica/OST juegos/NDS/Rhythm Heaven/Disc 1/01.mp3",
            "",
            "mp3",
        );
        assert_eq!(r.console_id, Some("nds"));
        assert_eq!(r.game, "Rhythm Heaven");
    }

    /// First match from the root, not the nearest to the file: the rip belongs
    /// to the machine it was ripped from.
    #[test]
    fn the_first_console_segment_from_the_root_wins() {
        let c = Classifier::new(vec![DEEP_ROOT.to_string()], HashMap::new(), HashMap::new());
        let r = c.classify(
            "/media/storage/Musica/OST juegos/SNES/Chrono Trigger/PSX arrangement/01.mp3",
            "",
            "mp3",
        );
        assert_eq!(r.console_id, Some("snes"));
    }

    /// A file loose in the console folder has no game folder at all, and must
    /// not be named after the console or after the cabinet above it.
    #[test]
    fn a_loose_file_in_a_console_folder_falls_back_to_the_file_name() {
        let c = Classifier::new(vec![DEEP_ROOT.to_string()], HashMap::new(), HashMap::new());
        let r = c.classify("/media/storage/Musica/OST juegos/Gba/some tune.mp3", "", "mp3");
        assert_eq!(r.console_id, Some("gba"));
        assert_eq!(r.game, "some tune");
    }

    fn plain() -> Classifier {
        Classifier::new(vec![ROOT.to_string()], HashMap::new(), HashMap::new())
    }

    fn classify(path: &str, album: &str, codec: &str) -> Classification {
        plain().classify(path, album, codec)
    }

    // ---- console ----

    /// The rule the whole module exists for: the folder above the game names
    /// the machine, at any depth, for formats that name nothing themselves.
    #[test]
    fn the_console_folder_names_the_machine() {
        let c = classify(&format!("{ROOT}/PSX/Ape Escape/01.mp3"), "", "MP3");
        assert_eq!(c.console_id, Some("ps1"));
        assert_eq!(c.console_source, ConsoleSource::RootSegment);
    }

    /// The depth-4 case that the old grandparent rule could not see. 3,379
    /// files in the measured library.
    #[test]
    fn a_disc_folder_does_not_hide_the_console() {
        let c = classify(&format!("{ROOT}/NDS/Rhythm Heaven (NDS)/Disc 1/Remix 2.mp3"), "", "MP3");
        assert_eq!(c.console_id, Some("nds"));
        assert_eq!(c.game, "Rhythm Heaven");
    }

    /// An extension that names its machine outranks the folder it was filed in.
    /// An SPC is a SNES rip wherever it sits.
    #[test]
    fn a_strong_extension_beats_the_folder_it_was_misfiled_in() {
        let c = classify(&format!("{ROOT}/PSX/Whatever/01.spc"), "", "SPC");
        assert_eq!(c.console_id, Some("snes"));
        assert_eq!(c.console_source, ConsoleSource::StrongCodec);
    }

    /// And the converse, which is why the split exists: a `.vgm` names no
    /// machine, so the folder decides. Measured: 31 real files need this.
    #[test]
    fn the_folder_beats_an_extension_that_names_no_machine() {
        let c = classify(&format!("{ROOT}/sms/Wonder Boy/01.vgm"), "", "VGM");
        assert_eq!(c.console_id, Some("mastersystem"));
        assert_eq!(c.console_source, ConsoleSource::RootSegment);
    }

    #[test]
    fn a_weak_extension_still_answers_when_the_folder_does_not() {
        let c = classify(&format!("{ROOT}/Megaten/Whatever/01.vgm"), "", "VGM");
        assert_eq!(c.console_id, Some("genesis"));
        assert_eq!(c.console_source, ConsoleSource::WeakCodec);
    }

    /// A franchise folder is not a machine, and pretending otherwise would be
    /// confidently wrong for four of the five consoles it spans.
    #[test]
    fn a_franchise_folder_leaves_the_console_blank() {
        let c = classify(&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3");
        assert_eq!(c.console_id, None);
        assert_eq!(c.console_source, ConsoleSource::Unknown);
        // ...but the game is still right, which is what makes it flaggable.
        assert_eq!(c.game, "Persona 5");
    }

    /// The segment rule must only fire below a registered root. Otherwise a
    /// stray `/home/pc/...` would classify every file on the machine.
    #[test]
    fn the_segment_rule_does_not_fire_outside_a_registered_root() {
        let c = classify("/home/pc/Downloads/song.mp3", "", "MP3");
        assert_eq!(c.console_id, None);
    }

    #[test]
    fn the_most_specific_root_wins() {
        let c = Classifier::new(
            vec![ROOT.to_string(), format!("{ROOT}/Megaten")],
            HashMap::new(),
            HashMap::new(),
        );
        // Under the inner root, `Persona 5` is segment 0 and is not a console.
        let got = c.classify(&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3");
        assert_eq!(got.console_id, None);
        assert_eq!(got.game, "Persona 5");
    }

    // ---- overrides ----

    #[test]
    fn a_folder_override_flags_a_whole_franchise_subtree() {
        let mut folders = HashMap::new();
        folders.insert(
            format!("{ROOT}/Megaten/Persona 5"),
            Override { console_id: Some("ps4".into()), game_name: None },
        );
        let c = Classifier::new(vec![ROOT.to_string()], HashMap::new(), folders);
        let got = c.classify(&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3");
        assert_eq!(got.console_id, Some("ps4"));
        assert_eq!(got.console_source, ConsoleSource::FolderOverride);
        // The blank half was left to the rules, not frozen.
        assert_eq!(got.game, "Persona 5");
        assert_eq!(got.game_source, GameSource::Folder);
    }

    /// The nearest ancestor wins, so a correction on one game does not have to
    /// fight the correction on the franchise above it.
    #[test]
    fn the_nearest_folder_override_wins() {
        let mut folders = HashMap::new();
        folders.insert(
            format!("{ROOT}/Megaten"),
            Override { console_id: Some("ps2".into()), game_name: None },
        );
        folders.insert(
            format!("{ROOT}/Megaten/Persona 5"),
            Override { console_id: Some("ps4".into()), game_name: None },
        );
        let c = Classifier::new(vec![ROOT.to_string()], HashMap::new(), folders);
        assert_eq!(
            c.classify(&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3").console_id,
            Some("ps4")
        );
        assert_eq!(
            c.classify(&format!("{ROOT}/Megaten/Persona 3/01.mp3"), "", "MP3").console_id,
            Some("ps2")
        );
    }

    /// A correction beats even an extension that names its own machine — the
    /// user is allowed to be right about a mislabelled rip.
    #[test]
    fn a_track_override_beats_everything() {
        let mut tracks = HashMap::new();
        tracks.insert(
            format!("{ROOT}/PSX/Foo/01.spc"),
            Override { console_id: Some("gamegear".into()), game_name: Some("Whatever".into()) },
        );
        let c = Classifier::new(vec![ROOT.to_string()], tracks, HashMap::new());
        let got = c.classify(&format!("{ROOT}/PSX/Foo/01.spc"), "Ignored", "SPC");
        assert_eq!(got.console_id, Some("gamegear"));
        assert_eq!(got.game, "Whatever");
    }

    /// An override naming a console that no longer exists must not resurrect it.
    #[test]
    fn an_override_naming_an_unknown_console_resolves_to_nothing() {
        let mut folders = HashMap::new();
        folders.insert(
            format!("{ROOT}/Megaten"),
            Override { console_id: Some("virtualboy".into()), game_name: None },
        );
        let c = Classifier::new(vec![ROOT.to_string()], HashMap::new(), folders);
        assert_eq!(c.classify(&format!("{ROOT}/Megaten/x/01.mp3"), "", "MP3").console_id, None);
    }

    /// Stored with a trailing slash, matched without one. Getting this wrong
    /// makes an override silently do nothing.
    #[test]
    fn an_override_stored_with_a_trailing_slash_still_matches() {
        let mut folders = HashMap::new();
        folders.insert(
            format!("{ROOT}/Megaten/"),
            Override { console_id: Some("ps2".into()), game_name: None },
        );
        let c = Classifier::new(vec![ROOT.to_string()], HashMap::new(), folders);
        assert_eq!(
            c.classify(&format!("{ROOT}/Megaten/Persona 3/01.mp3"), "", "MP3").console_id,
            Some("ps2")
        );
    }

    /// A folder override keyed at `.../Persona` must not capture `.../Persona 5`.
    #[test]
    fn a_folder_override_does_not_match_a_sibling_by_prefix() {
        let mut folders = HashMap::new();
        folders.insert(
            format!("{ROOT}/Megaten/Persona"),
            Override { console_id: Some("ps2".into()), game_name: None },
        );
        let c = Classifier::new(vec![ROOT.to_string()], HashMap::new(), folders);
        assert_eq!(
            c.classify(&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3").console_id,
            None
        );
    }

    // ---- game ----

    /// The ID666 rescue. A folder abbreviated to `ct/` tells you nothing, but
    /// the SPC header inside says "Chrono Trigger" and the reader already puts
    /// that in `album`.
    #[test]
    fn the_album_tag_rescues_an_abbreviated_folder() {
        let c = classify(&format!("{ROOT}/snes spc osts/ct/01.spc"), "Chrono Trigger", "SPC");
        assert_eq!(c.game, "Chrono Trigger");
        assert_eq!(c.game_source, GameSource::AlbumTag);
        assert_eq!(c.console_id, Some("snes"));
    }

    #[test]
    fn without_a_tag_the_game_folder_names_it() {
        let c = classify(&format!("{ROOT}/PSX/Ape Escape/01.mp3"), "", "MP3");
        assert_eq!(c.game, "Ape Escape");
        assert_eq!(c.game_source, GameSource::Folder);
    }

    #[test]
    fn whitespace_is_not_a_tag() {
        assert_eq!(classify(&format!("{ROOT}/PSX/Grandia/01.mp3"), "   ", "MP3").game, "Grandia");
    }

    /// The console folder is never the game, even when the game folder is
    /// missing entirely.
    #[test]
    fn a_file_loose_in_a_console_folder_is_not_named_after_the_console() {
        let c = classify(&format!("{ROOT}/NES/Solstice Theme.nsf"), "", "NSF");
        assert_ne!(c.game, "NES");
        assert_eq!(c.game, "Solstice Theme");
        assert_eq!(c.game_source, GameSource::FileName);
    }

    /// A file loose at the very top of the library has only its own name.
    #[test]
    fn a_loose_file_at_the_root_falls_back_to_its_filename() {
        let c = classify(&format!("{ROOT}/Tetris Plus - BGM 18.mp3"), "", "MP3");
        assert_eq!(c.game, "Tetris Plus - BGM 18");
        assert_eq!(c.game_source, GameSource::FileName);
    }

    /// When the user registers a single game's folder as the root, the discs
    /// under it still belong to that game.
    #[test]
    fn a_root_that_is_itself_a_game_names_the_game() {
        let c = Classifier::new(
            vec!["/music/Chrono Cross OST".to_string()],
            HashMap::new(),
            HashMap::new(),
        );
        let got = c.classify("/music/Chrono Cross OST/Disc 2/05.flac", "", "FLAC");
        assert_eq!(got.game, "Chrono Cross OST");
    }

    #[test]
    fn the_disc_folders_that_have_to_be_recognised() {
        for d in ["Disc 1", "disc 2", "CD1", "cd 3", "DISK 2", "Vol. 4", "Bonus", "extras"] {
            assert!(is_disc_folder(d), "{d:?} should be a disc folder");
        }
    }

    /// A disc usually has a name of its own, and missing that split one
    /// soundtrack into three "games" that each went hunting for a cover.
    /// All of these are real folder names.
    #[test]
    fn a_disc_with_a_title_of_its_own_is_still_a_disc() {
        for d in [
            "Disc 1 - Fairytale of the Isles",
            "Disc 2 - Blazing Stars",
            "Disc 3 - Roar of the Formidable",
            "Disc1 - V-Rock",
            "Disc 4 - Voice Actors Round Table Talk & Audio Commentary",
            "Disc 3 - Bonus Tracks",
            "CD2 - Krematoa",
        ] {
            assert!(is_disc_folder(d), "{d:?} should be a disc folder");
        }
    }

    /// A game whose name merely starts with one of those words is not a disc.
    #[test]
    fn a_game_is_not_a_disc_folder() {
        for d in [
            "Discworld",
            "Disc Jam",
            "CD Shooter",
            "Volume",
            "Bonus Level Zero",
            // The Commodore machine, not disc thirty-two. This is why the
            // number is bounded.
            "CD32",
            "Disc Room",
        ] {
            assert!(!is_disc_folder(d), "{d:?} should not be a disc folder");
        }
    }

    /// The whole point: three titled discs of one soundtrack are one game.
    #[test]
    fn titled_discs_of_one_soundtrack_are_one_game() {
        let names: Vec<String> = ["Disc 1 - Fairytale of the Isles", "Disc 2 - Blazing Stars"]
            .iter()
            .map(|d| {
                classify(&format!("{ROOT}/PC/Genshin Impact/{d}/01.mp3"), "", "MP3").game
            })
            .collect();
        assert_eq!(names, ["Genshin Impact", "Genshin Impact"]);
    }

    /// A subsong address addresses several tracks in one file and is not part
    /// of any name or path.
    #[test]
    fn a_subsong_suffix_leaks_into_nothing() {
        let c = classify(&format!("{ROOT}/GB/Pokemon Blue/pokemon.gbs#7"), "", "");
        assert_eq!(c.console_id, Some("gameboy"));
        assert_eq!(c.game, "Pokemon Blue");
    }

    /// The codec column is optional; the path carries the same information.
    #[test]
    fn an_absent_codec_is_read_from_the_path() {
        let c = classify(&format!("{ROOT}/Whatever/Foo/01.spc"), "", "");
        assert_eq!(c.console_id, Some("snes"));
    }

    // ---- sanitize ----

    #[test]
    fn the_real_name_this_sanitiser_was_written_for() {
        assert_eq!(
            sanitize_game_name(
                "Lufia II - Rise of the Sinistrals [Estpolis Denki II] [Lufia] (1995)(Neverland)(Taito)"
            ),
            "Lufia II - Rise of the Sinistrals"
        );
    }

    #[test]
    fn a_region_tag_is_not_part_of_the_name() {
        assert_eq!(sanitize_game_name("Rhythm Heaven (NDS)"), "Rhythm Heaven");
        assert_eq!(sanitize_game_name("Celeste Original Soundtrack [MP3]"), "Celeste Original Soundtrack");
    }

    /// Sanitising must never leave a game with no name at all.
    #[test]
    fn a_name_that_is_all_annotation_keeps_its_raw_form() {
        assert_eq!(sanitize_game_name("(1995)"), "");
        assert_eq!(sanitize_or_raw("(1995)"), "(1995)");
        let c = classify(&format!("{ROOT}/PC/(1995)/01.mp3"), "", "MP3");
        assert_eq!(c.game, "(1995)");
    }

    #[test]
    fn a_classification_always_names_a_game() {
        for path in [
            format!("{ROOT}/PSX/Foo/01.psf"),
            format!("{ROOT}/loose.mp3"),
            "/nowhere/x.mp3".to_string(),
            "relative.mp3".to_string(),
        ] {
            assert!(!classify(&path, "", "").game.is_empty(), "{path} produced no game");
        }
    }
}
