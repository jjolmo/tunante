//! Scanning folders into the library database, and reading the tree back out.
//!
//! The scan reads metadata by spawning `tunante-decoder probe`, one process per
//! file. That is slower per file than an in-process read, and it buys two things
//! worth the cost: the console RAM an emulator-backed reader allocates never
//! lands in this process, and a reader that hangs can be killed. The desktop app
//! wraps its in-process reads in a timeout that cannot actually interrupt a loop
//! running in C — it can only abandon the thread and leak it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tunante_core::db::models::Track;
use tunante_core::db::Database;
use tunante_core::vgm_path;
use walkdir::WalkDir;

use crate::decoder;

/// How long a single file gets before the scanner gives up on it.
///
/// Generous, because a PSF2 or USF set legitimately takes seconds to open. The
/// point is not speed, it is that a file which never returns cannot stall the
/// whole scan.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

pub struct ScanProgress {
    pub scanned: usize,
    pub total: usize,
    pub added: usize,
    pub failed: usize,
    pub current: String,
}

/// Walk `root`, probe everything that looks like audio, and insert what comes back.
///
/// `on_progress` is called as files are processed, so a UI can show movement on
/// what is, for a real collection, a minutes-long job.
///
/// # Why this is parallel
///
/// Each file costs a process spawn plus however long its format takes to open —
/// which for the emulator formats is the dominant term, and it is spent waiting
/// on one core. Running several at once is the natural shape for a design that
/// already puts every decode in its own process, and on this phone's eight cores
/// it is the difference between a scan of minutes and one of half an hour.
///
/// SQLite is left on this thread: the writes are trivial next to the probes, and
/// keeping one writer avoids `SQLITE_BUSY` entirely.
pub fn scan_folder(
    db: &Database,
    root: &Path,
    mut on_progress: impl FnMut(&ScanProgress),
) -> Result<usize, String> {
    let files: Vec<PathBuf> = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| vgm_path::is_audio_file(p))
        .collect();

    let mut progress = ScanProgress {
        scanned: 0,
        total: files.len(),
        added: 0,
        failed: 0,
        current: String::new(),
    };

    // Leave a core or two for the session — this runs on a phone the user is
    // holding, and a scan that makes the interface stutter is worse than a slow
    // one.
    let workers = std::thread::available_parallelism()
        .map(|n| (n.get().saturating_sub(2)).max(2))
        .unwrap_or(2);

    let queue = std::sync::Arc::new(std::sync::Mutex::new(files.into_iter()));
    let (tx, rx) = std::sync::mpsc::channel::<(String, Result<Vec<serde_json::Value>, String>)>();

    let mut handles = Vec::new();
    for _ in 0..workers {
        let queue = queue.clone();
        let tx = tx.clone();
        handles.push(std::thread::spawn(move || loop {
            let next = { queue.lock().ok().and_then(|mut q| q.next()) };
            let Some(path) = next else { return };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let result = decoder::probe(&path, PROBE_TIMEOUT, true);
            if tx.send((name, result)).is_err() {
                return;
            }
        }));
    }
    drop(tx);

    for (name, result) in rx {
        progress.scanned += 1;
        progress.current = name;

        match result {
            Ok(tracks) => {
                // One file can hold many tracks: a GME set or a vgmstream
                // container has one per subsong, each addressed as `path#n`.
                for value in tracks {
                    match serde_json::from_value::<Track>(value) {
                        Ok(track) => {
                            if db.insert_track(&track).is_ok() {
                                progress.added += 1;
                            }
                        }
                        Err(_) => progress.failed += 1,
                    }
                }
            }
            Err(_) => progress.failed += 1,
        }

        on_progress(&progress);
    }

    for h in handles {
        let _ = h.join();
    }

    Ok(progress.added)
}

/// One line of the library tab: a folder or a track, with how deep it sits.
#[derive(Clone, Debug)]
pub struct Row {
    pub label: String,
    pub detail: String,
    pub depth: usize,
    pub is_folder: bool,
    pub expanded: bool,
    /// For a folder, its path. For a track, its path including any `#subsong`.
    pub path: String,
}

/// The library as a flat list of visible rows.
///
/// A flat list is not a compromise forced by Slint's lack of a tree widget — it
/// is what keeps the memory flat. Only expanded folders contribute rows, so a
/// collection of any size costs whatever the user has actually opened.
pub struct Tree {
    roots: Vec<PathBuf>,
    expanded: std::collections::HashSet<String>,
}

impl Tree {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots, expanded: Default::default() }
    }

    pub fn toggle(&mut self, path: &str) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_string());
        }
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Build the visible rows, asking the database only about open folders.
    pub fn rows(&self, db: &Database) -> Vec<Row> {
        let mut out = Vec::new();
        for root in &self.roots {
            self.push_folder(db, root, 0, &mut out);
        }
        out
    }

    fn push_folder(&self, db: &Database, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        let key = dir.to_string_lossy().to_string();
        let expanded = self.is_expanded(&key);

        let label = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| key.clone());

        let tracks = db.get_tracks_by_folder(&key).unwrap_or_default();
        let subdirs = child_dirs(dir);

        out.push(Row {
            label,
            detail: if tracks.is_empty() {
                String::new()
            } else {
                format!("{} pistas", tracks.len())
            },
            depth,
            is_folder: true,
            expanded,
            path: key,
        });

        if !expanded {
            return;
        }

        for sub in subdirs {
            self.push_folder(db, &sub, depth + 1, out);
        }

        for t in tracks {
            out.push(Row {
                label: if t.title.is_empty() { file_label(&t.path) } else { t.title.clone() },
                detail: format_duration(t.duration_ms),
                depth: depth + 1,
                is_folder: false,
                expanded: false,
                path: t.path,
            });
        }
    }
}

/// Directories directly under `dir`, sorted, ignoring anything unreadable.
fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: BTreeMap<String, PathBuf> = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                v.insert(e.file_name().to_string_lossy().to_string(), e.path());
            }
        }
    }
    v.into_values().collect()
}

fn file_label(path: &str) -> String {
    let (real, sub) = vgm_path::parse_vgm_path(path);
    let name = Path::new(real)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| real.to_string());
    match sub {
        Some(n) => format!("{name} #{n}"),
        None => name,
    }
}

pub fn format_duration(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}
