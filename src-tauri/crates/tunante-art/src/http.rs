//! Talking to the four places a cover might come from, politely.
//!
//! # Why there is a trait here
//!
//! Not for abstraction's sake — for tests. The index parser and the whole
//! matcher are the interesting parts of this crate, and both become testable
//! against a checked-in fixture with no network at all if the fetch is one
//! swappable call. It is also the escape hatch if the desktop app ever has to
//! go back to `reqwest` for its own reasons.
//!
//! # The rules a public archive expects
//!
//! The old code had **no timeout at all**, a `Tunante/1.0` user agent, and no
//! spacing between requests. Wikimedia's own policy asks for a descriptive agent
//! with a contact URL and rejects generic ones, and a bulk run over 300 games
//! hammering four hosts unthrottled is how a project gets blocked rather than
//! rate-limited. So: one gate per host, a real agent string, and a breaker that
//! gives up on a host that has started refusing instead of grinding through 300
//! more failures.

use crate::ArtError;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Descriptive, with somewhere to complain to. Wikimedia asks for exactly this
/// and 403s the `Tunante/1.0` shape the old code sent.
pub const USER_AGENT: &str = concat!(
    "Tunante/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/jjolmo/tunante)"
);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: u32 = 3;
/// Consecutive refusals from one host before it is dropped for this run.
const BREAKER_TRIP: u32 = 5;

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub trait Http: Send + Sync {
    /// GET, with a hard ceiling on how much body will be read.
    ///
    /// The ceiling is enforced *while reading*, not after: a mislinked or
    /// hostile URL must not be able to fill a phone's disk before anyone checks
    /// how big it was.
    fn get(&self, url: &str, max_bytes: usize) -> Result<HttpResponse, ArtError>;
}

