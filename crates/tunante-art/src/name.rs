//! Folding a game name into something two different people's spelling of it
//! will agree on.
//!
//! Applied identically to both sides of every comparison — to the folder or tag
//! we have, and to the No-Intro name the Libretro archive publishes. That
//! symmetry is the whole trick: it does not matter much *what* the rules are, as
//! long as `Lufia II - Rise of the Sinistrals (USA)` and `Lufia 2` land on the
//! same string.
//!
//! Three of the rules are less obvious than they look, and each was chosen
//! against a real failure:
//!
//! - **Noise words are stripped only as a trailing run**, never in the middle.
//!   `Celeste Original Soundtrack` should become `celeste`, but a global strip
//!   would also maim `The Music Machine` and `Complete Chaos`. The abbreviated
//!   folders that a global strip might have rescued are already rescued by the
//!   rip's own tags.
//! - **Articles are removed, not rotated.** No-Intro writes `Lion King, The`;
//!   a library writes `The Lion King`. Dropping a leading *or* trailing article
//!   collapses both to the same key in one step, where rotating only handles the
//!   direction you thought of.
//! - **Roman numerals emit both forms rather than picking one, and `i` and `x`
//!   are deliberately excluded.** `Mega Man X`, `Rockman X` and `Final Fantasy X`
//!   are three different things and only one of them is a ten. Mapping `x → 10`
//!   is how a matcher returns the wrong box with full confidence.

/// A name, folded, plus the other spellings it could equally have had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    /// The primary folded form.
    pub key: String,
    /// `key`, split on spaces. Precomputed because the matcher walks these a lot.
    pub tokens: Vec<String>,
    /// Equally valid spellings — roman/arabic variants. Never contains `key`.
    pub alts: Vec<String>,
    /// The parenthesised groups that were stripped, lowercased. Used to rank
    /// regional variants of the same game against each other.
    pub groups: Vec<String>,
}

impl Normalized {
    /// Every spelling of this name, primary first.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.key.as_str()).chain(self.alts.iter().map(|s| s.as_str()))
    }
}

/// Words that mean "this is a soundtrack rip", not part of any game's title.
const NOISE: &[&str] = &[
    "ost", "osts", "soundtrack", "soundtracks", "original", "score", "bgm", "gamerip", "rip",
    "rips", "complete", "music", "mp3", "flac", "spc", "psf", "psf2", "nsf", "gsf", "usf", "vgm",
    "gbs", "2sf", "minipsf", "minigsf", "edition", "selection",
];

/// Noise that may also be stripped from the *front*.
///
/// Much narrower than [`NOISE`], and it has to be. Folders are named
/// `OST - Chrono Trigger`, so a leading marker is worth removing — but
/// `Complete Chaos` and `Original Sin` are games whose titles start with a word
/// that only means "rip" at the end of a name. Reusing the full list here turned
/// them into `chaos` and `sin`.
const LEADING_NOISE: &[&str] = &["ost", "osts", "soundtrack", "gamerip"];

const ARTICLES: &[&str] = &["the", "a", "an", "la", "el", "los", "las", "le", "les", "die", "der", "das", "il"];

const ROMAN: &[(&str, &str)] = &[
    ("ii", "2"),
    ("iii", "3"),
    ("iv", "4"),
    ("v", "5"),
    ("vi", "6"),
    ("vii", "7"),
    ("viii", "8"),
    ("ix", "9"),
    // `i` and `x` are absent on purpose. See the module docs.
];

/// Fold a Latin-1-ish letter to ASCII. A small table beats a Unicode
/// normalisation dependency for the handful of characters that actually turn up
/// in game titles.
fn fold_char(c: char) -> char {
    match c {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' | 'ø' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        'ā' => 'a',
        'ē' => 'e',
        'ī' => 'i',
        'ō' => 'o',
        'ū' => 'u',
        other => other,
    }
}

