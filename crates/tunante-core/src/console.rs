//! Which machine a track came from, and everything else the rest of the app
//! needs to know about that machine.
//!
//! # Why this is one table and not four
//!
//! It used to be four. This module held a Spanish name keyed by extension for
//! `tunante` and `tunante-android`; the desktop kept an English one in
//! `consoles.svelte.ts`; the cover-art code kept a third mapping Libretro
//! repository names, keyed on the *display strings minted in TypeScript* — so
//! renaming a label in a `.ts` file silently disabled every box-art lookup; and
//! the GME reader smuggled a fourth into `album_artist`. They disagreed about
//! whether SNES is called "SNES" or "Super Nintendo", about whether GameCube,
//! Wii and 3DS are one bucket or three, and about whether Saturn exists.
//!
//! One table, every consumer reading from it. Display names live here in both
//! languages because that is a translation, not a second opinion.
//!
//! # Strong and weak extensions
//!
//! The old table had one list of extensions and treated it as definitive. That
//! is right for half of them and wrong for the other half, and the split is not
//! a matter of taste:
//!
//! - A `.spc` **is** a SNES rip. It carries an SPC700 register dump; nothing
//!   else produces one. It stays a SNES rip when it is filed under `PSX/`.
//! - A `.vgm` is a log of writes to *some* sound chip. Mega Drive, Master
//!   System, PC Engine, Neo Geo and a dozen arcade boards all produce them.
//!   Measured on a real library: 770 under `Genesis/` and 31 under `sms/`.
//!   Calling every `.vgm` a Mega Drive rip, as the old table did, is wrong 4%
//!   of the time and unfixable from inside the file.
//!
//! So `codecs` names the machine and beats the folder it was filed in;
//! `weak_codecs` is only consulted when the folder had nothing to say. The
//! genuinely ambiguous extensions appear in neither list — `.adx` was measured
//! at 365 under `PSX/`, 132 under `3DS/` and 99 under `wii/`, and there is no
//! majority there worth guessing on.
//!
//! Every extension in `weak_codecs` appears exactly once across the whole
//! table: the entry names the console to assume when nothing else is known,
//! and a second claim on the same extension could never be reached.
//!
//! # The Libretro names are copied, not invented
//!
//! `libretro` holds the thumbnail repository's directory name verbatim, taken
//! from the index at `https://thumbnails.libretro.com/`. They are not
//! guessable: it is `Atari - 8-bit`, not "Atari - 8-bit Family", and
//! `Sega - Master System - Mark III`, not "Sega - Master System". `None` means
//! there is no repository worth asking — the cover-art code then goes straight
//! to its other sources rather than spending a round trip on a certain 404.

use std::path::Path;

/// One machine, and everything any part of the app needs to say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Console {
    /// Stable key. Stored in the database, sent over IPC, never shown to anyone.
    pub id: &'static str,
    /// English display name.
    pub name: &'static str,
    /// Spanish display name.
    pub name_es: &'static str,
    /// Folder names that mean this console, already normalised by
    /// [`normalize_segment`]. See [`by_folder_segment`] for how they match.
    pub aliases: &'static [&'static str],
    /// Extensions that identify this machine on their own. Lowercase.
    pub codecs: &'static [&'static str],
    /// Extensions this machine uses but does not own. Lowercase, and unique
    /// across the whole table.
    pub weak_codecs: &'static [&'static str],
    /// Directory name in the Libretro thumbnail archive, verbatim.
    pub libretro: Option<&'static str>,
    /// Zophar's Domain music section, for the formats that pack a whole game
    /// into one file.
    ///
    /// Only the six consoles whose GME format carries several subsongs — GBS,
    /// NSF/NSFE, HES, KSS, AY, SAP. Everything else is one song per file and
    /// has nowhere to put a mapping, so asking would be asking for nothing.
    ///
    /// Spelled out rather than derived: the slugs follow no pattern
    /// (`nintendo-nes-nsf`, `gameboy-gbs`, `turbografx-16-hes`, `msx2`,
    /// `spectrum`, `atari-8bit`), and guessing one produces a 404 that looks
    /// like "this game is not in the archive".
    pub zophar: Option<&'static str>,
}

