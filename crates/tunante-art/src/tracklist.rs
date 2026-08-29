//! Track names for the formats that pack a whole game into one file.
//!
//! A `.gbs` is every song in a Game Boy game, addressed by index. The format
//! has nowhere to put their names — unlike NSFE, whose `tlbl` chunk carries
//! them and which the reader already uses — so a rip with no `.m3u` beside it
//! shows as `pokemon.gbs - Track 17` and there is nothing in the file to do
//! better with.
//!
//! Zophar's Domain publishes the listing, in order, for most commercial games
//! on the six consoles whose format works this way. That order is the rip's
//! order, because the rip is the same file.
//!
//! What this does *not* do is decide anything. It fetches a list and counts it;
//! whether the list belongs to the file is [`matches_subsongs`]'s question, and
//! what to do about it is the caller's.

use crate::http::Http;

/// One entry of a game's listing, in the order the archive lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 1-based, as printed. Not an index into anything — see
    /// [`matches_subsongs`] for why the position is only trustworthy when the
    /// counts agree.
    pub number: u32,
    pub title: String,
    /// As printed — `3:00`, `0:07` — or empty when the row has none.
    ///
    /// Worth taking, and not only for the display. These formats loop forever,
    /// so nothing in the file says how long a track is: without a length the
    /// reader emulates the track and watches for silence to find out, which is
    /// slow and happens once per subsong. A published length skips that
    /// entirely and drives the auto-fade.
    ///
    /// A great many of these read `3:00`, which is the archive's convention for
    /// "this loops" rather than a measurement. That is the same thing a player
    /// needs to know, and the same convention every hand-written GME m3u uses.
    pub length: String,
}

/// The listing page for one game.
pub fn url(system: &str, game_slug: &str) -> String {
    format!("https://www.zophar.net/music/{system}/{game_slug}")
}

