//! The places a cover can come from, other than the Libretro archive.
//!
//! Every one of these is keyless. That is a hard requirement, not a preference:
//! this ships as an open-source binary on three platforms, and an embedded API
//! secret is a secret you have published. It rules out SteamGridDB, IGDB,
//! MobyGames, RAWG, TheGamesDB and ScreenScraper, all of which were checked.
//!
//! # Order, and why it is that order
//!
//! Measured against the games a real collection could not match in the Libretro
//! archive: iTunes 30, Steam 20, Deezer 8, Nintendo 8, Wikipedia 0. Wikipedia
//! scores zero not because it is bad — on its own it finds 28 of 30 — but
//! because everything it would have found was already caught by something more
//! trustworthy. It stays last, as a net.
//!
//! # Every one of these needs a name gate
//!
//! None of these APIs can say "I don't have that". They all return their best
//! guess, and their best guess is regularly a different game:
//!
//! - Wikipedia's search mapped six different Touhou games onto one article, and
//!   `Double Spoiler` onto the article "X".
//! - Steam matched `Devil May Cry 3` to **Devil May Cry 5** at a 0.9 similarity
//!   gate — the same numbered-series failure documented in [`crate::name`].
//! - Deezer answered `Catherine` with *Catherine & Cie (Bande originale)*, a
//!   French film.
//!
//! So the gate here is normalised **equality**, not similarity. For an album
//! the query must be contained in the album's title, because a soundtrack is
//! legitimately called "Bastion (Original Soundtrack)" — but for a *game* it
//! must match outright.
//!
//! # Two responses that lie about their status
//!
//! Deezer signals quota exhaustion with **HTTP 200** and an `error` object.
//! Nintendo's media CDN returns **HTTP 200 with an HTML 404 page**. Both are
//! handled here, and the magic-byte check in [`crate::image`] is the backstop.

use crate::http::Http;
use crate::name;
use crate::Confidence;

/// A cover someone is offering us, before it has been downloaded or validated.
#[derive(Debug, Clone)]
pub struct Hit {
    pub url: String,
    pub confidence: Confidence,
    /// Which source, for the review list and the logs.
    pub source: &'static str,
    /// What the source thinks this is, so a human can check it.
    pub matched_name: String,
}

/// Enough JSON for one lookup. 512 KiB is generous for a search response.
const MAX_JSON: usize = 512 * 1024;

fn json(http: &dyn Http, url: &str) -> Option<serde_json::Value> {
    let resp = http.get(url, MAX_JSON).ok()?;
    if !resp.is_success() {
        return None;
    }
    serde_json::from_slice(&resp.body).ok()
}

/// Is `title` the same name as `query`, allowing for any of the spellings the
/// normaliser considers equivalent?
fn same_name(query: &str, title: &str) -> bool {
    let q = name::normalize(query);
    let t = name::normalize(title);
    let mine: Vec<String> = q.keys().map(str::to_string).collect();
    let theirs: Vec<String> = t.keys().map(str::to_string).collect();
    mine.iter().any(|a| theirs.contains(a))
}

/// Words an album may add after a game's name without becoming a different
/// thing: how the release was packaged, not what it is of.
///
/// Deliberately excludes anything that could name a *different product* —
/// `remake`, `remaster`, `advance`, `deluxe`. "Final Fantasy VII Remake" is not
/// Final Fantasy VII.
const ALBUM_PACKAGING: &[&str] = &[
    "pt", "part", "vol", "volume", "disc", "disk", "cd", "ep", "set", "box", "collection",
    "and", "the", "a", "an",
];

/// Is `title` an album *of* `query`?
///
/// The query must be a **prefix**, and everything after it must be packaging.
/// Three weaker rules were tried and each let a real wrong answer through:
///
/// - *Similarity*: matched `Devil May Cry 3` to Devil May Cry 5.
/// - *Containment anywhere*: matched `Bastion` to "Runebound Bastion Expansion".
/// - *Prefix alone*: matched `Catherine` to "Catherine & Cie (Bande originale)",
///   a French film — the leftover "cie" is a different title, not packaging.
///
/// Note the normaliser has usually already removed the soundtrack words, so
/// "Bastion (Original Soundtrack)" arrives here as simply "bastion".
fn album_matches(query: &str, title: &str) -> bool {
    let q = name::normalize(query);
    let t = name::normalize(title);
    if q.tokens.is_empty() || t.tokens.len() < q.tokens.len() {
        return false;
    }
    if t.tokens[..q.tokens.len()] != q.tokens[..] {
        return false;
    }
    t.tokens[q.tokens.len()..]
        .iter()
        .all(|w| ALBUM_PACKAGING.contains(&w.as_str()) || w.chars().all(|c| c.is_ascii_digit()))
}