/// Every machine the library knows how to name.
pub static CONSOLES: &[Console] = &[
    Console {
        id: "nes",
        name: "NES",
        name_es: "NES",
        aliases: &["nes", "famicom", "fc", "nintendo entertainment system"],
        codecs: &["nsf", "nsfe"],
        weak_codecs: &[],
        libretro: Some("Nintendo - Nintendo Entertainment System"),
        zophar: Some("nintendo-nes-nsf"),
    },
    Console {
        id: "snes",
        name: "SNES",
        name_es: "Super Nintendo",
        // "snes spc osts" is not a typo: alias matching drops trailing tokens,
        // so the folder a real library used lands here on the third try.
        aliases: &["snes", "sfc", "super nintendo", "super famicom"],
        codecs: &["spc"],
        weak_codecs: &[],
        libretro: Some("Nintendo - Super Nintendo Entertainment System"),
        zophar: None,
    },
    Console {
        id: "gameboy",
        name: "Game Boy",
        name_es: "Game Boy",
        aliases: &["gb", "gameboy", "game boy"],
        codecs: &["gbs"],
        weak_codecs: &[],
        libretro: Some("Nintendo - Game Boy"),
        zophar: Some("gameboy-gbs"),
    },
    Console {
        id: "gbc",
        name: "Game Boy Color",
        name_es: "Game Boy Color",
        aliases: &["gbc", "game boy color", "gameboy color"],
        codecs: &[],
        weak_codecs: &[],
        libretro: Some("Nintendo - Game Boy Color"),
        zophar: None,
    },
    Console {
        id: "gba",
        name: "Game Boy Advance",
        name_es: "Game Boy Advance",
        aliases: &["gba", "agb", "game boy advance", "gameboy advance"],
        codecs: &["gsf", "minigsf", "gsflib"],
        weak_codecs: &[],
        libretro: Some("Nintendo - Game Boy Advance"),
        zophar: None,
    },
    Console {
        id: "nds",
        name: "Nintendo DS",
        name_es: "Nintendo DS",
        aliases: &["nds", "ds", "nintendo ds"],
        codecs: &["2sf", "mini2sf", "2sflib", "ncsf", "minincsf"],
        weak_codecs: &["strm"],
        libretro: Some("Nintendo - Nintendo DS"),
        zophar: None,
    },
    Console {
        id: "n3ds",
        name: "Nintendo 3DS",
        name_es: "Nintendo 3DS",
        aliases: &["3ds", "n3ds", "nintendo 3ds"],
        codecs: &[],
        weak_codecs: &["bcstm", "bcwav", "csmp", "cstm"],
        libretro: Some("Nintendo - Nintendo 3DS"),
        zophar: None,
    },
    Console {
        id: "n64",
        name: "Nintendo 64",
        name_es: "Nintendo 64",
        aliases: &["n64", "nintendo 64"],
        codecs: &["usf", "miniusf"],
        weak_codecs: &[],
        libretro: Some("Nintendo - Nintendo 64"),
        zophar: None,
    },
    Console {
        id: "gamecube",
        name: "GameCube",
        name_es: "GameCube",
        aliases: &["gamecube", "gc", "ngc", "game cube"],
        codecs: &[],
        weak_codecs: &["dsp", "idsp", "hps"],
        libretro: Some("Nintendo - GameCube"),
        zophar: None,
    },
    Console {
        id: "wii",
        name: "Wii",
        name_es: "Wii",
        aliases: &["wii"],
        codecs: &[],
        // `.ast` and `.ras` measured overwhelmingly under `wii/`, not GameCube.
        weak_codecs: &["brstm", "brwav", "rwsd", "rwar", "rwav", "ast", "ras"],
        libretro: Some("Nintendo - Wii"),
        zophar: None,
    },
    Console {
        id: "wiiu",
        name: "Wii U",
        name_es: "Wii U",
        aliases: &["wiiu", "wii u"],
        codecs: &[],
        weak_codecs: &["bfstm", "bfwav", "bfsar", "bars"],
        libretro: Some("Nintendo - Wii U"),
        zophar: None,
    },
    Console {
        id: "switch",
        name: "Nintendo Switch",
        name_es: "Nintendo Switch",
        aliases: &["switch", "nsw", "nintendo switch"],
        codecs: &[],
        weak_codecs: &[],
        // Libretro has no Switch archive, and will not.
        libretro: None,
        zophar: None,
    },
    Console {
        id: "mastersystem",
        name: "Master System",
        name_es: "Master System",
        aliases: &["sms", "master system", "sega master system", "mark iii"],
        codecs: &[],
        weak_codecs: &[],
        libretro: Some("Sega - Master System - Mark III"),
        zophar: None,
    },
    Console {
        id: "genesis",
        name: "Mega Drive",
        name_es: "Mega Drive",
        aliases: &["genesis", "md", "megadrive", "mega drive", "sega genesis", "sega mega drive"],
        codecs: &[],
        weak_codecs: &["vgm", "vgz", "gym"],
        libretro: Some("Sega - Mega Drive - Genesis"),
        zophar: None,
    },
    Console {
        id: "gamegear",
        name: "Game Gear",
        name_es: "Game Gear",
        aliases: &["gg", "game gear"],
        codecs: &[],
        weak_codecs: &[],
        libretro: Some("Sega - Game Gear"),
        zophar: None,
    },
    Console {
        id: "saturn",
        name: "Sega Saturn",
        name_es: "Sega Saturn",
        aliases: &["saturn", "ss", "sega saturn"],
        codecs: &["ssf", "minissf", "ssflib"],
        weak_codecs: &[],
        libretro: Some("Sega - Saturn"),
        zophar: None,
    },
    Console {
        id: "dreamcast",
        name: "Dreamcast",
        name_es: "Dreamcast",
        aliases: &["dreamcast", "dc", "sega dreamcast"],
        codecs: &["dsf", "minidsf", "dsflib"],
        weak_codecs: &[],
        libretro: Some("Sega - Dreamcast"),
        zophar: None,
    },
    Console {
        id: "ps1",
        name: "PlayStation",
        name_es: "PlayStation",
        aliases: &["psx", "ps1", "psone", "playstation"],
        codecs: &["psf", "minipsf", "psflib"],
        weak_codecs: &["xa", "svag", "vag", "mib", "ads", "sts"],
        libretro: Some("Sony - PlayStation"),
        zophar: None,
    },
    Console {
        id: "ps2",
        name: "PlayStation 2",
        name_es: "PlayStation 2",
        aliases: &["ps2", "playstation 2"],
        codecs: &["psf2", "minipsf2", "psf2lib"],
        weak_codecs: &[],
        libretro: Some("Sony - PlayStation 2"),
        zophar: None,
    },
    Console {
        id: "ps3",
        name: "PlayStation 3",
        name_es: "PlayStation 3",
        aliases: &["ps3", "playstation 3"],
        codecs: &[],
        weak_codecs: &[],
        // The archive exists but held 67 covers when last counted, so most
        // lookups will fall through to the other sources anyway.
        libretro: Some("Sony - PlayStation 3"),
        zophar: None,
    },
    Console {
        id: "ps4",
        name: "PlayStation 4",
        name_es: "PlayStation 4",
        aliases: &["ps4", "playstation 4"],
        codecs: &[],
        weak_codecs: &[],
        // 20 covers when last counted. Kept for the same reason as PS3.
        libretro: Some("Sony - PlayStation 4"),
        zophar: None,
    },
    Console {
        id: "psp",
        name: "PSP",
        name_es: "PSP",
        aliases: &["psp", "playstation portable"],
        codecs: &[],
        weak_codecs: &["at3", "at9"],
        libretro: Some("Sony - PlayStation Portable"),
        zophar: None,
    },
    Console {
        id: "psvita",
        name: "PS Vita",
        name_es: "PS Vita",
        aliases: &["vita", "psvita", "ps vita", "playstation vita"],
        codecs: &[],
        weak_codecs: &[],
        libretro: Some("Sony - PlayStation Vita"),
        zophar: None,
    },
    Console {
        id: "tg16",
        name: "TurboGrafx-16",
        name_es: "PC Engine",
        aliases: &["pce", "pc engine", "turbografx", "turbografx 16", "tg16"],
        codecs: &["hes"],
        weak_codecs: &[],
        libretro: Some("NEC - PC Engine - TurboGrafx 16"),
        zophar: Some("turbografx-16-hes"),
    },
    Console {
        id: "msx",
        name: "MSX",
        name_es: "MSX",
        aliases: &["msx"],
        codecs: &["kss"],
        weak_codecs: &[],
        libretro: Some("Microsoft - MSX"),
        zophar: Some("msx2"),
    },
    Console {
        id: "spectrum",
        name: "ZX Spectrum",
        name_es: "ZX Spectrum",
        aliases: &["zx", "spectrum", "zx spectrum"],
        codecs: &["ay"],
        weak_codecs: &[],
        libretro: Some("Sinclair - ZX Spectrum"),
        zophar: Some("spectrum"),
    },
    Console {
        id: "c64",
        name: "Commodore 64",
        name_es: "Commodore 64",
        aliases: &["c64", "commodore 64", "commodore"],
        codecs: &["sid"],
        weak_codecs: &[],
        libretro: Some("Commodore - 64"),
        zophar: None,
    },
    Console {
        id: "atari8",
        name: "Atari 8-bit",
        name_es: "Atari de 8 bits",
        // SAP is a POKEY log. The old table filed it under the 2600, which has
        // a TIA and cannot produce one.
        aliases: &["atari", "atari 8 bit", "a800", "atari 800"],
        codecs: &["sap"],
        weak_codecs: &[],
        libretro: Some("Atari - 8-bit"),
        zophar: Some("atari-8bit"),
    },
    Console {
        id: "arcade",
        name: "Arcade",
        name_es: "Arcade",
        aliases: &["arcade", "mame", "neogeo", "neo geo"],
        codecs: &["qsf", "miniqsf", "qsflib"],
        weak_codecs: &[],
        // MAME names cabinets by ROM set, which no soundtrack folder matches.
        libretro: None,
        zophar: None,
    },
    Console {
        id: "xbox",
        name: "Xbox",
        name_es: "Xbox",
        aliases: &["xbox"],
        codecs: &[],
        weak_codecs: &[],
        libretro: Some("Microsoft - Xbox"),
        zophar: None,
    },
    Console {
        id: "x360",
        name: "Xbox 360",
        name_es: "Xbox 360",
        aliases: &["x360", "xbox 360"],
        codecs: &[],
        weak_codecs: &[],
        libretro: Some("Microsoft - Xbox 360"),
        zophar: None,
    },
    Console {
        id: "pc",
        name: "PC",
        name_es: "PC",
        aliases: &["pc", "windows", "dos", "msdos", "steam"],
        codecs: &[],
        weak_codecs: &[],
        // The DOS archive exists, but a `PC/` folder today holds indie releases
        // that were never in it. Wikidata answers these far better.
        libretro: None,
        zophar: None,
    },
];

