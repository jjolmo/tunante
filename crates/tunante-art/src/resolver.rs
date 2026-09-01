//! Finding one cover, and finding three hundred.
//!
//! The order the chain runs in is not a guess. Measured over the games a real
//! 23,000-file collection could not match in its own console's archive:
//!
//! | step | recovered |
//! |---|---|
//! | the console's own Libretro archive | 320 of 473 |
//! | every *other* Libretro archive | +24 |
//! | iTunes | +30 |
//! | Steam | +20 |
//! | Deezer | +8 |
//! | Nintendo | +8 |
//! | Wikipedia | 0 |
//!
//! Box art from the right game on the wrong platform beats an album cover that
//! merely shares a name, which is why the cross-archive step comes before the
//! storefronts. Wikipedia finds plenty on its own but nothing the steps above
//! missed, so it stays last and stays [`Confidence::Low`].

use crate::folder::{self, Overwrite, Stored};
use crate::http::{Http, UreqHttp};
use crate::image::{self, ImageInfo};
use crate::index::Index;
use crate::search::{self, Archive};
use crate::{archive, cache, name, sources, ArtError, Confidence};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// One game to find a cover for.
#[derive(Debug, Clone)]
pub struct CoverRequest {
    /// Names to try, best first — normally the rip's own album tag, then the
    /// folder it sits in. Every one is tried and the most confident wins; the
    /// old code stopped at the first that returned any bytes, which let a dirty
    /// tag beat a clean folder name purely by being first.
    pub candidates: Vec<String>,
    /// The console this was filed under. `""` when unknown.
    pub console_id: String,
    /// This console's Libretro archive directory, from `tunante_core::console`.
    /// `None` for a machine with no archive — Switch, PC.
    pub libretro_system: Option<String>,
    /// Every other archive, for the multiplatform case.
    pub other_systems: Vec<(String, String)>,
    /// Where a found cover would be written. `None` to resolve without writing.
    pub dir: Option<PathBuf>,
}

impl CoverRequest {
    fn primary(&self) -> &str {
        self.candidates.first().map(|s| s.as_str()).unwrap_or("")
    }

    fn cache_key(&self) -> String {
        let normalized = name::normalize(self.primary()).key;
        cache::key(cache::Kind::Game, &normalized, &self.console_id)
    }
}

/// The names worth trying for one track, best first.
///
/// All three apps build a request the same way, so the rule lives here.
///
/// The folder is included even when the classifier already chose a name, and
/// that is the point rather than belt-and-braces. A rip's album tag is the
/// *soundtrack's* title, which is often not the game's: a track under
/// `NDS/Final Fantasy Tactics A2/` is tagged
/// "Final Fantasy Tactics A2: The Sealed Grimoire", while the archive calls the
/// game "Final Fantasy Tactics A2 - Grimoire of the Rift". The tag matches
/// nothing and the folder matches exactly. The reverse is just as common —
/// a folder called `ct` whose tag says "Chrono Trigger" — which is why both go
/// in and the most confident answer wins.
pub fn candidates_for(game: &str, album: &str, path: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim();
        if s.len() >= 2 && !out.iter().any(|x: &String| x.eq_ignore_ascii_case(s)) {
            out.push(s.to_string());
        }
    };
    push(game);
    push(album);

    // The folder the track sits in — and the one above it *only* when this one
    // is a disc, as in `Genshin Impact/Disc 2 - Blazing Stars/`. Going up
    // unconditionally offers the console folder as a game name, so a DS track
    // arrived here asking the archive about "NDS".
    let real = path.split('#').next().unwrap_or(path);
    let parent = std::path::Path::new(real).parent();
    if let Some(name) = parent.and_then(|d| d.file_name()).and_then(|n| n.to_str()) {
        push(name);
        if looks_like_a_disc(name) {
            if let Some(up) = parent.and_then(|d| d.parent()) {
                if let Some(n) = up.file_name().and_then(|n| n.to_str()) {
                    push(n);
                }
            }
        }
    }
    out
}

