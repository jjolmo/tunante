//! Watching library folders for changes: the scan that never ends.
//!
//! Moved here from the retired desktop app with its couplings turned into
//! parameters. This module owns what is generic —
//! choosing a backend per platform, falling back to polling when inotify runs
//! out of watches, and debouncing the storm of events a single file copy
//! produces — and hands each settled change to a callback. What to *do* about
//! a changed file (re-read its metadata, touch the database, tell the UI) is
//! the caller's business: the desktop re-reads in process today, a
//! helper-based app will probe out of process, and this module cares about
//! neither.
//!
//! The filter is a parameter for the same reason: the static extension list
//! lives in `tunante-core`, but the desktop also consults vgmstream's dynamic
//! list, which lives behind `tunante-codec` — a crate this one must never
//! link, or every vendored core would ride along into every app.

use notify::{Config, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// What happened to a file, after debouncing.
///
/// Two cases rather than notify's many: a create and a modify both mean "read
/// this file again", and everything else the caller could want collapses into
/// "forget it".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChange {
    /// Created or modified — the file's metadata should be (re-)read.
    Modified,
    /// Removed — whatever the library knew about it is stale.
    Removed,
}

pub struct FolderWatcher {
    watcher: Option<Box<dyn Watcher + Send>>,
    watched_paths: HashMap<String, bool>,
    tx: mpsc::Sender<notify::Result<Event>>,
    /// True if using PollWatcher fallback instead of native watcher
    is_polling: bool,
}

impl FolderWatcher {
    /// Build the watcher and start its processing thread.
    ///
    /// `is_interesting` decides which paths are worth reporting at all (in
    /// practice: "is this an audio file"); `on_change` receives each settled
    /// change, on the processing thread, no sooner than the debounce window
    /// after the last event for that path.
    pub fn new(
        is_interesting: impl Fn(&Path) -> bool + Send + 'static,
        mut on_change: impl FnMut(FileChange, &Path) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

        std::thread::spawn(move || {
            Self::process_events(rx, is_interesting, &mut on_change);
        });

        Self {
            watcher: None,
            watched_paths: HashMap::new(),
            tx,
            is_polling: false,
        }
    }

    pub fn start_watching(&mut self, path: &str) -> Result<(), String> {
        if self.watcher.is_none() {
            self.create_watcher()?;
        }

        if let Some(ref mut watcher) = self.watcher {
            match watcher.watch(Path::new(path), RecursiveMode::Recursive) {
                Ok(()) => {
                    self.watched_paths.insert(path.to_string(), true);
                }
                Err(e) => {
                    // On Linux, if inotify fails (too many watches), fall back to PollWatcher
                    #[cfg(target_os = "linux")]
                    if !self.is_polling {
                        log::warn!(
                            "Native watcher failed for {}: {} — falling back to PollWatcher",
                            path,
                            e
                        );
                        return self.fallback_to_poll(path);
                    }
                    return Err(e.to_string());
                }
            }
        }
        Ok(())
    }

    /// Create the appropriate watcher for this platform.
    /// macOS: always PollWatcher (kqueue FD limits).
    /// Linux/Windows: native watcher (inotify/ReadDirectoryChanges).
    fn create_watcher(&mut self) -> Result<(), String> {
        let tx = self.tx.clone();

        #[cfg(target_os = "macos")]
        {
            let config = Config::default().with_poll_interval(Duration::from_secs(120));
            let watcher = PollWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                config,
            )
            .map_err(|e| e.to_string())?;
            self.watcher = Some(Box::new(watcher));
            self.is_polling = true;
            log::info!("File watcher: PollWatcher (macOS, 120s interval)");
        }