/// Look a console up by its stable id.
pub fn by_id(id: &str) -> Option<&'static Console> {
    CONSOLES.iter().find(|c| c.id == id)
}

/// The console an extension names on its own, if any.
///
/// Case-insensitive, so it takes the database's uppercase `codec` column as
/// happily as a lowercase extension.
pub fn by_codec(ext: &str) -> Option<&'static Console> {
    let ext = ext.trim().to_ascii_lowercase();
    if ext.is_empty() {
        return None;
    }
    CONSOLES.iter().find(|c| c.codecs.contains(&ext.as_str()))
}

/// The console to assume for an extension that does not name one.
///
/// Only meaningful once the folder has been asked and had no answer.
pub fn by_weak_codec(ext: &str) -> Option<&'static Console> {
    let ext = ext.trim().to_ascii_lowercase();
    if ext.is_empty() {
        return None;
    }
    CONSOLES.iter().find(|c| c.weak_codecs.contains(&ext.as_str()))
}

/// Fold a folder name into the form the alias lists are written in.
///
/// Lowercase, and every run of separators becomes a single space, so
/// `"Super_Nintendo"`, `"super-nintendo"` and `"SUPER   NINTENDO"` are one name.
pub fn normalize_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut pending_space = false;
    for ch in segment.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_space = true;
        }
    }
    out
}