/// `Disc 2`, `CD1`, `Disc 3 - Bonus Tracks`.
///
/// A narrower copy of the same idea in `tunante_core::classify`, kept here
/// because this crate deliberately does not depend on that one and because this
/// only has to recognise the folder, not classify it.
fn looks_like_a_disc(name: &str) -> bool {
    let lower = name.to_lowercase();
    let mut words = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty());
    let Some(first) = words.next() else { return false };
    let split = first.find(|c: char| c.is_ascii_digit()).unwrap_or(first.len());
    let (word, fused) = first.split_at(split);
    let word = if word.is_empty() { first } else { word };
    if !matches!(word, "disc" | "disk" | "cd" | "dvd" | "vol" | "volume") {
        return matches!(lower.as_str(), "bonus" | "extras" | "extra");
    }
    let number = if fused.is_empty() { words.next().unwrap_or("") } else { fused };
    number.parse::<u32>().is_ok_and(|n| (1..=20).contains(&n))
}

/// A cover, downloaded and checked.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub bytes: Vec<u8>,
    pub info: ImageInfo,
    pub confidence: Confidence,
    pub source: String,
    /// What the source called it, so a person can tell whether it is right.
    pub matched_name: String,
}

/// What a bulk run decided about one game, without doing it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    pub game: String,
    pub console_id: String,
    pub source: String,
    pub matched_name: String,
    pub confidence: Confidence,
    /// `None` when nothing was found.
    pub url: Option<String>,
    /// The image already in the folder, which is left alone. Recording this is
    /// *not* what undo wants — see `written`.
    pub existing: Option<String>,
    /// The file this run created, if it created one.
    ///
    /// This, and only this, is what may be undone. Conflating it with
    /// `existing` would make "undo" delete the user's own artwork — the exact
    /// files [`crate::folder::store_cover`] refuses to overwrite.
    pub written: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BulkProgress {
    pub done: usize,
    pub total: usize,
    pub found: usize,
    pub written: usize,
    pub skipped: usize,
    pub current: String,
}

pub struct BulkOptions {
    /// Report what would happen, write nothing.
    pub dry_run: bool,
    /// Anything below this is reported but not applied. Defaults to
    /// [`Confidence::High`], because the output is a permanent write into
    /// someone's library and a wrong cover is worse than no cover.
    pub min_confidence: Confidence,
    pub overwrite: Overwrite,
    pub cancel: Arc<AtomicBool>,
    /// Bounded by the remote hosts, not by the CPU — see [`WORKERS`].
    pub workers: usize,
}

impl Default for BulkOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            min_confidence: Confidence::High,
            overwrite: Overwrite::Never,
            cancel: Arc::new(AtomicBool::new(false)),
            workers: WORKERS,
        }
    }
}

/// Four, fixed. Not `available_parallelism`: the limit here is how fast four
/// public archives are willing to be asked, which has nothing to do with how
/// many cores this machine has. The scan pool next door counts cores because
/// probing really is CPU-bound.
pub const WORKERS: usize = 4;

/// How many rows one archive may contribute to the picker. Enough to show the
/// regional variants of the right game, few enough that they do not push the
/// other sources off the end of the list.
const ARCHIVE_OPTIONS: usize = 8;

/// One archive entry as something to offer.
///
/// The confidence is what it is worth as an *answer*, not how well it sorted:
/// the same name is the same game, anything else is a suggestion, and a hit
/// from a platform that did not corroborate it is one step less either way —
/// the same rule [`crate::search`] applies.
fn archive_hit(system: &str, entry: &crate::index::Entry, query: &str, same_console: bool) -> sources::Hit {
    let equal = name::normalize(query).keys().any(|k| entry.norm.keys().any(|e| e == k));
    let confidence = match (equal, same_console) {
        (true, true) => Confidence::Exact,
        (true, false) => Confidence::High,
        (false, true) => Confidence::Medium,
        (false, false) => Confidence::Low,
    };
    sources::Hit {
        url: archive::cover_url(system, &entry.file),
        confidence,
        source: if same_console { "libretro" } else { "libretro-other" },
        matched_name: entry.file.clone(),
    }
}