/// Turn a game's name into the slug the archive uses.
///
/// Lowercase, spaces to hyphens, and everything else dropped — which is what
/// the site's own URLs look like (`final-fantasy-adventure-[mystic-quest]`
/// keeps its brackets, so those stay).
pub fn slug(game: &str) -> String {
    let mut out = String::with_capacity(game.len());
    let mut last_dash = true;
    for ch in game.trim().to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '[' || ch == ']' {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Every slug worth trying for one game, best first.
///
/// The archive follows the No-Intro convention of rotating a leading article to
/// the end — "The Legend of Zelda" is filed as "Legend of Zelda, The", and
/// `the-legend-of-zelda` is a 404 that reads as "this game is not here". It is
/// one of the most famous games on the console, so a lookup that cannot find it
/// is a lookup nobody will trust.
///
/// Both directions, because a library holds names from both conventions: the
/// cover archives and Nintendo's own catalogue say "The Legend of Zelda", while
/// a folder ripped from a No-Intro set says "Legend of Zelda, The".
pub fn slug_candidates(game: &str) -> Vec<String> {
    const ARTICLES: [&str; 6] = ["the", "a", "an", "el", "la", "los"];
    let g = game.trim();
    let mut out = vec![slug(g)];

    let lower = g.to_lowercase();
    for art in ARTICLES {
        // "The Legend of Zelda" → "Legend of Zelda, The"
        if let Some(rest) = lower.strip_prefix(&format!("{art} ")) {
            let rotated = format!("{}, {art}", &g[g.len() - rest.len()..]);
            push_unique(&mut out, slug(&rotated));
        }
        // "Legend of Zelda, The" → "The Legend of Zelda"
        if let Some(head) = lower.strip_suffix(&format!(", {art}")) {
            let unrotated = format!("{art} {}", &g[..head.len()]);
            push_unique(&mut out, slug(&unrotated));
        }
    }
    out
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !s.is_empty() && !v.contains(&s) {
        v.push(s);
    }
}

/// Pull the numbered listing out of a page.
///
/// By class, not by position: each row carries `number`, `name`, `length` and
/// `download` cells, and reading the classes survives a layout change in a way
/// that counting `<td>`s does not.
///
/// Returns an empty list rather than an error when nothing matches — a game
/// the archive does not have answers with a page, not a 404, and an empty
/// listing is that answer.
pub fn parse(body: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("class=\"name\"") {
        rest = &rest[i..];
        let Some(title) = take_cell(&mut rest) else { break };
        if title.is_empty() {
            continue;
        }
        // The length cell of the same row, which follows the name. Taken by
        // walking forward from here rather than by collecting all the lengths
        // and zipping: a row missing one would shift every pairing after it,
        // and a length on the wrong track is worse than no length.
        let mut after = rest;
        let length = match after.find("class=\"length\"") {
            Some(j) if !after[..j].contains("class=\"name\"") => {
                after = &after[j..];
                take_cell(&mut after).unwrap_or_default()
            }
            _ => String::new(),
        };
        out.push(Entry { number: out.len() as u32 + 1, title, length });
    }
    out
}

/// Consume `<td …>text</td>` from the front, returning its text.
fn take_cell(rest: &mut &str) -> Option<String> {
    let open = rest.find('>')?;
    let close = rest[open..].find("</td>")?;
    let text = strip_tags(&rest[open + 1..open + close]);
    *rest = &rest[open + close..];
    Some(text)
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for ch in s.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    // The site writes a handful of entities and nothing exotic.
    out.replace("&amp;", "&")
        .replace("&#039;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .trim()
        .to_string()
}

/// Is this "name" just the track's own number wearing a word?
///
/// The archive lists an unnamed track as `Track 12`, and a partially tagged rip
/// is common — the Zelda NSF has 22 real names among 37 entries. Writing
/// `Track 12` over `zelda.nsf - Track 12` improves nothing and makes a
/// placeholder look like somebody's answer, which is worse than the obvious
/// filename it replaced.
///
/// Only when the number matches its own position. A game with a song genuinely
/// called "Track 3" at position 9 keeps it.
pub fn is_placeholder(entry: &Entry) -> bool {
    let t = entry.title.trim();
    t.strip_prefix("Track ")
        .or_else(|| t.strip_prefix("track "))
        .and_then(|n| n.trim().parse::<u32>().ok())
        .is_some_and(|n| n == entry.number)
}

/// How many of these actually name something.
pub fn named_count(entries: &[Entry]) -> usize {
    entries.iter().filter(|e| !is_placeholder(e)).count()
}

/// Does this listing describe this file?
///
/// The only check worth making, and it has to be made. Position is the entire
/// mapping — entry 17 names subsong 17 — so a listing of a different length is
/// a listing of a different rip, and applying it renames every track to the
/// wrong song. A wrong name is worse than `Track 17`: the missing name is
/// visible, the wrong one looks deliberate.
///
/// Exact, deliberately. Castlevania's NSF lists 16 songs and 38 sound effects
/// and the file holds all 54; a listing that is "close" is not the same rip.
pub fn matches_subsongs(entries: &[Entry], subsongs: usize) -> bool {
    !entries.is_empty() && entries.len() == subsongs
}

/// Fetch and parse, or nothing.
pub fn fetch(http: &dyn Http, system: &str, game: &str) -> Vec<Entry> {
    for candidate in slug_candidates(game) {
        let url = url(system, &candidate);
        // A listing page is tens of kilobytes; a cap keeps a redirect to
        // something enormous from being read into memory.
        if let Ok(r) = http.get(&url, 512 * 1024) {
            if r.status == 200 {
                let entries = parse(&String::from_utf8_lossy(&r.body));
                if !entries.is_empty() {
                    return entries;
                }
            }
        }
    }
    Vec::new()
}

/// The listing as a GME-style `.m3u`, the sidecar every player of these
/// formats already reads — this one included.
///
/// Written beside the file rather than into a table so it outlives Tunante:
/// the next player to open that folder gets the names too.
pub fn to_m3u(file_name: &str, entries: &[Entry]) -> String {
    // `file::TYPE,track,title,time` — and the type is not optional.
    //
    // The parser skips from `::` to the first comma, so an empty type shifts
    // every field one place left and the title lands where the length is read.
    // The first version of this wrote `file::,1,,Title` and produced tracks
    // called ",Title", which is how the bug announced itself.
    let ty = file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_uppercase())
        .unwrap_or_default();
    let mut s = String::from("# Generated by Tunante from zophar.net\n");
    for e in entries {
        // A comma inside a title would split the field, so it is escaped the
        // way the reader un-escapes it.
        // A placeholder is written as no title at all, so the reader falls back
        // to what it would have said anyway rather than to a fake answer. The
        // line still has to exist: the length is worth keeping, and a missing
        // line would shift nothing but would lose it.
        let title = if is_placeholder(e) {
            String::new()
        } else {
            e.title.replace(',', "\\,")
        };
        s.push_str(&format!("{file_name}::{ty},{},{},{}\n", e.number, title, e.length));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page as the site actually serves it, saved rather than described.
    const CASTLEVANIA: &str = include_str!("../tests/fixtures/zophar-castlevania.html");

    #[test]
    fn the_whole_listing_comes_out_in_order() {
        let e = parse(CASTLEVANIA);
        assert_eq!(e.len(), 54, "16 songs and 38 sound effects");
        assert_eq!(e[0].title, "Introduction (Castle Gate)");
        assert_eq!(e[0].number, 1);
        assert_eq!(e[1].title, "Vampire Killer (Courtyard)");
        assert_eq!(e[53].number, 54);
    }

    /// Sound effects count. The file holds them as subsongs like anything else,
    /// so dropping them would shift every name after the first one.
    /// The lengths come out paired with their own titles, which is the part
    /// that can silently go wrong: a length on the wrong track is worse than
    /// no length at all.
    #[test]
    fn each_title_keeps_its_own_length() {
        let e = parse(CASTLEVANIA);
        assert_eq!(e[0].title, "Introduction (Castle Gate)");
        assert_eq!(e[0].length, "0:07", "the short one is first");
        assert_eq!(e[1].length, "3:00");
        assert!(e.iter().all(|x| !x.length.is_empty()), "every row has one");
    }

    #[test]
    fn sound_effects_are_entries_too() {
        let e = parse(CASTLEVANIA);
        assert!(e.len() > 16, "only the music would leave 38 subsongs unnamed");
    }

    #[test]
    fn a_page_with_no_listing_yields_nothing_rather_than_failing() {
        assert!(parse("<html><body>Not found</body></html>").is_empty());
    }

    #[test]
    fn markup_and_entities_inside_a_title_are_resolved() {
        let html = r#"<td class="name"><a href="/x">Bloody Tears &amp; Beyond</a></td>"#;
        assert_eq!(parse(html)[0].title, "Bloody Tears & Beyond");
    }

    #[test]
    fn slugs_look_like_the_site_s_own_urls() {
        assert_eq!(slug("Castlevania"), "castlevania");
        assert_eq!(slug("Pokemon Trading Card Game"), "pokemon-trading-card-game");
        assert_eq!(slug("Final Fantasy Adventure [Mystic Quest]"), "final-fantasy-adventure-[mystic-quest]");
        assert_eq!(slug("  Mega Man 3  "), "mega-man-3");
        assert_eq!(slug("Zelda: Link's Awakening"), "zelda-link-s-awakening");
    }

    /// Verified against the live archive: `the-legend-of-zelda` and
    /// `legend-of-zelda` both 404, `legend-of-zelda-the` is a 200.
    #[test]
    fn a_leading_article_is_tried_rotated_to_the_end() {
        let c = slug_candidates("The Legend of Zelda");
        assert!(c.contains(&"legend-of-zelda-the".to_string()), "{c:?}");
        assert_eq!(c[0], "the-legend-of-zelda", "the name as given comes first");
    }

    /// And the other way, because a No-Intro folder name arrives already
    /// rotated and the online catalogues do not use that form.
    #[test]
    fn a_trailing_article_is_tried_at_the_front() {
        let c = slug_candidates("Legend of Zelda, The");
        assert!(c.contains(&"the-legend-of-zelda".to_string()), "{c:?}");
    }

    #[test]
    fn a_name_with_no_article_yields_one_candidate() {
        assert_eq!(slug_candidates("Castlevania"), vec!["castlevania".to_string()]);
    }

    /// Verified against the live archive: that Zelda rip is listed with 37
    /// entries of which 15 are `Track N`.
    #[test]
    fn a_numbered_placeholder_is_not_a_name() {
        assert!(is_placeholder(&Entry { number: 1, title: "Track 1".into(), length: String::new() }));
        assert!(is_placeholder(&Entry { number: 12, title: " Track 12 ".into(), length: String::new() }));
        assert!(!is_placeholder(&Entry { number: 1, title: "Overworld".into(), length: String::new() }));
    }

    /// Position matters: a song really called "Track 3" sitting at nine is a
    /// name, not a placeholder.
    #[test]
    fn a_number_that_is_not_its_own_position_is_a_name() {
        assert!(!is_placeholder(&Entry { number: 9, title: "Track 3".into(), length: String::new() }));
    }

    #[test]
    fn a_placeholder_is_written_as_no_title_but_keeps_its_length() {
        let e = vec![
            Entry { number: 1, title: "Track 1".into(), length: "3:00".into() },
            Entry { number: 2, title: "Overworld".into(), length: "1:30".into() },
        ];
        let m = to_m3u("z.nsf", &e);
        assert!(m.contains("z.nsf::NSF,1,,3:00\n"), "{m}");
        assert!(m.contains("z.nsf::NSF,2,Overworld,1:30\n"), "{m}");
    }

    /// The gate the whole feature rests on. Position *is* the mapping, so a
    /// listing of a different length is a listing of a different rip.
    #[test]
    fn a_listing_of_the_wrong_length_is_refused() {
        let e = parse(CASTLEVANIA);
        assert!(matches_subsongs(&e, 54));
        assert!(!matches_subsongs(&e, 53), "one short is a different rip");
        assert!(!matches_subsongs(&e, 55));
        assert!(!matches_subsongs(&[], 0), "nothing matches nothing");
    }

    #[test]
    fn the_m3u_is_the_shape_the_readers_here_parse() {
        let e = vec![
            Entry { number: 1, title: "Title".into(), length: "1:23".into() },
            Entry { number: 2, title: "World Map".into(), length: String::new() },
        ];
        let m = to_m3u("pokemon.gbs", &e);
        assert!(m.contains("pokemon.gbs::GBS,1,Title,1:23\n"), "{m}");
        assert!(m.contains("pokemon.gbs::GBS,2,World Map,\n"), "{m}");
    }

    /// A comma inside a title is a field separator unless it is escaped, and
    /// game music is full of them.
    #[test]
    fn a_comma_in_a_title_is_escaped() {
        let e = vec![Entry { number: 1, title: "Hello, World".into(), length: String::new() }];
        assert!(to_m3u("x.nsf", &e).contains("x.nsf::NSF,1,Hello\\, World,\n"));
    }
}