/// The console a folder name means, if it means one.
///
/// Matches the whole normalised name first, then drops trailing tokens one at a
/// time — so `"snes spc osts"` tries `"snes spc osts"`, then `"snes spc"`, then
/// `"snes"`, and lands. A real library is full of folders named this way.
///
/// Leading tokens only, and never a substring: `"ps"` must not swallow `"psp"`
/// or `"psx"`, and a game called `"Wii Sports"` filed at the top level must not
/// be mistaken for the console.
pub fn by_folder_segment(segment: &str) -> Option<&'static Console> {
    let normalized = normalize_segment(segment);
    if normalized.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = normalized.split(' ').collect();
    for take in (1..=tokens.len()).rev() {
        let candidate = tokens[..take].join(" ");
        if let Some(c) = CONSOLES
            .iter()
            .find(|c| c.aliases.contains(&candidate.as_str()))
        {
            return Some(c);
        }
    }
    None
}

/// The bucket a track with no known console is filed under.
///
/// A real id and not the empty string, because this value is used as a grouping
/// key and as a UI field that distinguishes "no console" from "not set". The
/// database spells unknown as `""`; every view spells it as this.
pub const UNKNOWN: &str = "otros";

/// The console key a track is filed under: its resolved id, or [`UNKNOWN`].
///
/// Reads what `crate::classify` already decided and stored, rather than
/// deriving it again from the extension. That is the whole point: an `.mp3`
/// under `PC/` or `Switch/` has no extension to derive from, and only the
/// stored answer knows where it came from.
pub fn key_of(track: &crate::db::models::Track) -> &str {
    if track.console_id.is_empty() {
        UNKNOWN
    } else {
        &track.console_id
    }
}