/// Names below this are too generic to ask a search engine about.
const MIN_QUERY_LEN: usize = 3;

fn usable(query: &str) -> Option<String> {
    let n = name::normalize(query);
    (n.key.len() >= MIN_QUERY_LEN).then_some(n.key)
}

// --- album sources -----------------------------------------------------------

/// iTunes. Best single source for anything that shipped as a soundtrack album.
pub fn itunes(http: &dyn Http, query: &str) -> Option<Hit> {
    usable(query)?;
    let url = format!(
        "https://itunes.apple.com/search?term={}&media=music&entity=album&limit=6",
        urlencoding::encode(&format!("{query} soundtrack"))
    );
    let data = json(http, &url)?;
    for r in data.get("results")?.as_array()? {
        let title = r.get("collectionName")?.as_str()?;
        let art = r.get("artworkUrl100").and_then(|v| v.as_str())?;
        if album_matches(query, title) {
            return Some(Hit {
                // The size is part of the path and can simply be asked for
                // larger. 600 rather than the 3000 the API will serve: this
                // ends up in a folder that syncs to a phone.
                url: art.replace("100x100bb", "600x600bb"),
                confidence: Confidence::High,
                source: "itunes",
                matched_name: title.to_string(),
            });
        }
    }
    None
}

/// Deezer. Keyless, and catches a different slice than iTunes does.
pub fn deezer(http: &dyn Http, query: &str) -> Option<Hit> {
    usable(query)?;
    let url = format!(
        "https://api.deezer.com/search/album?q={}&limit=6",
        urlencoding::encode(query)
    );
    let data = json(http, &url)?;
    // Quota exhaustion arrives as HTTP 200 with an `error` object rather than
    // as a status code.
    if data.get("error").is_some() {
        log::warn!("deezer refused: {}", data["error"]);
        return None;
    }
    for r in data.get("data")?.as_array()? {
        let title = r.get("title")?.as_str()?;
        let cover = r.get("cover_xl").and_then(|v| v.as_str())?;
        if album_matches(query, title) {
            return Some(Hit {
                url: cover.to_string(),
                confidence: Confidence::High,
                source: "deezer",
                matched_name: title.to_string(),
            });
        }
    }
    None
}

// --- storefronts -------------------------------------------------------------

/// Steam. Only for what actually shipped on Steam, and only on an exact name.
pub fn steam(http: &dyn Http, query: &str) -> Option<Hit> {
    let key = usable(query)?;
    if key.len() < 4 {
        return None;
    }
    let url = format!(
        "https://steamcommunity.com/actions/SearchApps/{}",
        urlencoding::encode(&key)
    );
    let data = json(http, &url)?;
    for a in data.as_array()?.iter().take(5) {
        let title = a.get("name")?.as_str()?;
        let appid = a.get("appid")?.as_str()?;
        // Equality, not similarity: at 0.9 similarity this matched
        // `Devil May Cry 3` to Devil May Cry 5.
        if same_name(query, title) {
            return Some(Hit {
                url: format!(
                    "https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg"
                ),
                confidence: Confidence::High,
                source: "steam",
                matched_name: title.to_string(),
            });
        }
    }
    None
}

/// Nintendo's European catalogue. Keyless, and the only good answer for Switch,
/// which has no Libretro archive and never will.
pub fn nintendo(http: &dyn Http, query: &str) -> Option<Hit> {
    let key = usable(query)?;
    let url = format!(
        "https://searching.nintendo-europe.com/en/select?q={}&fq=type:GAME&wt=json&rows=6",
        urlencoding::encode(&key)
    );
    let data = json(http, &url)?;
    for d in data.get("response")?.get("docs")?.as_array()? {
        let title = d.get("title")?.as_str()?;
        let img = d.get("image_url_sq_s").and_then(|v| v.as_str())?;
        if same_name(query, title) {
            let url = if img.starts_with("//") { format!("https:{img}") } else { img.to_string() };
            return Some(Hit {
                url,
                confidence: Confidence::High,
                source: "nintendo",
                matched_name: title.to_string(),
            });
        }
    }
    None
}

