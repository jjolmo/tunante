//! Looking for one game across every archive, not just its own.
//!
//! # Why this is not "search the console's archive and give up"
//!
//! Because games are multiplatform and rips are filed by whoever made them.
//! Measured over the games a real collection could not match in their own
//! console's archive, searching the rest recovered **one in five**, and every
//! one was the same game under another platform:
//!
//! - Game Boy rips of what are really Game Boy *Color* titles — Pokémon
//!   Crystal, Telefang, Shantae. The `.gbs` format does not distinguish them
//!   and no folder ever will.
//! - PC folders holding games that also shipped on 3DS, Wii, DS or Vita —
//!   Cave Story, La-Mulana, Child of Light, Plants vs. Zombies.
//! - An NES folder holding Shovel Knight, which is a modern multiplatform game
//!   that merely sounds like an NES one.
//!
//! For a music player, the box art of the right game from the wrong platform is
//! a good answer. It is still marked one step less confident than a hit in the
//! console's own archive, because the platform did not corroborate it.
//!
//! # Two guards, both learned from real wrong answers
//!
//! A two-letter folder called `Vs` matched `VS. (USA)` on another system, and
//! `Pokemon Gold` matched a pirate dump. Hence [`MIN_CROSS_LEN`] and the
//! hack/pirate penalty in [`crate::index`].

use crate::index::{Index, Match};
use crate::Confidence;

/// Below this many characters, a normalised name is too generic to search
/// outside its own console. `Vs`, `ct`, `sm` match something everywhere.
pub const MIN_CROSS_LEN: usize = 4;

/// One archive, with the console it belongs to.
pub struct Archive<'a> {
    /// The console id this archive is for.
    pub console_id: &'a str,
    pub index: &'a Index,
}

#[derive(Debug, Clone)]
pub struct Found {
    pub console_id: String,
    pub entry_index: usize,
    pub confidence: Confidence,
    pub via: String,
    /// False when the hit came from another platform's archive.
    pub same_console: bool,
}

/// Search `own` first, then everything else.
///
/// `candidates` are the names to try, best first — typically the rip's own
/// album tag and then the folder it sits in.
pub fn find<'a>(
    own: Option<&Archive<'a>>,
    others: &[Archive<'a>],
    candidates: &[String],
) -> Option<Found> {
    if let Some(a) = own {
        if let Some(m) = a.index.best_match(candidates) {
            return Some(promote(a.console_id, m, true));
        }
    }

    // Anything too short to be distinctive stays home.
    let long_enough: Vec<String> = candidates
        .iter()
        .filter(|c| crate::name::normalize(c).key.len() >= MIN_CROSS_LEN)
        .cloned()
        .collect();
    if long_enough.is_empty() {
        return None;
    }

    let mut best: Option<Found> = None;
    for a in others {
        if Some(a.console_id) == own.map(|o| o.console_id) {
            continue;
        }
        if let Some(m) = a.index.best_match(&long_enough) {
            // Only an equality-grade hit is worth taking from a platform that
            // does not corroborate it. A fuzzy match against 46,000 names from
            // twenty archives is a lottery, not a lookup.
            if m.confidence < Confidence::High {
                continue;
            }
            let found = promote(a.console_id, m, false);
            if best.as_ref().is_none_or(|b| found.confidence > b.confidence) {
                let exact = found.confidence == Confidence::High;
                best = Some(found);
                if exact {
                    break;
                }
            }
        }
    }
    best
}

/// A hit in another console's archive is one step less certain: the name
/// agreed, the platform did not.
fn promote(console_id: &str, m: Match, same_console: bool) -> Found {
    let confidence = if same_console {
        m.confidence
    } else {
        match m.confidence {
            Confidence::Exact => Confidence::High,
            other => other,
        }
    };
    Found {
        console_id: console_id.to_string(),
        entry_index: m.entry_index,
        confidence,
        via: m.via,
        same_console,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(names: &[&str]) -> Index {
        Index::new(names.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn the_consoles_own_archive_wins() {
        let own = idx(&["Chrono Trigger (USA)"]);
        let other = idx(&["Chrono Trigger (Japan)"]);
        let a = Archive { console_id: "snes", index: &own };
        let b = Archive { console_id: "ps1", index: &other };
        let got = find(Some(&a), &[b], &["Chrono Trigger".into()]).unwrap();
        assert_eq!(got.console_id, "snes");
        assert!(got.same_console);
        assert_eq!(got.confidence, Confidence::Exact);
    }

    /// The case this module exists for: a Game Boy folder holding a Game Boy
    /// Color game, which no format or folder can tell apart.
    #[test]
    fn another_platforms_archive_answers_when_the_right_one_cannot() {
        let own = idx(&["Tetris (World)"]);
        let gbc = idx(&["Pokemon - Crystal Version (USA)"]);
        let a = Archive { console_id: "gameboy", index: &own };
        let b = Archive { console_id: "gbc", index: &gbc };
        let got = find(Some(&a), &[b], &["Pokemon Crystal".into()]).unwrap();
        assert_eq!(got.console_id, "gbc");
        assert!(!got.same_console);
    }

    /// ...but it is never presented as certainty, because the platform did not
    /// corroborate the name.
    #[test]
    fn a_hit_from_elsewhere_is_downgraded() {
        let gbc = idx(&["Shantae (USA)"]);
        let b = Archive { console_id: "gbc", index: &gbc };
        let got = find(None, &[b], &["Shantae".into()]).unwrap();
        assert_eq!(got.confidence, Confidence::High, "an exact name elsewhere is not Exact");
    }

    /// A two-letter folder matched `VS. (USA)` on an unrelated system. Short
    /// names match something everywhere and mean nothing.
    #[test]
    fn a_name_too_short_to_be_distinctive_stays_home() {
        let other = idx(&["VS. (USA)", "CT (Japan)"]);
        let b = Archive { console_id: "ps1", index: &other };
        assert!(find(None, &[b], &["Vs".into()]).is_none());
        let b2 = Archive { console_id: "ps1", index: &idx(&["VS. (USA)", "CT (Japan)"]) };
        assert!(find(None, &[b2], &["ct".into()]).is_none());
    }

    /// Fuzzy across twenty archives is a lottery. Only an equality-grade name
    /// travels between platforms.
    #[test]
    fn a_fuzzy_hit_does_not_travel_between_archives() {
        // "Castlevania Bloodlines" vs "Castlevania Bloodline" would fuzzy-match
        // within one archive, but must not be taken from another.
        let other = idx(&["Castlevania - Bloodlines (USA)"]);
        let b = Archive { console_id: "genesis", index: &other };
        let got = find(None, &[b], &["Castlevania Bloodlones".into()]);
        assert!(got.is_none(), "a fuzzy hit crossed archives: {got:?}");
    }

    #[test]
    fn nothing_anywhere_is_nothing() {
        let b = Archive { console_id: "ps1", index: &idx(&["Ape Escape (USA)"]) };
        assert!(find(None, &[b], &["Halo".into()]).is_none());
    }
}
