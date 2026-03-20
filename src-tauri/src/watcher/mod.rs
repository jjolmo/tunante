use crate::commands::library::is_audio_file;
use crate::metadata;
use crate::AppState;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub struct FolderWatcher {
    watcher: Option<RecommendedWatcher>,
    watched_paths: HashMap<String, bool>,
    tx: mpsc::Sender<notify::Result<Event>>,
}

#[derive(Clone, serde::Serialize)]
pub struct WatcherEvent {
    pub event_type: String,
    pub path: String,
}

impl FolderWatcher {
    pub fn new(state: Arc<AppState>, app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

        let state_clone = state.clone();
        let app_clone = app.clone();
        std::thread::spawn(move || {
            Self::process_events(rx, state_clone, app_clone);
        });

        Self {
            watcher: None,
            watched_paths: HashMap::new(),
            tx,
        }
    }

    pub fn start_watching(&mut self, path: &str) -> Result<(), String> {
        if self.watcher.is_none() {
            let tx = self.tx.clone();
            let watcher = RecommendedWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                Config::default(),
            )
            .map_err(|e| e.to_string())?;
            self.watcher = Some(watcher);
        }

        if let Some(ref mut watcher) = self.watcher {
            watcher
                .watch(std::path::Path::new(path), RecursiveMode::Recursive)
                .map_err(|e| e.to_string())?;
            self.watched_paths.insert(path.to_string(), true);
        }
        Ok(())
    }

    pub fn stop_watching(&mut self, path: &str) -> Result<(), String> {
        if let Some(ref mut watcher) = self.watcher {
            let _ = watcher.unwatch(std::path::Path::new(path));
            self.watched_paths.remove(path);
        }
        Ok(())
    }

    fn process_events(
        rx: mpsc::Receiver<notify::Result<Event>>,
        state: Arc<AppState>,
        app: AppHandle,
    ) {
        let mut pending: HashMap<PathBuf, (EventKind, Instant)> = HashMap::new();
        let debounce_duration = Duration::from_secs(2);

        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    for path in event.paths {
                        if is_audio_file(&path) {
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
                Self::handle_file_event(&path, kind, &state, &app);
            }
        }
    }

    fn handle_file_event(
        path: &PathBuf,
        kind: EventKind,
        state: &Arc<AppState>,
        app: &AppHandle,
    ) {
        let path_str = path.to_string_lossy().to_string();

        match kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                match metadata::read_metadata(path) {
                    Ok(track) => {
                        let db = state.db.lock();
                        if let Ok(Some(existing)) = db.get_track_by_path(&path_str) {
                            let mut updated = track;
                            updated.id = existing.id;
                            if let Err(e) = db.insert_track(&updated) {
                                log::error!("Failed to update track {}: {}", path_str, e);
                            }
                        } else if let Err(e) = db.insert_track(&track) {
                            log::error!("Failed to insert track {}: {}", path_str, e);
                        }
                        drop(db);

                        let _ = app.emit(
                            "library-updated",
                            WatcherEvent {
                                event_type: "modified".to_string(),
                                path: path_str,
                            },
                        );
                    }
                    Err(e) => {
                        log::warn!("Failed to read metadata for {}: {}", path_str, e);
                    }
                }
            }
            EventKind::Remove(_) => {
                let db = state.db.lock();
                if let Err(e) = db.remove_track_by_path(&path_str) {
                    log::error!("Failed to remove track {}: {}", path_str, e);
                }
                drop(db);

                let _ = app.emit(
                    "library-updated",
                    WatcherEvent {
                        event_type: "deleted".to_string(),
                        path: path_str,
                    },
                );
            }
            _ => {}
        }
    }
}