/// Strip bracketed groups, returning the remainder and the groups themselves.
///
/// Depth-tracked rather than regex-based, so a nested group does not leave a
/// dangling bracket behind. Written for names like
/// `Lufia II - Rise of the Sinistrals [Estpolis Denki II] [Lufia] (1995)(Neverland)(Taito)`.
fn split_groups(raw: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(raw.len());
    let mut groups = Vec::new();
    let mut current = String::new();
    let (mut paren, mut bracket, mut curly) = (0i32, 0i32, 0i32);
    for ch in raw.chars() {
        let depth = paren + bracket + curly;
        match ch {
            '(' => paren += 1,
            '[' => bracket += 1,
            '{' => curly += 1,
            ')' | ']' | '}' => {
                match ch {
                    ')' => paren = (paren - 1).max(0),
                    ']' => bracket = (bracket - 1).max(0),
                    _ => curly = (curly - 1).max(0),
                }
                if paren + bracket + curly == 0 && !current.trim().is_empty() {
                    groups.push(current.trim().to_lowercase());
                    current.clear();
                }
            }
            _ if depth == 0 => out.push(ch),
            _ => current.push(ch),
        }
    }
    (out, groups)
}

/// Fold a name into its comparable form.
pub fn normalize(raw: &str) -> Normalized {
    // A subsong address is not part of any name.
    let raw = raw.split('#').next().unwrap_or(raw);
    let (stripped, groups) = split_groups(raw);

    let mut flat = String::with_capacity(stripped.len());
    for ch in stripped.chars() {
        let ch = fold_char(ch.to_lowercase().next().unwrap_or(ch));
        if ch == '&' {
            flat.push_str(" and ");
        } else if ch.is_alphanumeric() {
            flat.push(ch);
        } else {
            flat.push(' ');
        }
    }

    let mut tokens: Vec<String> = flat.split_whitespace().map(|s| s.to_string()).collect();

    // Noise, as a trailing run only, and never down to nothing: a game really
    // called "Music" keeps its name.
    while tokens.len() > 1 && NOISE.contains(&tokens.last().unwrap().as_str()) {
        tokens.pop();
    }
    // A leading "OST - " is the same marker from the other end, but only the
    // unambiguous ones: see LEADING_NOISE.
    while tokens.len() > 1 && LEADING_NOISE.contains(&tokens[0].as_str()) {
        tokens.remove(0);
    }

    // Articles at either end. Both directions, so `The Lion King` and
    // `Lion King, The` fold together.
    if tokens.len() > 1 && ARTICLES.contains(&tokens[0].as_str()) {
        tokens.remove(0);
    }
    if tokens.len() > 1 && ARTICLES.contains(&tokens.last().unwrap().as_str()) {
        tokens.pop();
    }

    let key = tokens.join(" ");

    // Both spellings of any standalone numeral, so `Lufia 2` reaches
    // `Lufia II - Rise of the Sinistrals`.
    let mut alts = Vec::new();
    for (roman, arabic) in ROMAN {
        for (from, to) in [(*roman, *arabic), (*arabic, *roman)] {
            if tokens.iter().any(|t| t == from) {
                let swapped: Vec<String> = tokens
                    .iter()
                    .map(|t| if t == from { to.to_string() } else { t.clone() })
                    .collect();
                let alt = swapped.join(" ");
                if alt != key && !alts.contains(&alt) {
                    alts.push(alt);
                }
            }
        }
    }

    Normalized { key, tokens, alts, groups }
}

/// How much of a name two strings share, by Ratcliff/Obershelp gestalt matching.
///
/// Hand-rolled rather than pulled from `strsim` on purpose. The 0.82 cutoff the
/// matcher uses was measured with Python's `difflib.SequenceMatcher`, which is
/// this algorithm; `normalized_levenshtein` and `jaro_winkler` return different
/// numbers for the same pair, so keeping the constant while swapping the
/// algorithm would silently move the hit rate in both directions at once.
pub fn similarity(a: &str, b: &str) -> f64 {
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let matched = matching_chars(&a, &b);
    2.0 * matched as f64 / (a.len() + b.len()) as f64
}

