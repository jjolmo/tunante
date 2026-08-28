//! What fraction of a real collection the matcher actually finds.
//!
//! Both sides of this are real and checked in: the Libretro `Named_Boxarts`
//! listing for the SNES (3,676 names), and the 79 game-folder names from a
//! 23,000-file collection — abbreviations, misspellings and all.
//!
//! It exists because "54% of folders matched" is the sort of claim that lives in
//! a commit message, ages badly, and is never checked again. Here it is a number
//! the test prints and a floor the test enforces.
//!
//! The floor is deliberately below the current result. This measures a matching
//! *heuristic* against messy real data; a change that trades two hits here for
//! five elsewhere is legitimate, and the test should catch a collapse, not
//! bicker over one folder.

use std::collections::BTreeSet;
use tunante_art::index::Index;
use tunante_art::Confidence;

fn lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn archive() -> Index {
    Index::new(lines(include_str!("fixtures/libretro-snes-boxarts.txt")))
}

fn folders() -> Vec<String> {
    lines(include_str!("fixtures/snes-folders.txt"))
}

#[test]
fn the_archive_fixture_is_the_real_thing() {
    let idx = archive();
    assert!(idx.len() > 3000, "only {} entries — fixture truncated?", idx.len());
    // A few names that must survive any change to the parser or normaliser.
    let idx2 = archive();
    for q in ["Chrono Trigger", "Secret of Mana", "Super Metroid"] {
        assert!(idx2.best_match(&[q.to_string()]).is_some(), "{q} vanished from the archive");
    }
}

#[test]
fn folder_names_alone_find_most_of_a_real_collection() {
    let idx = archive();
    let folders = folders();
    assert_eq!(folders.len(), 79, "fixture changed");

    let mut by_confidence: std::collections::BTreeMap<String, usize> = Default::default();
    let mut misses: Vec<&str> = Vec::new();
    let mut hits = 0usize;

    for f in &folders {
        match idx.best_match(&[f.clone()]) {
            Some(m) => {
                hits += 1;
                *by_confidence.entry(format!("{:?}", m.confidence)).or_default() += 1;
            }
            None => misses.push(f),
        }
    }

    let pct = hits * 100 / folders.len();
    println!("\n  folder names only: {hits}/{} matched ({pct}%)", folders.len());
    for (c, n) in &by_confidence {
        println!("    {n:3}  {c}");
    }
    println!("  missed ({}):", misses.len());
    for m in &misses {
        println!("    {m}");
    }

    // 62% when this fixture was taken. A Python prototype of the same idea got
    // 54%; the difference is the article rule, the two subtitle stages and the
    // roman/arabic alternates, each of which was worth a handful of folders.
    //
    // Of the 30 that miss, 28 are abbreviations — see the next test. The other
    // two are `TMNT 4 - Turtles in Time` (the archive says "Teenage Mutant Ninja
    // Turtles IV") and `Treasure of the Rudras` (it says "Rudra no Hihou"). Both
    // want an alias table, not a cleverer string metric.
    assert!(pct >= 55, "hit rate fell to {pct}% — it was 62% when this fixture was taken");
}

/// The other half of the story, and the reason the misses above are acceptable.
///
/// Nearly every folder that fails is an abbreviation — `ct`, `ff6`, `smw`, `yi`,
/// `mo2`. Those are unmatched *by design*: guessing what `mo2` means would be
/// inventing an answer. They are not lost, because the rip itself carries the
/// game name — an SPC's ID666 header says "Earthbound" — and the classifier puts
/// that in the album tag, which the caller passes as another candidate.
///
/// This test feeds both, as the real pipeline does.
#[test]
fn the_rips_own_tags_rescue_the_abbreviations() {
    let idx = archive();

    // Real ID666 `game` fields, read from the actual .spc files in those folders.
    let tagged: &[(&str, &str)] = &[
        ("ct", "Chrono Trigger"),
        ("ff6", "Final Fantasy 6"),
        ("sd3", "Seiken Densetsu 3"),
        ("yi", "Yoshi's Island"),
        ("smw", "Super Mario World"),
        ("iog", "Illusion of Time"),
        ("ewj", "Earthworm Jim"),
        ("terra", "Terranigma"),
        ("ogre", "Ogre Battle"),
        ("kingmonster", "King of the Monsters"),
        ("smas", "Super Mario Bros."),
    ];

    let mut rescued = 0;
    let mut still_missing = Vec::new();
    for (folder, tag) in tagged {
        let folder_alone = idx.best_match(&[folder.to_string()]).is_some();
        match idx.best_match(&[tag.to_string(), folder.to_string()]) {
            Some(_) if !folder_alone => rescued += 1,
            Some(_) => {}
            None => still_missing.push(*folder),
        }
    }
    println!("\n  abbreviations rescued by the rip's own tag: {rescued}/{}", tagged.len());
    if !still_missing.is_empty() {
        println!("  still missing: {still_missing:?}");
    }
    assert!(
        rescued >= 8,
        "only {rescued} of {} abbreviations were rescued by their tags",
        tagged.len()
    );
}

/// No two folders may resolve to the same cover. A matcher that maps half the
/// library onto one popular game would score well above and be useless.
#[test]
fn distinct_games_do_not_collapse_onto_one_cover() {
    let idx = archive();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut collisions = Vec::new();
    for f in folders() {
        if let Some(m) = idx.best_match(&[f.clone()]) {
            if !seen.insert(m.entry_index) {
                collisions.push(f);
            }
        }
    }
    assert!(collisions.is_empty(), "these folders matched a cover already used: {collisions:?}");
}

/// Whatever else changes, a fuzzy guess must never be presented as certainty:
/// the caller uses this to decide what may be written to disk unattended.
#[test]
fn fuzzy_results_are_labelled_as_fuzzy() {
    let idx = archive();
    for f in folders() {
        if let Some(m) = idx.best_match(&[f.clone()]) {
            if m.confidence == Confidence::Exact {
                let entry = &idx.entries[m.entry_index];
                assert!(
                    entry.norm.keys().any(|k| tunante_art::name::normalize(&f).keys().any(|q| q == k)),
                    "{f:?} was called Exact but does not equal {:?}",
                    entry.file
                );
            }
        }
    }
}