/// The minimum gap between two requests to the same host.
fn min_gap(host: &str) -> Duration {
    if host.ends_with("wikipedia.org")
        || host.ends_with("wikidata.org")
        || host.ends_with("wikimedia.org")
    {
        // Wikimedia's stated limit for an unauthenticated client.
        Duration::from_millis(1000)
    } else if host.ends_with("apple.com") {
        // iTunes tolerates roughly 20/minute before it starts refusing.
        Duration::from_millis(3000)
    } else if host.ends_with("github.com") {
        Duration::from_millis(1000)
    } else {
        // A CDN of static files. Four in flight is nothing to it.
        Duration::from_millis(100)
    }
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

pub struct UreqHttp {
    agent: ureq::Agent,
    /// When each host may next be spoken to.
    gates: Mutex<HashMap<String, Instant>>,
    /// Consecutive refusals per host, and whether it has been given up on.
    breaker: Mutex<HashMap<String, u32>>,
    /// Cheap deterministic jitter, so retries from several workers do not line
    /// up. A whole `rand` dependency for this would be silly.
    tick: AtomicU64,
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqHttp {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(TOTAL_TIMEOUT))
            // A non-2xx is an answer, not an exception: 404 is the normal
            // outcome of asking an archive for a game it does not have.
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self {
            agent,
            gates: Mutex::new(HashMap::new()),
            breaker: Mutex::new(HashMap::new()),
            tick: AtomicU64::new(0),
        }
    }

    /// Block until this host may be spoken to again.
    fn wait_turn(&self, host: &str) {
        let gap = min_gap(host);
        let sleep_for = {
            let mut gates = self.gates.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let next = gates.entry(host.to_string()).or_insert(now);
            let wait = next.saturating_duration_since(now);
            *next = (*next).max(now) + gap;
            wait
        };
        if !sleep_for.is_zero() {
            std::thread::sleep(sleep_for);
        }
    }

    fn is_tripped(&self, host: &str) -> bool {
        self.breaker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(host)
            .is_some_and(|&n| n >= BREAKER_TRIP)
    }

    fn note(&self, host: &str, refused: bool) {
        let mut b = self.breaker.lock().unwrap_or_else(|e| e.into_inner());
        let n = b.entry(host.to_string()).or_insert(0);
        if refused {
            *n += 1;
            if *n == BREAKER_TRIP {
                log::warn!("{host} has refused {BREAKER_TRIP} times in a row — dropping it for this run");
            }
        } else {
            *n = 0;
        }
    }

    fn jitter(&self) -> Duration {
        Duration::from_millis(self.tick.fetch_add(137, Ordering::Relaxed) % 400)
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str, max_bytes: usize) -> Result<HttpResponse, ArtError> {
        let host = host_of(url);
        if self.is_tripped(&host) {
            return Err(ArtError::Network(format!("{host} is refusing; skipped")));
        }

        let mut last: Option<ArtError> = None;
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(500 * (1 << attempt)) + self.jitter());
            }
            self.wait_turn(&host);

            match self.agent.get(url).call() {
                Ok(mut resp) => {
                    let status = resp.status().as_u16();
                    // 429 and 403 are the host telling us to stop, and are the
                    // only statuses worth retrying or counting against it. A 404
                    // is a perfectly good answer.
                    let refused = status == 429 || status == 403;
                    self.note(&host, refused);
                    if refused && attempt + 1 < MAX_ATTEMPTS {
                        if let Some(after) = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.trim().parse::<u64>().ok())
                        {
                            std::thread::sleep(Duration::from_secs(after.min(60)));
                        }
                        last = Some(ArtError::Http { status, url: url.to_string() });
                        continue;
                    }
                    let content_type = resp
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    let body = resp
                        .body_mut()
                        .with_config()
                        .limit(max_bytes as u64)
                        .read_to_vec()
                        .map_err(|e| ArtError::Network(format!("reading {url}: {e}")))?;
                    return Ok(HttpResponse { status, content_type, body });
                }
                Err(e) => {
                    // A transport failure says nothing about the host's opinion
                    // of us, so it must not trip the breaker.
                    last = Some(ArtError::Network(format!("{url}: {e}")));
                }
            }
        }
        Err(last.unwrap_or_else(|| ArtError::Network(format!("{url}: no attempt succeeded"))))
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;

    /// An `Http` that answers from a table. The whole point of the trait.
    pub struct FakeHttp {
        pub responses: HashMap<String, HttpResponse>,
        pub requested: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        pub fn new() -> Self {
            Self { responses: HashMap::new(), requested: Mutex::new(Vec::new()) }
        }
        pub fn with(mut self, url: &str, status: u16, body: &str) -> Self {
            self.responses.insert(
                url.to_string(),
                HttpResponse {
                    status,
                    content_type: "text/html".into(),
                    body: body.as_bytes().to_vec(),
                },
            );
            self
        }
    }

    impl Http for FakeHttp {
        fn get(&self, url: &str, _max: usize) -> Result<HttpResponse, ArtError> {
            self.requested.lock().unwrap().push(url.to_string());
            self.responses.get(url).cloned().ok_or(ArtError::Http {
                status: 404,
                url: url.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_come_out_of_urls() {
        assert_eq!(host_of("https://thumbnails.libretro.com/a/b.png"), "thumbnails.libretro.com");
        assert_eq!(host_of("https://www.wikidata.org/w/api.php?x=1"), "www.wikidata.org");
        assert_eq!(host_of("https://example.com:8080/x"), "example.com");
        assert_eq!(host_of("not a url"), "");
    }

    /// Wikimedia gets a second between requests; a CDN of static files does not
    /// need one.
    #[test]
    fn the_hosts_that_ask_to_be_slowed_down_are() {
        assert_eq!(min_gap("commons.wikimedia.org"), Duration::from_millis(1000));
        assert_eq!(min_gap("www.wikidata.org"), Duration::from_millis(1000));
        assert_eq!(min_gap("en.wikipedia.org"), Duration::from_millis(1000));
        assert_eq!(min_gap("itunes.apple.com"), Duration::from_millis(3000));
        assert_eq!(min_gap("thumbnails.libretro.com"), Duration::from_millis(100));
    }

    /// The agent string Wikimedia's policy asks for: a name, a version and a
    /// way to reach whoever is responsible.
    #[test]
    fn the_user_agent_says_who_we_are() {
        assert!(USER_AGENT.starts_with("Tunante/"));
        assert!(USER_AGENT.contains("https://github.com/"));
        assert_ne!(USER_AGENT, "Tunante/1.0", "the shape Wikimedia rejects");
    }

    #[test]
    fn a_gate_actually_delays_the_second_call() {
        let h = UreqHttp::new();
        let start = Instant::now();
        h.wait_turn("thumbnails.libretro.com");
        h.wait_turn("thumbnails.libretro.com");
        assert!(start.elapsed() >= Duration::from_millis(90), "no gap was enforced");
    }

    /// A different host must not wait behind an unrelated one.
    #[test]
    fn gates_are_per_host() {
        let h = UreqHttp::new();
        h.wait_turn("itunes.apple.com");
        let start = Instant::now();
        h.wait_turn("thumbnails.libretro.com");
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    /// A run that has been refused five times over should stop asking, not
    /// grind through three hundred more.
    #[test]
    fn a_host_that_keeps_refusing_is_dropped() {
        let h = UreqHttp::new();
        for _ in 0..BREAKER_TRIP {
            assert!(!h.is_tripped("x.example"));
            h.note("x.example", true);
        }
        assert!(h.is_tripped("x.example"));
        // ...and one success clears it, so a blip does not poison the run.
        h.note("x.example", false);
        assert!(!h.is_tripped("x.example"));
    }

    /// A 404 is the normal answer for a game an archive does not carry, and must
    /// not count against the host.
    #[test]
    fn a_missing_cover_is_not_a_refusal() {
        let h = UreqHttp::new();
        for _ in 0..10 {
            h.note("y.example", false);
        }
        assert!(!h.is_tripped("y.example"));
    }
}