/// Total length of the matching blocks: the longest common substring, plus the
/// same measure recursively on what is left to each side of it.
fn matching_chars(a: &[char], b: &[char]) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    // Longest common substring, by the usual rolling DP over one row.
    let (mut best_a, mut best_b, mut best_len) = (0usize, 0usize, 0usize);
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] { prev[j - 1] + 1 } else { 0 };
            if cur[j] > best_len {
                best_len = cur[j];
                best_a = i - best_len;
                best_b = j - best_len;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
        cur.iter_mut().for_each(|v| *v = 0);
    }
    if best_len == 0 {
        return 0;
    }
    best_len
        + matching_chars(&a[..best_a], &b[..best_b])
        + matching_chars(&a[best_a + best_len..], &b[best_b + best_len..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> String {
        normalize(s).key
    }

    #[test]
    fn the_name_the_group_stripper_was_written_for() {
        let n = normalize(
            "Lufia II - Rise of the Sinistrals [Estpolis Denki II] [Lufia] (1995)(Neverland)(Taito)",
        );
        assert_eq!(n.key, "lufia ii rise of the sinistrals");
        assert!(n.groups.contains(&"1995".to_string()));
        assert!(n.groups.contains(&"neverland".to_string()));
    }

    /// Both sides of a real comparison have to land on the same string. This is
    /// the property the whole module exists for.
    #[test]
    fn a_library_folder_and_a_no_intro_name_agree() {
        for (mine, theirs) in [
            ("Chrono Trigger", "Chrono Trigger (USA)"),
            ("Secret of Mana", "Secret of Mana (USA)"),
            ("Super Metroid OST", "Super Metroid (Japan, USA)"),
            ("Final Fantasy III", "Final Fantasy III (USA) (Rev 1)"),
            ("Celeste Original Soundtrack [MP3]", "Celeste"),
        ] {
            assert_eq!(key(mine), key(theirs), "{mine:?} vs {theirs:?}");
        }
    }

    /// No-Intro rotates the article to the end. Removing it at either end
    /// collapses both spellings without needing to know which one you have.
    #[test]
    fn an_article_at_either_end_is_dropped() {
        assert_eq!(key("The Lion King"), key("Lion King, The"));
        assert_eq!(key("The Legend of Zelda"), "legend of zelda");
    }

    /// Trailing only. A global strip would maim these.
    #[test]
    fn noise_words_inside_a_title_survive() {
        assert_eq!(key("The Music Machine"), "music machine");
        assert_eq!(key("Complete Chaos"), "complete chaos");
        assert_eq!(key("Original Sin"), "original sin");
    }

    #[test]
    fn noise_words_at_the_end_do_not() {
        assert_eq!(key("Shantae OST"), "shantae");
        assert_eq!(key("Chrono Trigger Original Soundtrack"), "chrono trigger");
        assert_eq!(key("Xenogears gamerip"), "xenogears");
    }

    /// Stripping must never leave nothing behind.
    #[test]
    fn a_game_actually_called_music_keeps_its_name() {
        assert_eq!(key("Music"), "music");
        assert_eq!(key("OST"), "ost");
        assert_eq!(key("The"), "the");
    }

    #[test]
    fn both_numeral_spellings_are_offered() {
        let n = normalize("Lufia 2");
        assert_eq!(n.key, "lufia 2");
        assert!(n.alts.contains(&"lufia ii".to_string()));

        let n = normalize("Final Fantasy VII");
        assert!(n.alts.contains(&"final fantasy 7".to_string()));
    }

    /// The one that makes a matcher confidently wrong. `X` is a name here, not
    /// a ten, in three separate long-running series.
    #[test]
    fn x_is_not_ten_and_i_is_not_one() {
        for name in ["Mega Man X", "Final Fantasy X", "Rockman X"] {
            let n = normalize(name);
            assert!(
                n.alts.is_empty(),
                "{name:?} should offer no numeral alternative, got {:?}",
                n.alts
            );
        }
        assert!(normalize("Final Fantasy I").alts.is_empty());
    }

    #[test]
    fn accents_and_ampersands_fold() {
        assert_eq!(key("Pokémon Ultra Sun"), "pokemon ultra sun");
        assert_eq!(key("Ōkami"), "okami");
        assert_eq!(key("Digital Devil Saga 1 & 2"), key("Digital Devil Saga 1 and 2"));
    }

    #[test]
    fn separators_do_not_matter() {
        assert_eq!(key("Chrono_Cross"), key("Chrono Cross"));
        assert_eq!(key("Mega-Man-Zero"), key("Mega Man Zero"));
    }

    #[test]
    fn a_subsong_address_is_not_part_of_a_name() {
        assert_eq!(key("pokemon.gbs#7"), key("pokemon.gbs"));
    }

    // --- similarity ---

    /// Ratcliff/Obershelp, matching Python's difflib, which is where the 0.82
    /// cutoff came from.
    #[test]
    fn similarity_matches_the_algorithm_the_cutoff_was_measured_with() {
        assert!((similarity("", "") - 1.0).abs() < 1e-9);
        assert!((similarity("abc", "abc") - 1.0).abs() < 1e-9);
        assert!(similarity("abc", "xyz").abs() < 1e-9);
        // difflib.SequenceMatcher(None, "pocky and rocky", "pocky rocky").ratio()
        // == 0.8461538461538461
        let got = similarity("pocky and rocky", "pocky rocky");
        assert!((got - 0.846_153_846_153_846_1).abs() < 1e-9, "got {got}");
        // ...("demons crest", "demon's crest") == 0.96
        let got = similarity("demons crest", "demon s crest");
        assert!((got - 0.96).abs() < 1e-9, "got {got}");
    }

    #[test]
    fn the_near_misses_the_cutoff_has_to_catch() {
        for (a, b) in [
            ("demons crest", "demon s crest"),
            ("kirbys dream course", "kirby s dream course"),
            ("un squadron", "u n squadron"),
            ("romancing saga 2", "romancing sa ga 2"),
        ] {
            assert!(similarity(a, b) >= 0.82, "{a:?} vs {b:?} = {}", similarity(a, b));
        }
    }

    /// The limit of this measure, stated as a test rather than discovered later.
    ///
    /// Different games in one series are textually almost identical —
    /// `final fantasy vii` and `final fantasy viii` score 0.97, well above any
    /// usable cutoff. **Similarity cannot tell them apart, and no threshold
    /// will.** What keeps the matcher honest is elsewhere: exact matching runs
    /// first (so a name that is really `Final Fantasy VII` never reaches the
    /// fuzzy stage), and the fuzzy stage refuses to answer at all when the
    /// runner-up scores within 0.03 of the winner.
    ///
    /// If this test ever starts failing because the numbers dropped, the
    /// ambiguity guard has become load-bearing in a new way and deserves a look.
    #[test]
    fn similarity_alone_cannot_separate_a_numbered_series() {
        for (a, b) in [
            ("final fantasy vii", "final fantasy viii"),
            ("mega man zero 2", "mega man zero 3"),
        ] {
            assert!(
                similarity(a, b) > 0.9,
                "{a:?} vs {b:?} = {} — if this dropped, revisit the ambiguity guard",
                similarity(a, b)
            );
        }
        // Which is why the numeral variants are generated as *alternates* and
        // compared for equality, rather than left to the fuzzy stage.
        assert!(normalize("Final Fantasy VII").alts.contains(&"final fantasy 7".to_string()));
    }

    /// A leading rip marker goes; a title that merely starts with one of those
    /// words does not.
    #[test]
    fn a_leading_rip_marker_goes_but_a_real_title_stays() {
        assert_eq!(key("OST - Chrono Trigger"), "chrono trigger");
        assert_eq!(key("Soundtrack Xenogears"), "xenogears");
        assert_eq!(key("Complete Chaos"), "complete chaos");
        assert_eq!(key("Original Sin"), "original sin");
        assert_eq!(key("Music Machine"), "music machine");
    }
}
