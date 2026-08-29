//! Getting a system's box-art listing, and not getting it again for a week.
//!
//! Two sources, in a deliberate order. The primary is the directory listing at
//! `thumbnails.libretro.com` — one request, gzips to about 150 KB, no rate
//! limit. It is also nginx autoindex output rather than an API: no versioning,
//! no stability promise, and it could be turned off tomorrow. So when it stops
//! looking like a listing there is a fallback to the same repository on GitHub
//! through the git-tree API, which *is* a documented contract but allows only
//! 60 requests an hour per IP — plenty behind a 7-day cache, not something to
//! spend by default.

use crate::http::Http;
use crate::index::{self, Index, INDEX_FAIL_TTL_SECS, INDEX_TTL_SECS};
use crate::{cache, ArtError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const AUTOINDEX_HOST: &str = "https://thumbnails.libretro.com";
const GITHUB_API: &str = "https://api.github.com/repos/libretro-thumbnails";
/// The listing is HTML; a few megabytes is far more than any system needs.
const MAX_LISTING_BYTES: usize = 16 * 1024 * 1024;

const FORMAT_VERSION: &str = "tunante-art-index v1";

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A file name for a system, safe on every filesystem this runs on.
fn slug(system: &str) -> String {
    system
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect()
}

fn index_dir() -> std::path::PathBuf {
    let d = cache::cache_dir().join("libretro-index");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Read a cached listing, if it is still fresh.
///
/// The **parsed names** are stored, never the HTML: 3,676 names is about 150 KB
/// where the page they came from is 970 KB, for exactly the same information.
fn read_cached(system: &str) -> Option<Vec<String>> {
    let path = index_dir().join(format!("{}.idx", slug(system)));
    let text = std::fs::read_to_string(&path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != FORMAT_VERSION {
        return None;
    }
    let written: u64 = lines.next()?.trim().parse().ok()?;
    let fresh = now_secs().checked_sub(written).is_some_and(|age| age < INDEX_TTL_SECS);
    if !fresh {
        return None;
    }
    Some(lines.map(str::to_string).filter(|l| !l.is_empty()).collect())
}

fn write_cached(system: &str, names: &[String]) {
    let path = index_dir().join(format!("{}.idx", slug(system)));
    let mut out = String::with_capacity(names.len() * 32);
    out.push_str(FORMAT_VERSION);
    out.push('\n');
    out.push_str(&now_secs().to_string());
    out.push('\n');
    for n in names {
        out.push_str(n);
        out.push('\n');
    }
    if let Err(e) = std::fs::write(&path, out) {
        log::warn!("could not cache the {system} index: {e}");
    }
}

/// Has fetching this system failed recently?
///
/// Without this, a bulk run over 300 games during an outage asks for the same
/// index 300 times.
fn recently_failed(system: &str) -> bool {
    let path = index_dir().join(format!("{}.stale", slug(system)));
    let Ok(text) = std::fs::read_to_string(&path) else { return false };
    let Ok(when) = text.trim().parse::<u64>() else { return false };
    now_secs().checked_sub(when).is_some_and(|age| age < INDEX_FAIL_TTL_SECS)
}

fn record_failure(system: &str) {
    let path = index_dir().join(format!("{}.stale", slug(system)));
    let _ = std::fs::write(path, now_secs().to_string());
}

type Memo = Mutex<HashMap<String, Arc<Index>>>;

fn memo() -> &'static Memo {
    static M: OnceLock<Memo> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The box-art listing for one Libretro system directory.
///
/// `system` is the archive's own directory name, verbatim — the caller gets it
/// from `tunante_core::console`, which is the one place those strings live.
pub fn index_for(http: &dyn Http, system: &str) -> Result<Arc<Index>, ArtError> {
    if let Some(hit) = memo().lock().unwrap_or_else(|e| e.into_inner()).get(system) {
        return Ok(Arc::clone(hit));
    }
    if let Some(names) = read_cached(system) {
        let idx = Arc::new(Index::new(names));
        memo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(system.to_string(), Arc::clone(&idx));
        return Ok(idx);
    }
    if recently_failed(system) {
        return Err(ArtError::Network(format!("{system}: index fetch failed recently")));
    }

    let names = match fetch_autoindex(http, system) {
        Ok(n) => n,
        Err(primary) => {
            log::warn!("{system}: autoindex unusable ({primary}); trying the GitHub tree");
            match fetch_github(http, system) {
                Ok(n) => n,
                Err(secondary) => {
                    record_failure(system);
                    return Err(ArtError::Network(format!(
                        "{system}: no listing available ({primary}; {secondary})"
                    )));
                }
            }
        }
    };

    log::info!("{system}: {} covers in the archive", names.len());
    write_cached(system, &names);
    let idx = Arc::new(Index::new(names));
    memo()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(system.to_string(), Arc::clone(&idx));
    Ok(idx)
}

fn fetch_autoindex(http: &dyn Http, system: &str) -> Result<Vec<String>, ArtError> {
    let url = format!(
        "{AUTOINDEX_HOST}/{}/Named_Boxarts/",
        urlencoding::encode(system)
    );
    let resp = http.get(&url, MAX_LISTING_BYTES)?;
    if !resp.is_success() {
        return Err(ArtError::Http { status: resp.status, url });
    }
    let body = String::from_utf8_lossy(&resp.body);
    let names = index::parse_autoindex(&body);
    if !index::looks_like_a_listing(&body, names.len()) {
        return Err(ArtError::Network(format!(
            "{url} returned {} bytes but no file names — has the format changed?",
            resp.body.len()
        )));
    }
    Ok(names)
}

/// The same listing from the GitHub API. Two requests: the repository root
/// tree, then the `Named_Boxarts` subtree.
fn fetch_github(http: &dyn Http, system: &str) -> Result<Vec<String>, ArtError> {
    let repo = system.replace(' ', "_");
    let root_url = format!("{GITHUB_API}/{}/git/trees/master", urlencoding::encode(&repo));
    let root = http.get(&root_url, MAX_LISTING_BYTES)?;
    if !root.is_success() {
        return Err(ArtError::Http { status: root.status, url: root_url });
    }
    let v: serde_json::Value = serde_json::from_slice(&root.body)
        .map_err(|e| ArtError::Network(format!("bad tree JSON: {e}")))?;
    let sha = v
        .get("tree")
        .and_then(|t| t.as_array())
        .and_then(|entries| {
            entries
                .iter()
                .find(|e| e.get("path").and_then(|p| p.as_str()) == Some("Named_Boxarts"))
        })
        .and_then(|e| e.get("sha"))
        .and_then(|s| s.as_str())
        .ok_or_else(|| ArtError::Network(format!("{repo} has no Named_Boxarts")))?;

    let sub_url = format!("{GITHUB_API}/{}/git/trees/{sha}", urlencoding::encode(&repo));
    let sub = http.get(&sub_url, MAX_LISTING_BYTES)?;
    if !sub.is_success() {
        return Err(ArtError::Http { status: sub.status, url: sub_url });
    }
    let names = index::parse_github_tree(&String::from_utf8_lossy(&sub.body))?;
    if names.is_empty() {
        return Err(ArtError::Network(format!("{repo}: the tree held no covers")));
    }
    Ok(names)
}

/// The URL of one cover, built from a file name the server itself gave us —
/// never from one we guessed.
pub fn cover_url(system: &str, file: &str) -> String {
    format!(
        "{AUTOINDEX_HOST}/{}/Named_Boxarts/{}.png",
        urlencoding::encode(system),
        urlencoding::encode(file)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::FakeHttp;

    const SYSTEM: &str = "Nintendo - Super Nintendo Entertainment System";

    fn autoindex_url() -> String {
        format!("{AUTOINDEX_HOST}/{}/Named_Boxarts/", urlencoding::encode(SYSTEM))
    }

    #[test]
    fn a_cover_url_is_built_from_the_servers_own_spelling() {
        let url = cover_url(SYSTEM, "Pocky & Rocky (USA)");
        assert!(url.contains("Pocky%20%26%20Rocky%20%28USA%29.png"), "{url}");
        assert!(url.starts_with(AUTOINDEX_HOST));
    }

    #[test]
    fn a_system_name_becomes_a_safe_filename() {
        assert_eq!(slug(SYSTEM), "nintendo___super_nintendo_entertainment_system");
        assert!(!slug("Sega - Mega Drive - Genesis").contains('/'));
    }

    #[test]
    fn the_autoindex_is_parsed_into_names() {
        let page = r#"<a href="../">up</a><a href="Chrono%20Trigger%20%28USA%29.png">x</a>"#;
        let http = FakeHttp::new().with(&autoindex_url(), 200, page);
        assert_eq!(fetch_autoindex(&http, SYSTEM).unwrap(), ["Chrono Trigger (USA)"]);
    }

    /// The gate that catches the archive changing shape under us. It must fire
    /// on "no names at all", not on "fewer names than I expected" — the real
    /// PlayStation 4 archive holds twenty.
    #[test]
    fn a_page_that_stopped_being_a_listing_is_rejected() {
        let junk = "<html><body>".to_string() + &"moved. ".repeat(200) + "</body></html>";
        let http = FakeHttp::new().with(&autoindex_url(), 200, &junk);
        assert!(fetch_autoindex(&http, SYSTEM).is_err());
    }

    #[test]
    fn a_genuinely_tiny_archive_is_accepted() {
        let page: String = (0..20).map(|i| format!("<a href=\"G{i}.png\">x</a>")).collect();
        let http = FakeHttp::new().with(&autoindex_url(), 200, &page);
        assert_eq!(fetch_autoindex(&http, SYSTEM).unwrap().len(), 20);
    }

    #[test]
    fn a_non_success_is_not_a_listing() {
        let http = FakeHttp::new().with(&autoindex_url(), 503, "down");
        assert!(matches!(fetch_autoindex(&http, SYSTEM), Err(ArtError::Http { status: 503, .. })));
    }

    /// The whole point of the second source: the primary is not an API.
    #[test]
    fn the_github_tree_answers_when_the_autoindex_cannot() {
        let repo = SYSTEM.replace(' ', "_");
        let root = format!("{GITHUB_API}/{}/git/trees/master", urlencoding::encode(&repo));
        let sub = format!("{GITHUB_API}/{}/git/trees/abc123", urlencoding::encode(&repo));
        let http = FakeHttp::new()
            .with(&root, 200, r#"{"tree":[{"path":"Named_Boxarts","sha":"abc123"}]}"#)
            .with(&sub, 200, r#"{"tree":[{"path":"Chrono Trigger (USA).png"}]}"#);
        assert_eq!(fetch_github(&http, SYSTEM).unwrap(), ["Chrono Trigger (USA)"]);
    }

    #[test]
    fn a_repo_without_boxarts_is_an_error_not_an_empty_index() {
        let repo = SYSTEM.replace(' ', "_");
        let root = format!("{GITHUB_API}/{}/git/trees/master", urlencoding::encode(&repo));
        let http = FakeHttp::new().with(&root, 200, r#"{"tree":[{"path":"README.md","sha":"z"}]}"#);
        assert!(fetch_github(&http, SYSTEM).is_err());
    }
}
