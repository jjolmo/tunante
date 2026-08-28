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

    /// The default has to be the safe one: this writes into a synced library.
    #[test]
    fn the_defaults_do_not_overwrite_and_do_not_apply_guesses() {
        let o = BulkOptions::default();
        assert_eq!(o.overwrite, Overwrite::Never);
        assert_eq!(o.min_confidence, Confidence::High);
        assert!(!o.dry_run);
    }
}