/// Spanish display name for a console key. Used by `tunante` and Android.
pub fn label_es(key: &str) -> &'static str {
    by_id(key).map(|c| c.name_es).unwrap_or("Otros")
}

/// English display name for a console key. Used by the desktop app.
pub fn label_en(key: &str) -> &'static str {
    by_id(key).map(|c| c.name).unwrap_or("Other")
}

/// Sort rank: known consoles alphabetically by label, unknown always last.
///
/// Sorting by id would file "Otros" among the a's and order everything else by
/// a string nobody ever sees.
pub fn display_order(key: &str) -> (u8, &'static str) {
    if key == UNKNOWN {
        (1, "Otros")
    } else {
        (0, label_es(key))
    }
}

/// The extension of a track path, lowercase, with any `#subsong` suffix removed.
pub fn extension_of(path: &str) -> String {
    let real = crate::vgm_path::parse_vgm_path(path).0;
    Path::new(real)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_extensions_name_their_machine() {
        assert_eq!(by_codec("nsf").unwrap().id, "nes");
        assert_eq!(by_codec("spc").unwrap().id, "snes");
        assert_eq!(by_codec("psf2").unwrap().id, "ps2");
        assert_eq!(by_codec("sap").unwrap().id, "atari8");
    }

    /// The database stores `codec` uppercase; the table is written lowercase.
    #[test]
    fn the_database_spelling_of_a_codec_still_matches() {
        assert_eq!(by_codec("SPC").unwrap().id, "snes");
        assert_eq!(by_codec("MINIPSF").unwrap().id, "ps1");
    }

    /// `.psf2` must not be read as `.psf`: the match is on the whole extension.
    #[test]
    fn psf2_is_not_psf() {
        assert_ne!(by_codec("psf2").unwrap().id, by_codec("psf").unwrap().id);
    }

    /// The reason the strong/weak split exists. A `.vgm` names no machine, so
    /// it must not claim one from the extension alone.
    #[test]
    fn a_vgm_does_not_name_a_machine_on_its_own() {
        assert!(by_codec("vgm").is_none());
        assert_eq!(by_weak_codec("vgm").unwrap().id, "genesis");
    }

    /// And an extension with no majority worth guessing on claims nothing at
    /// all. Measured: 365 under `PSX/`, 132 under `3DS/`, 99 under `wii/`.
    #[test]
    fn a_genuinely_ambiguous_extension_stays_unclaimed() {
        assert!(by_codec("adx").is_none());
        assert!(by_weak_codec("adx").is_none());
    }

    /// Plain audio says nothing about a console, and must not be made to.
    #[test]
    fn standard_audio_names_no_console() {
        for ext in ["mp3", "flac", "ogg", "wav", "m4a", "opus"] {
            assert!(by_codec(ext).is_none(), "{ext} should not be a strong codec");
            assert!(by_weak_codec(ext).is_none(), "{ext} should not be a weak codec");
        }
    }

    /// Each weak extension may be claimed once. A second claim would be
    /// unreachable, and the shadowed entry would be a silent lie.
    #[test]
    fn no_extension_is_claimed_twice() {
        let mut seen: Vec<&str> = Vec::new();
        for c in CONSOLES {
            for ext in c.codecs.iter().chain(c.weak_codecs.iter()) {
                assert!(!seen.contains(ext), "{ext} is claimed by more than one console");
                seen.push(ext);
            }
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for c in CONSOLES {
            assert!(!seen.contains(&c.id), "duplicate console id {}", c.id);
            seen.push(c.id);
        }
    }

    /// An alias written unnormalised would never match anything, because the
    /// lookup normalises the input and compares literally.
    #[test]
    fn every_alias_is_already_normalised() {
        for c in CONSOLES {
            for a in c.aliases {
                assert_eq!(&normalize_segment(a), a, "alias {a:?} of {} is not normalised", c.id);
            }
        }
    }

    #[test]
    fn separators_do_not_make_a_different_folder() {
        assert_eq!(normalize_segment("Super_Nintendo"), "super nintendo");
        assert_eq!(normalize_segment("super-nintendo"), "super nintendo");
        assert_eq!(normalize_segment("SUPER   NINTENDO"), "super nintendo");
        assert_eq!(normalize_segment("  [PSX]  "), "psx");
    }

    /// The folder names a real library actually uses.
    #[test]
    fn the_folders_a_real_library_is_made_of() {
        for (folder, id) in [
            ("snes spc osts", "snes"),
            ("PSX", "ps1"),
            ("Gba", "gba"),
            ("NDS", "nds"),
            ("N64", "n64"),
            ("wii", "wii"),
            ("Genesis", "genesis"),
            ("3DS", "n3ds"),
            ("gamecube", "gamecube"),
            ("GB", "gameboy"),
            ("NES", "nes"),
            ("sms", "mastersystem"),
            ("psp", "psp"),
            ("PS4", "ps4"),
            ("Switch", "switch"),
            ("PC", "pc"),
        ] {
            assert_eq!(
                by_folder_segment(folder).map(|c| c.id),
                Some(id),
                "folder {folder:?}"
            );
        }
    }

    /// Franchise and community folders are not consoles, and saying so is the
    /// whole point of the flagging UI. Guessing here would be worse than the
    /// honest blank.
    #[test]
    fn a_folder_that_is_not_a_console_claims_nothing() {
        for folder in [
            "Megaten",
            "Pokemon",
            "OCRemixes",
            "Crypt of the necrodancer",
            "Chiptune",
            "[PLATAFORMA]",
        ] {
            assert!(
                by_folder_segment(folder).is_none(),
                "{folder:?} should not resolve to a console"
            );
        }
    }

    /// Trailing tokens are dropped; leading ones are not. Otherwise a game
    /// filed at the top level would be read as the machine it runs on.
    #[test]
    fn a_game_named_after_a_console_is_not_the_console() {
        assert_eq!(by_folder_segment("Wii Sports").map(|c| c.id), Some("wii"));
        // ...but only because "wii" leads. The reverse must not match.
        assert!(by_folder_segment("Super Mario Wii U Deluxe").is_none());
    }

    /// A prefix of an alias is not the alias.
    #[test]
    fn ps_does_not_swallow_psp_or_psx() {
        assert!(by_folder_segment("ps").is_none());
        assert_eq!(by_folder_segment("psp").map(|c| c.id), Some("psp"));
        assert_eq!(by_folder_segment("psx").map(|c| c.id), Some("ps1"));
    }

    #[test]
    fn an_unknown_id_is_not_a_console() {
        assert!(by_id("dreamcast").is_some());
        assert!(by_id("nonexistent").is_none());
    }

    /// A subsong address is not an extension.
    #[test]
    fn a_subsong_suffix_does_not_hide_the_format() {
        assert_eq!(extension_of("/m/pokemon.gbs#7"), "gbs");
        assert_eq!(by_codec(&extension_of("/m/pokemon.gbs#7")).unwrap().id, "gameboy");
    }

    #[test]
    fn a_file_without_an_extension_has_none() {
        assert_eq!(extension_of("/m/no-extension"), "");
        assert!(by_codec("").is_none());
        assert!(by_weak_codec("").is_none());
    }

    /// The Libretro directory names are copied from the live index and are not
    /// guessable. If one is edited to a plausible-looking guess, every lookup
    /// for that console 404s silently.
    #[test]
    fn the_libretro_names_that_are_easy_to_get_wrong() {
        assert_eq!(by_id("atari8").unwrap().libretro, Some("Atari - 8-bit"));
        assert_eq!(
            by_id("mastersystem").unwrap().libretro,
            Some("Sega - Master System - Mark III")
        );
        assert_eq!(by_id("c64").unwrap().libretro, Some("Commodore - 64"));
        assert_eq!(by_id("switch").unwrap().libretro, None);
    }
}