        #[cfg(not(target_os = "macos"))]
        {
            let watcher = RecommendedWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                Config::default(),
            )
            .map_err(|e| e.to_string())?;
            self.watcher = Some(Box::new(watcher));
            self.is_polling = false;
            log::info!("File watcher: native (inotify/ReadDirectoryChanges)");
        }

        Ok(())
    }

    /// Fall back from native watcher to PollWatcher (Linux only).
    /// Re-watches all previously watched paths with the poll watcher.
    #[cfg(target_os = "linux")]
    fn fallback_to_poll(&mut self, new_path: &str) -> Result<(), String> {
        let tx = self.tx.clone();
        let config = Config::default().with_poll_interval(Duration::from_secs(120));
        let mut poll_watcher = PollWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            config,
        )
        .map_err(|e| format!("PollWatcher fallback failed: {}", e))?;

        // Re-watch all existing paths
        for existing_path in self.watched_paths.keys() {
            if let Err(e) =
                poll_watcher.watch(Path::new(existing_path), RecursiveMode::Recursive)
            {
                log::warn!("PollWatcher failed to re-watch {}: {}", existing_path, e);
            }
        }

        // Watch the new path that triggered the fallback
        poll_watcher
            .watch(Path::new(new_path), RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;
        self.watched_paths.insert(new_path.to_string(), true);

        self.watcher = Some(Box::new(poll_watcher));
        self.is_polling = true;
        log::info!("File watcher: fell back to PollWatcher (120s interval)");
        Ok(())
    }

    /// What is being watched right now — so a caller can reconcile this
    /// against the database's list instead of remembering its own.
    pub fn watched(&self) -> Vec<String> {
        self.watched_paths.keys().cloned().collect()
    }

    pub fn stop_watching(&mut self, path: &str) -> Result<(), String> {
        if let Some(ref mut watcher) = self.watcher {
            let _ = watcher.unwatch(Path::new(path));
            self.watched_paths.remove(path);
        }
        Ok(())
    }

    fn process_events(
        rx: mpsc::Receiver<notify::Result<Event>>,
        is_interesting: impl Fn(&Path) -> bool,
        on_change: &mut impl FnMut(FileChange, &Path),
    ) {
        let mut pending: HashMap<PathBuf, (FileChange, Instant)> = HashMap::new();
        let debounce_duration = Duration::from_secs(2);

        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    // Classified on arrival, not at flush time. The last event
                    // of every file copy is Access(Close(Write)); letting it
                    // overwrite the pending kind and fall through a `_` arm is
                    // how the desktop's watcher silently swallowed every new
                    // file — the bug this module's test caught on day one.
                    let change = match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            Some(FileChange::Modified)
                        }
                        EventKind::Remove(_) => Some(FileChange::Removed),
                        // No meaning of its own, but it is activity: refresh
                        // the debounce clock of whatever is already pending —
                        // Close(Write) is precisely "the copy just finished".
                        _ => None,
                    };
                    for path in event.paths {
                        if !is_interesting(&path) {
                            continue;
                        }
                        match change {
                            Some(c) => {
                                pending.insert(path, (c, Instant::now()));
                            }
                            None => {
                                if let Some(entry) = pending.get_mut(&path) {
                                    entry.1 = Instant::now();
                                }
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    log::error!("Watcher error: {}", e);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }

            let now = Instant::now();
            let ready: Vec<(PathBuf, FileChange)> = pending
                .iter()
                .filter(|(_, (_, timestamp))| now.duration_since(*timestamp) >= debounce_duration)
                .map(|(path, (change, _))| (path.clone(), *change))
                .collect();

            for (path, change) in ready {
                pending.remove(&path);
                on_change(change, &path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// The whole promise in one test: a file created under a watched folder
    /// reaches the handler as Modified, after the debounce, on the processing
    /// thread. Real filesystem, real inotify, real clock — the debounce is
    /// the feature, so the test has to wait it out.
    #[test]
    fn a_created_file_reaches_the_handler_debounced() {
        let dir = std::env::temp_dir().join(format!("tunante-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let hits: Arc<Mutex<Vec<(FileChange, std::path::PathBuf)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let sink = hits.clone();
        let mut w = FolderWatcher::new(
            |p| p.extension().is_some_and(|e| e == "mp3"),
            move |change, path| {
                sink.lock().unwrap().push((change, path.to_path_buf()));
            },
        );
        w.start_watching(&dir.to_string_lossy()).unwrap();
        // Let the backend establish its watch before producing events.
        std::thread::sleep(Duration::from_millis(300));

        std::fs::write(dir.join("song.mp3"), b"not really audio").unwrap();
        // Filtered out: never worth a probe, must never reach the handler.
        std::fs::write(dir.join("cover.jpg"), b"not audio either").unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            {
                let got = hits.lock().unwrap();
                if !got.is_empty() {
                    assert!(
                        got.iter().all(|(c, p)| {
                            *c == FileChange::Modified
                                && p.file_name().is_some_and(|n| n == "song.mp3")
                        }),
                        "unexpected events: {got:?}"
                    );
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "the created file never reached the handler"
            );
            std::thread::sleep(Duration::from_millis(250));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