pub struct Resolver {
    http: Arc<dyn Http>,
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Resolver {
    pub fn new() -> Self {
        Self { http: Arc::new(UreqHttp::new()) }
    }

    /// For tests, and as the escape hatch if a caller must supply its own stack.
    pub fn with_http(http: Arc<dyn Http>) -> Self {
        Self { http }
    }

    /// Where a cover for this request could come from, without downloading it.
    pub fn find(&self, req: &CoverRequest) -> Option<sources::Hit> {
        let usable: Vec<String> =
            req.candidates.iter().filter(|c| !c.trim().is_empty()).cloned().collect();
        if usable.is_empty() {
            return None;
        }

        // 1 & 2. The archives, own console first.
        let own_index: Option<Arc<Index>> = req
            .libretro_system
            .as_ref()
            .and_then(|s| archive::index_for(self.http.as_ref(), s).ok());
        let others: Vec<(String, Arc<Index>)> = req
            .other_systems
            .iter()
            .filter_map(|(cid, sys)| {
                archive::index_for(self.http.as_ref(), sys).ok().map(|i| (cid.clone(), i))
            })
            .collect();

        let own_arch = own_index.as_ref().map(|i| Archive {
            console_id: req.console_id.as_str(),
            index: i.as_ref(),
        });
        let other_arch: Vec<Archive> = others
            .iter()
            .map(|(cid, i)| Archive { console_id: cid.as_str(), index: i.as_ref() })
            .collect();

        if let Some(found) = search::find(own_arch.as_ref(), &other_arch, &usable) {
            let system = if found.same_console {
                req.libretro_system.clone()
            } else {
                req.other_systems
                    .iter()
                    .find(|(cid, _)| *cid == found.console_id)
                    .map(|(_, s)| s.clone())
            };
            let index = if found.same_console {
                own_index.as_ref()
            } else {
                others.iter().find(|(cid, _)| *cid == found.console_id).map(|(_, i)| i)
            };
            if let (Some(system), Some(index)) = (system, index) {
                let entry = &index.entries[found.entry_index];
                return Some(sources::Hit {
                    url: archive::cover_url(&system, &entry.file),
                    confidence: found.confidence,
                    source: if found.same_console { "libretro" } else { "libretro-other" },
                    matched_name: entry.file.clone(),
                });
            }
        }

        // 3. The keyless storefronts and album services, in measured order.
        for (_, lookup) in sources::CHAIN {
            for c in &usable {
                if let Some(hit) = lookup(self.http.as_ref(), c) {
                    return Some(hit);
                }
            }
        }
        None
    }

    /// Every cover on offer for one request, for a person to choose from.
    ///
    /// The list the automatic path never shows. [`Resolver::find`] returns the
    /// one answer it is willing to write unattended and stops at the first
    /// source that has it, which is right for a bulk run and useless when the
    /// answer is wrong: there is nothing to correct it *with*. This asks
    /// everything and returns everything, ordered the way the chain is ordered,
    /// and lets the person decide.
    ///
    /// `query` is a name they typed. Without one this falls back to the same
    /// candidates the automatic path uses, so opening the picker on a track
    /// shows what the download would have found.
    ///
    /// Only the console's own archive is scanned loosely. The other twenty are
    /// asked for the name outright — a fuzzy match against 46,000 titles from
    /// archives that do not corroborate the platform is a lottery, which is the
    /// same reasoning as [`crate::search`], and here it would also be twenty
    /// scans deep in a user's keystroke.
    pub fn options(&self, req: &CoverRequest, query: Option<&str>, limit: usize) -> Vec<sources::Hit> {
        let names: Vec<String> = match query.map(str::trim).filter(|q| !q.is_empty()) {
            Some(q) => vec![q.to_string()],
            None => req.candidates.iter().filter(|c| !c.trim().is_empty()).cloned().collect(),
        };
        if names.is_empty() {
            return Vec::new();
        }

        let mut out: Vec<sources::Hit> = Vec::new();
        let push = |hit: sources::Hit, out: &mut Vec<sources::Hit>| {
            if !out.iter().any(|h| h.url == hit.url) {
                out.push(hit);
            }
        };

        // 1. This console's own archive, loosely.
        if let Some(system) = req.libretro_system.as_deref() {
            if let Ok(index) = archive::index_for(self.http.as_ref(), system) {
                for name in &names {
                    for i in index.candidates(name, ARCHIVE_OPTIONS) {
                        let entry = &index.entries[i];
                        push(archive_hit(system, entry, name, true), &mut out);
                    }
                }
            }
        }

        // 2. Every other archive, on the name outright. This is where a Game
        //    Boy folder holding a Game Boy Color game gets its box art.
        for name in &names {
            if name::normalize(name).key.len() < search::MIN_CROSS_LEN {
                continue;
            }
            for (cid, system) in &req.other_systems {
                if *cid == req.console_id {
                    continue;
                }
                let Ok(index) = archive::index_for(self.http.as_ref(), system) else { continue };
                // One per archive: the rest are the same game's other regions,
                // and twenty archives' worth of those would bury the sources
                // below.
                if let Some(&i) = index.exact(name).first() {
                    push(archive_hit(system, &index.entries[i], name, false), &mut out);
                }
            }
        }

        // 3. The storefronts and album services, loosely.
        for name in &names {
            for hit in sources::search_covers(self.http.as_ref(), name) {
                push(hit, &mut out);
            }
        }

        out.truncate(limit);
        out
    }

    /// Take a cover somebody chose and make it this game\'s.
    ///
    /// Everything the automatic path would have decided is already decided, so
    /// this only downloads, checks and keeps — but it keeps in both places, and
    /// missing either one makes the choice look like it did not take. The cache
    /// is what the player reads for the artwork panel, and the folder is what
    /// survives a rescan and reaches the phone.
    ///
    /// [`Overwrite::Replace`], unconditionally. The rule that protects the
    /// user\'s own artwork exists because a *download* must not overwrite it;
    /// this is the user pointing at an image and saying to use that one.
    pub fn fetch_chosen(&self, req: &CoverRequest, url: &str) -> Result<Resolved, ArtError> {
        let resp = self.http.get(url, image::MAX_BYTES)?;
        if !resp.is_success() {
            return Err(ArtError::Http { status: resp.status, url: url.to_string() });
        }
        let info = image::inspect(&resp.body)?;

        let key = req.cache_key();
        cache::forget(&key);
        let _ = cache::put(&key, &resp.body);

        if let Some(dir) = req.dir.as_ref() {
            folder::store_cover(dir, &resp.body, &info, Overwrite::Replace)?;
        }

        Ok(Resolved {
            bytes: resp.body,
            info,
            confidence: Confidence::Exact,
            source: "chosen".into(),
            matched_name: req.primary().to_string(),
        })
    }

    /// Throw away whatever is remembered about this request.
    ///
    /// For "that cover is wrong, try again": the cache is doing its job when it
    /// returns the same answer, and its job is what has to be bypassed.
    pub fn forget(&self, req: &CoverRequest) {
        cache::forget(&req.cache_key());
    }

    /// Find, download, validate, cache.
    ///
    /// Returns `Ok(None)` when nothing was found — which is an answer, not an
    /// error, and is remembered for [`cache::MISS_TTL`] so a bulk run does not
    /// ask the same four services about the same game every time.
    pub fn resolve(&self, req: &CoverRequest) -> Result<Option<Resolved>, ArtError> {
        let key = req.cache_key();
        if let Some(bytes) = cache::get(&key) {
            if let Ok(info) = image::inspect(&bytes) {
                return Ok(Some(Resolved {
                    bytes,
                    info,
                    confidence: Confidence::Exact,
                    source: "cache".into(),
                    matched_name: req.primary().to_string(),
                }));
            }
        }
        if cache::is_fresh_miss(&key) {
            return Ok(None);
        }

        let Some(hit) = self.find(req) else {
            cache::record_miss(&key);
            return Ok(None);
        };

        let resp = self.http.get(&hit.url, image::MAX_BYTES)?;
        if !resp.is_success() {
            cache::record_miss(&key);
            return Ok(None);
        }
        // Never trust the status alone: Nintendo's media host answers 200 with
        // an HTML 404 page, and the old code's only check was a length.
        let info = match image::inspect(&resp.body) {
            Ok(i) => i,
            Err(e) => {
                log::warn!("{} offered {} which is not a cover: {e}", hit.source, hit.url);
                cache::record_miss(&key);
                return Ok(None);
            }
        };
        let _ = cache::put(&key, &resp.body);

        Ok(Some(Resolved {
            bytes: resp.body,
            info,
            confidence: hit.confidence,
            source: hit.source.to_string(),
            matched_name: hit.matched_name,
        }))
    }

    /// Resolve and, when the confidence is high enough, write into the folder.
    pub fn resolve_and_store(
        &self,
        req: &CoverRequest,
        min_confidence: Confidence,
        overwrite: Overwrite,
    ) -> Result<(Option<Resolved>, Option<Stored>), ArtError> {
        let Some(found) = self.resolve(req)? else { return Ok((None, None)) };
        let Some(dir) = req.dir.as_ref() else { return Ok((Some(found), None)) };
        if found.confidence < min_confidence {
            return Ok((Some(found), None));
        }
        let stored = folder::store_cover(dir, &found.bytes, &found.info, overwrite)?;
        Ok((Some(found), Some(stored)))
    }

    /// A whole library, or one folder of it.
    ///
    /// Progress is reported through `on_progress`, called on the calling thread
    /// — the same shape as the library scan's pool, so there is one idiom in
    /// this repository for "long job with a progress bar", not two.
    pub fn resolve_many(
        self: &Arc<Self>,
        requests: Vec<CoverRequest>,
        opts: &BulkOptions,
        mut on_progress: impl FnMut(&BulkProgress),
    ) -> Vec<Plan> {
        let total = requests.len();
        let queue = Arc::new(Mutex::new(requests.into_iter().enumerate()));
        let (tx, rx) = std::sync::mpsc::channel::<(usize, Plan, bool)>();
        let mut handles = Vec::new();

        for _ in 0..opts.workers.max(1) {
            let queue = Arc::clone(&queue);
            let tx = tx.clone();
            let me = Arc::clone(self);
            let cancel = Arc::clone(&opts.cancel);
            let (dry, min_conf, overwrite) = (opts.dry_run, opts.min_confidence, opts.overwrite);
            handles.push(std::thread::spawn(move || loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let Some((i, req)) = queue.lock().unwrap_or_else(|e| e.into_inner()).next() else {
                    return;
                };
                let game = req.primary().to_string();
                let existing = req.dir.as_ref().and_then(|d| folder::folder_image(d));

                let (plan, wrote) = if dry {
                    match me.find(&req) {
                        Some(h) => (
                            Plan {
                                game,
                                console_id: req.console_id.clone(),
                                source: h.source.to_string(),
                                matched_name: h.matched_name,
                                confidence: h.confidence,
                                url: Some(h.url),
                                existing: existing.map(|p| p.display().to_string()),
                                // A preview writes nothing.
                                written: None,
                            },
                            false,
                        ),
                        None => (miss(game, &req, existing), false),
                    }
                } else {
                    match me.resolve_and_store(&req, min_conf, overwrite) {
                        Ok((Some(found), stored)) => {
                            let written = match &stored {
                                Some(Stored::Written(p)) => Some(p.display().to_string()),
                                _ => None,
                            };
                            (
                                Plan {
                                    game,
                                    console_id: req.console_id.clone(),
                                    source: found.source,
                                    matched_name: found.matched_name,
                                    confidence: found.confidence,
                                    url: None,
                                    existing: existing.map(|p| p.display().to_string()),
                                    written: written.clone(),
                                },
                                written.is_some(),
                            )
                        }
                        Ok((None, _)) => (miss(game, &req, existing), false),
                        Err(e) => {
                            log::warn!("cover for {game}: {e}");
                            (miss(game, &req, existing), false)
                        }
                    }
                };
                if tx.send((i, plan, wrote)).is_err() {
                    return;
                }
            }));
        }
        drop(tx);

        let mut out: Vec<Option<Plan>> = (0..total).map(|_| None).collect();
        let mut p = BulkProgress { total, ..Default::default() };
        for (i, plan, wrote) in rx {
            p.done += 1;
            if plan.url.is_some() || plan.source != "none" {
                p.found += 1;
            }
            if wrote {
                p.written += 1;
            } else if plan.existing.is_some() {
                p.skipped += 1;
            }
            p.current = plan.game.clone();
            out[i] = Some(plan);
            on_progress(&p);
        }
        for h in handles {
            let _ = h.join();
        }
        out.into_iter().flatten().collect()
    }
}