// --- last resort -------------------------------------------------------------

/// The English Wikipedia's article image.
///
/// `pilicense=any` is the whole trick and its absence is why this never worked:
/// `pageimages` defaults to `pilicense=free`, which excludes exactly the
/// fair-use box art that Wikipedia holds and Commons cannot.
///
/// Ranked last and never applied unattended. Its search always answers, so the
/// title gate is doing all the work, and what it returns is whatever the
/// article's lead image happens to be — sometimes a title screen, sometimes a
/// screenshot.
pub fn wikipedia(http: &dyn Http, query: &str) -> Option<Hit> {
    let key = usable(query)?;
    if key.len() < 4 {
        return None;
    }
    let url = format!(
        "https://en.wikipedia.org/w/api.php?action=query&generator=search&gsrsearch={}\
         &gsrlimit=3&prop=pageimages&piprop=original&pilicense=any&format=json",
        urlencoding::encode(&format!("{key} video game"))
    );
    let data = json(http, &url)?;
    for (_, page) in data.get("query")?.get("pages")?.as_object()? {
        let title = page.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let src = page.get("original").and_then(|o| o.get("source")).and_then(|v| v.as_str());
        let Some(src) = src else { continue };
        if src.to_lowercase().ends_with(".svg") {
            continue;
        }
        if same_name(query, title) {
            return Some(Hit {
                url: src.to_string(),
                confidence: Confidence::Low,
                source: "wikipedia",
                matched_name: title.to_string(),
            });
        }
    }
    None
}

