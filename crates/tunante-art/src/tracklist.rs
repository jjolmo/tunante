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
        let Some(open) = rest.find('>') else { break };
        let Some(close) = rest[open..].find("</td>") else { break };
        let cell = &rest[open + 1..open + close];
        rest = &rest[open + close..];

        let title = strip_tags(cell);
        if title.is_empty() {
            continue;
        }
        out.push(Entry { number: out.len() as u32 + 1, title });
    }
    out
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
    let url = url(system, &slug(game));
    // A listing page is tens of kilobytes; a cap keeps a redirect to something
    // enormous from being read into memory.
    match http.get(&url, 512 * 1024) {
        Ok(r) if r.status == 200 => parse(&String::from_utf8_lossy(&r.body)),
        _ => Vec::new(),
    }
}

/// The listing as a GME-style `.m3u`, the sidecar every player of these
/// formats already reads — this one included.
///
/// Written beside the file rather than into a table so it outlives Tunante:
/// the next player to open that folder gets the names too.
pub fn to_m3u(file_name: &str, entries: &[Entry]) -> String {
    let mut s = String::from("# Generated by Tunante from zophar.net\n");
    for e in entries {
        // `file::TYPE,track,,title` is the shape the readers here parse; the
        // type field is left empty because GME infers it from the extension.
        s.push_str(&format!("{file_name}::,{},,{}\n", e.number, e.title));
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
            Entry { number: 1, title: "Title".into() },
            Entry { number: 2, title: "World Map".into() },
        ];
        let m = to_m3u("pokemon.gbs", &e);
        assert!(m.contains("pokemon.gbs::,1,,Title\n"), "{m}");
        assert!(m.contains("pokemon.gbs::,2,,World Map\n"), "{m}");
    }
}
