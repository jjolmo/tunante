//! Watching library folders for changes: the scan that never ends.
//!
//! Moved here from the desktop app (fase 1 of docs/plan-desktop-slint.md) with
//! its couplings turned into parameters. This module owns what is generic —
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
        let mut pending: HashMap<PathBuf, (EventKind, Instant)> = HashMap::new();
        let debounce_duration = Duration::from_secs(2);

        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    for path in event.paths {
                        if is_interesting(&path) {
                            pending.insert(path, (event.kind, Instant::now()));
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
            let ready: Vec<(PathBuf, EventKind)> = pending
                .iter()
                .filter(|(_, (_, timestamp))| now.duration_since(*timestamp) >= debounce_duration)
                .map(|(path, (kind, _))| (path.clone(), *kind))
                .collect();

            for (path, kind) in ready {
                pending.remove(&path);
                let change = match kind {
                    EventKind::Create(_) | EventKind::Modify(_) => FileChange::Modified,
                    EventKind::Remove(_) => FileChange::Removed,
                    _ => continue,
                };
                on_change(change, &path);
            }
        }
    }
}