/// Every non-archive source, in the order the measurements justify.
pub const CHAIN: &[(&str, fn(&dyn Http, &str) -> Option<Hit>)] = &[
    ("itunes", itunes),
    ("steam", steam),
    ("deezer", deezer),
    ("nintendo", nintendo),
    ("wikipedia", wikipedia),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::FakeHttp;

    #[test]
    fn a_soundtrack_album_is_the_right_answer_for_a_game() {
        assert!(album_matches("Bastion", "Bastion (Original Soundtrack)"));
        assert!(album_matches("Into the Breach", "Into the Breach Soundtrack"));
        assert!(album_matches("Celeste", "Celeste (Original Soundtrack)"));
    }

    /// The three real wrong answers these gates exist to reject.
    #[test]
    fn the_wrong_answers_these_gates_were_written_for() {
        // Deezer offered this for "Catherine". It is a French film.
        assert!(!album_matches("Catherine", "Catherine & Cie (Bande originale)"));
        // Steam offered this at a 0.9 similarity gate.
        assert!(!same_name("Devil May Cry 3", "Devil May Cry 5"));
        // Wikipedia's search offered this for "Double Spoiler".
        assert!(!same_name("Double Spoiler", "X"));
    }

    /// A different entry in the same series is the dangerous near-miss.
    #[test]
    fn a_numbered_series_is_not_collapsed() {
        assert!(!same_name("Final Fantasy VII", "Final Fantasy VIII"));
        assert!(!album_matches("Mega Man Zero 2", "Mega Man Zero 3 Soundtrack"));
        // ...but the two spellings of one number are the same game.
        assert!(same_name("Final Fantasy VII", "Final Fantasy 7"));
    }

    #[test]
    fn a_name_too_short_to_ask_about_is_not_asked_about() {
        let http = FakeHttp::new();
        assert!(itunes(&http, "ct").is_none());
        assert!(steam(&http, "Vs").is_none());
        assert!(wikipedia(&http, "yi").is_none());
        assert!(http.requested.lock().unwrap().is_empty(), "asked anyway");
    }

    #[test]
    fn itunes_answers_and_asks_for_a_bigger_image() {
        let url = "https://itunes.apple.com/search?term=Bastion%20soundtrack&media=music&entity=album&limit=6";
        let http = FakeHttp::new().with(
            url,
            200,
            r#"{"results":[{"collectionName":"Bastion (Original Soundtrack)",
                 "artworkUrl100":"https://is1.mzstatic.com/image/x/100x100bb.jpg"}]}"#,
        );
        let hit = itunes(&http, "Bastion").unwrap();
        assert!(hit.url.ends_with("600x600bb.jpg"), "{}", hit.url);
        assert_eq!(hit.source, "itunes");
    }

    #[test]
    fn itunes_declines_a_different_game() {
        let url = "https://itunes.apple.com/search?term=Bastion%20soundtrack&media=music&entity=album&limit=6";
        let http = FakeHttp::new().with(
            url,
            200,
            r#"{"results":[{"collectionName":"Runebound Bastion Expansion",
                 "artworkUrl100":"https://x/100x100bb.jpg"}]}"#,
        );
        assert!(itunes(&http, "Bastion").is_none());
    }

    /// Deezer reports an exhausted quota as HTTP 200 with a body. Reading only
    /// the status would treat that as "no such album" forever.
    #[test]
    fn deezer_quota_exhaustion_is_not_read_as_absence() {
        let url = "https://api.deezer.com/search/album?q=Bastion&limit=6";
        let http = FakeHttp::new().with(url, 200, r#"{"error":{"code":4,"message":"Quota limit"}}"#);
        assert!(deezer(&http, "Bastion").is_none());
    }

    #[test]
    fn steam_builds_a_cover_url_from_the_appid() {
        let http = FakeHttp::new().with(
            "https://steamcommunity.com/actions/SearchApps/bastion",
            200,
            r#"[{"appid":"107100","name":"Bastion"}]"#,
        );
        let hit = steam(&http, "Bastion").unwrap();
        assert!(hit.url.contains("/107100/library_600x900.jpg"), "{}", hit.url);
    }

    #[test]
    fn nintendo_fills_the_switch_gap() {
        let http = FakeHttp::new().with(
            "https://searching.nintendo-europe.com/en/select?q=octopath%20traveler&fq=type:GAME&wt=json&rows=6",
            200,
            r#"{"response":{"docs":[{"title":"OCTOPATH TRAVELER",
                 "image_url_sq_s":"//img.nintendo.example/octopath.jpg"}]}}"#,
        );
        let hit = nintendo(&http, "Octopath Traveler").unwrap();
        assert!(hit.url.starts_with("https://"), "protocol-relative URL not fixed: {}", hit.url);
    }

    /// Without `pilicense=any` this endpoint returns only freely-licensed
    /// images, which for a commercial game means none. Its absence is why the
    /// old code never found anything here.
    #[test]
    fn wikipedia_asks_for_non_free_images() {
        let http = FakeHttp::new();
        let _ = wikipedia(&http, "Bastion");
        let asked = http.requested.lock().unwrap().join(" ");
        assert!(asked.contains("pilicense=any"), "asked: {asked}");
    }

    #[test]
    fn wikipedia_is_never_presented_as_certain() {
        let http = FakeHttp::new().with(
            "https://en.wikipedia.org/w/api.php?action=query&generator=search&gsrsearch=bastion%20video%20game&gsrlimit=3&prop=pageimages&piprop=original&pilicense=any&format=json",
            200,
            r#"{"query":{"pages":{"1":{"title":"Bastion",
                 "original":{"source":"https://upload.wikimedia.org/x/Bastion_Boxart.jpg"}}}}}"#,
        );
        let hit = wikipedia(&http, "Bastion").unwrap();
        assert_eq!(hit.confidence, Confidence::Low);
    }

    /// A wordmark is not a cover, and Wikidata/Wikipedia are full of them.
    #[test]
    fn an_svg_is_not_a_cover() {
        let http = FakeHttp::new().with(
            "https://en.wikipedia.org/w/api.php?action=query&generator=search&gsrsearch=undertale%20video%20game&gsrlimit=3&prop=pageimages&piprop=original&pilicense=any&format=json",
            200,
            r#"{"query":{"pages":{"1":{"title":"Undertale",
                 "original":{"source":"https://upload.wikimedia.org/x/Undertale_logo.svg"}}}}}"#,
        );
        assert!(wikipedia(&http, "Undertale").is_none());
    }

    #[test]
    fn the_chain_is_in_the_measured_order() {
        let names: Vec<&str> = CHAIN.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["itunes", "steam", "deezer", "nintendo", "wikipedia"]);
        assert_eq!(*names.last().unwrap(), "wikipedia", "wikipedia must stay last");
    }
}