fn miss(game: String, req: &CoverRequest, existing: Option<PathBuf>) -> Plan {
    Plan {
        game,
        console_id: req.console_id.clone(),
        source: "none".into(),
        matched_name: String::new(),
        confidence: Confidence::Low,
        url: None,
        existing: existing.map(|p| p.display().to_string()),
        written: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::FakeHttp;

    fn req(names: &[&str]) -> CoverRequest {
        CoverRequest {
            candidates: names.iter().map(|s| s.to_string()).collect(),
            console_id: "snes".into(),
            libretro_system: None,
            other_systems: Vec::new(),
            dir: None,
        }
    }

    #[test]
    fn nothing_to_go_on_is_not_a_lookup() {
        let http = Arc::new(FakeHttp::new());
        let r = Resolver::with_http(http.clone());
        assert!(r.find(&req(&["", "   "])).is_none());
        assert!(http.requested.lock().unwrap().is_empty(), "asked anyway");
    }

    /// The chain must reach the storefronts when there is no archive for this
    /// console — which is the entire Switch and PC case.
    #[test]
    fn a_console_with_no_archive_still_gets_a_chance() {
        let http = Arc::new(FakeHttp::new().with(
            "https://itunes.apple.com/search?term=Celeste%20soundtrack&media=music&entity=album&limit=6",
            200,
            r#"{"results":[{"collectionName":"Celeste (Original Soundtrack)",
                 "artworkUrl100":"https://x/100x100bb.jpg"}]}"#,
        ));
        let r = Resolver::with_http(http);
        let hit = r.find(&req(&["Celeste"])).unwrap();
        assert_eq!(hit.source, "itunes");
    }

    /// A source that offers something which is not an image must not have it
    /// written into anyone's music folder.
    #[test]
    fn an_html_error_page_is_not_stored_as_a_cover() {
        let http = Arc::new(
            FakeHttp::new()
                .with(
                    "https://itunes.apple.com/search?term=Celeste%20soundtrack&media=music&entity=album&limit=6",
                    200,
                    r#"{"results":[{"collectionName":"Celeste","artworkUrl100":"https://x/100x100bb.jpg"}]}"#,
                )
                .with("https://x/600x600bb.jpg", 200, "<!DOCTYPE html><html>404</html>"),
        );
        let tmp = std::env::temp_dir().join(format!("tunante-art-res-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("TUNANTE_CACHE_DIR", &tmp);

        let r = Resolver::with_http(http);
        let mut q = req(&["Celeste"]);
        q.dir = Some(tmp.clone());
        let (found, stored) = r.resolve_and_store(&q, Confidence::High, Overwrite::Never).unwrap();
        assert!(found.is_none(), "an HTML page was accepted as a cover");
        assert!(stored.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_bulk_run_reports_one_plan_per_game_in_order() {
        let http = Arc::new(FakeHttp::new());
        let r = Arc::new(Resolver::with_http(http));
        let reqs = vec![req(&["Alpha"]), req(&["Beta"]), req(&["Gamma"])];
        let opts = BulkOptions { dry_run: true, ..Default::default() };
        let mut seen = 0;
        let plans = r.resolve_many(reqs, &opts, |_| seen += 1);
        assert_eq!(plans.len(), 3);
        assert_eq!(seen, 3, "progress was not reported for every game");
        assert_eq!(
            plans.iter().map(|p| p.game.as_str()).collect::<Vec<_>>(),
            ["Alpha", "Beta", "Gamma"],
            "results came back out of order"
        );
    }

    /// A minutes-long run over someone's whole library has to be stoppable.
    #[test]
    fn a_cancelled_run_stops() {
        let http = Arc::new(FakeHttp::new());
        let r = Arc::new(Resolver::with_http(http));
        let cancel = Arc::new(AtomicBool::new(true));
        let opts = BulkOptions { dry_run: true, cancel, ..Default::default() };
        let reqs: Vec<CoverRequest> = (0..50).map(|i| req(&[&format!("Game {i}")])).collect();
        assert!(r.resolve_many(reqs, &opts, |_| {}).is_empty());
    }

    /// The bug this field exists for.
    ///
    /// `existing` is the image that was already in the folder and was left
    /// alone; `written` is what the run created. Recording the wrong one in the
    /// undo manifest — which is what the first version of the Tauri command did
    /// — turns "undo the last run" into "delete the covers the user chose".
    /// A test on `Manifest` alone passed happily while that was true, because
    /// the mistake was in the wiring.
    #[test]
    fn a_run_records_what_it_wrote_and_not_what_it_found() {
        let d = std::env::temp_dir().join(format!("tunante-art-undo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // The user's own artwork, which store_cover must never touch.
        std::fs::write(d.join("front.png"), b"mine").unwrap();

        let http = Arc::new(FakeHttp::new());
        let r = Arc::new(Resolver::with_http(http));
        let mut q = req(&["Whatever"]);
        q.dir = Some(d.clone());
        let opts = BulkOptions::default();
        let plans = r.resolve_many(vec![q], &opts, |_| {});

        assert_eq!(plans.len(), 1);
        assert!(plans[0].written.is_none(), "nothing was written, so nothing is undoable");
        assert!(
            plans[0].existing.as_deref().is_some_and(|e| e.ends_with("front.png")),
            "the user's image should be reported as existing"
        );
        // The two must never be the same value: undo consumes `written`.
        assert_ne!(plans[0].written, plans[0].existing);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The bug that made a real library show no cover for a game the archive
    /// plainly has.
    #[test]
    fn the_folder_is_tried_even_when_the_tag_named_something_else() {
        let c = candidates_for(
            "Final Fantasy Tactics A2: The Sealed Grimoire",
            "Final Fantasy Tactics A2: The Sealed Grimoire",
            "/m/OST/NDS/Final Fantasy Tactics A2/101 Main Theme.mini2sf",
        );
        assert!(
            c.iter().any(|x| x == "Final Fantasy Tactics A2"),
            "the folder names the game and was not offered: {c:?}"
        );
        // The tag is still first: it is what rescues an abbreviated folder.
        assert_eq!(c[0], "Final Fantasy Tactics A2: The Sealed Grimoire");
    }

    /// ...and the abbreviated-folder case still works the other way round.
    #[test]
    fn the_tag_still_leads_for_an_abbreviated_folder() {
        let c = candidates_for("Chrono Trigger", "Chrono Trigger", "/m/OST/snes spc osts/ct/01.spc");
        assert_eq!(c[0], "Chrono Trigger");
        assert!(c.iter().any(|x| x == "ct"));
    }

    /// A disc folder is not a game, so the one above it goes in too.
    #[test]
    fn a_disc_folder_offers_its_parent() {
        let c = candidates_for("", "", "/m/OST/PC/Genshin Impact/Disc 2 - Blazing Stars/01.mp3");
        assert!(c.iter().any(|x| x == "Genshin Impact"), "{c:?}");
    }

    /// ...but an ordinary game folder must not offer the console above it. A DS
    /// track was asking the archive about a game called "NDS".
    #[test]
    fn an_ordinary_folder_does_not_offer_the_console_above_it() {
        let c = candidates_for("", "", "/m/OST/NDS/Final Fantasy Tactics A2/101.mini2sf");
        assert!(c.iter().any(|x| x == "Final Fantasy Tactics A2"), "{c:?}");
        assert!(!c.iter().any(|x| x == "NDS"), "the console folder was offered: {c:?}");
    }

    #[test]
    fn a_subsong_suffix_does_not_leak_into_a_candidate() {
        let c = candidates_for("", "", "/m/GB/Pokemon Blue/pokemon.gbs#7");
        assert!(c.iter().any(|x| x == "Pokemon Blue"), "{c:?}");
    }

    /// The default has to be the safe one: this writes into a synced library.
    #[test]
    fn the_defaults_do_not_overwrite_and_do_not_apply_guesses() {
        let o = BulkOptions::default();
        assert_eq!(o.overwrite, Overwrite::Never);
        assert_eq!(o.min_confidence, Confidence::High);
        assert!(!o.dry_run);
    }
}
