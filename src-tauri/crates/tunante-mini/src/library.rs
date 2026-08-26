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


/// Which shape the library takes.
///
/// The tree mirrors the disk, which is honest but makes you walk down to a game
/// you already know the name of. The other two are indexes over the same rows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Tree,
    /// One row per folder that directly holds music. An album, in practice:
    /// this collection puts one game per directory.
    Albums,
    /// One row per console, from the format of the files. Open it and the games
    /// for that console are inside.
    Consoles,
}

impl Mode {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Mode::Albums,
            2 => Mode::Consoles,
            _ => Mode::Tree,
        }
    }
}

/// "1 pista" y no "1 pistas".
fn pistas(n: usize) -> String {
    if n == 1 { "1 pista".to_string() } else { format!("{n} pistas") }
}

fn juegos(n: usize) -> String {
    if n == 1 { "1 juego".to_string() } else { format!("{n} juegos") }
}

/// The console a file belongs to, from its extension.
///
/// The extension is the whole story for these formats: a `.spc` is a ripped
/// SNES sound driver and cannot be anything else. `.vgm` is the exception —
/// it is a chip log and the header says which chip — so it gets its own row
/// rather than a guess.
///
/// Everything with no console is grouped rather than dropped: the point of this
/// view is to reach music, and hiding a third of the library because it is an
/// mp3 would defeat it.
pub fn console_of(path: &str) -> &'static str {
    let real = vgm_path::parse_vgm_path(path).0;
    let ext = Path::new(real)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "nsf" | "nsfe" => "NES",
        "spc" => "Super Nintendo",
        "gbs" => "Game Boy",
        "gsf" | "minigsf" | "gsflib" => "Game Boy Advance",
        "2sf" | "mini2sf" | "2sflib" => "Nintendo DS",
        "usf" | "miniusf" => "Nintendo 64",
        "psf" | "minipsf" | "psflib" => "PlayStation",
        "psf2" | "minipsf2" | "psf2lib" => "PlayStation 2",
        "vgm" | "vgz" => "VGM (Mega Drive y compañía)",
        "sid" => "Commodore 64",
        "ay" => "ZX Spectrum",
        "hes" => "PC Engine",
        "kss" => "MSX",
        "xa" => "PlayStation (streams)",
        "adx" | "ast" | "dsp" | "brstm" | "bcstm" | "strm" | "bfstm" | "hps" => {
            "Rips de GameCube, Wii y 3DS"
        }
        _ => "Otros",
    }
}

/// The library as a flat list of visible rows.
///
/// A flat list is not a compromise forced by Slint's lack of a tree widget — it
/// is what keeps the memory flat. Only expanded folders contribute rows, so a
/// collection of any size costs whatever the user has actually opened.
pub struct Tree {
    roots: Vec<PathBuf>,
    expanded: std::collections::HashSet<String>,
    /// What each open folder holds, remembered between rebuilds.
    ///
    /// Every tap rebuilds the whole visible list, and without this each rebuild
    /// re-queried SQLite and re-read the directory for every folder already
    /// open — so opening the tenth folder cost ten queries, not one. The cache
    /// is only ever as large as what the user has actually opened.
    cache: std::cell::RefCell<std::collections::HashMap<String, FolderContents>>,
}

#[derive(Clone)]
struct FolderContents {
    /// Only what sits directly in this folder. Not its descendants.
    tracks: Vec<Track>,
    /// Everything underneath, subfolders included. For the count on the folder
    /// row, which is more useful as "how much is in here" than as "how many
    /// files did I put loose in this one directory".
    total: usize,
    subdirs: Vec<PathBuf>,
}

impl Tree {
    /// The roots start open.
    ///
    /// Collapsed, a freshly scanned library is a single row reading
    /// "Musica — 1384 pistas" in an otherwise empty screen, which reads as
    /// broken: there is nothing to scroll and no hint that tapping does
    /// anything. Opening the roots costs one query and shows the collection.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        let expanded = roots
            .iter()
            .map(|r| r.to_string_lossy().to_string())
            .collect();
        Self { roots, expanded, cache: Default::default() }
    }

    pub fn toggle(&mut self, path: &str) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_string());
        }
    }

    /// What a folder holds, from the cache when we have already looked.
    fn contents(&self, db: &Database, key: &str, dir: &Path) -> FolderContents {
        if let Some(hit) = self.cache.borrow().get(key) {
            return hit.clone();
        }
        // get_tracks_by_folder matches `path LIKE 'folder/%'`, so it returns
        // every descendant. Listing those under the folder row as well as its
        // subfolders is what made the root show 1839 file rows and repeat every
        // one of them inside the subfolder it actually lives in.
        let all = db.get_tracks_by_folder(key).unwrap_or_default();
        let total = all.len();
        let prefix = format!("{}/", key.trim_end_matches('/'));
        let tracks = all
            .into_iter()
            .filter(|t| {
                // Compare on the real file: a subsong's path carries a `#n`
                // suffix, and the directory is the same either way.
                let real = vgm_path::parse_vgm_path(&t.path).0;
                real.strip_prefix(prefix.as_str())
                    .is_some_and(|rest| !rest.contains('/'))
            })
            .collect();

        let value = FolderContents {
            tracks,
            total,
            subdirs: child_dirs(dir),
        };
        self.cache.borrow_mut().insert(key.to_string(), value.clone());
        value
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Build the visible rows, asking the database only about open folders.
    pub fn rows(&self, db: &Database) -> Vec<Row> {
        self.rows_for(db, Mode::Tree)
    }

    pub fn rows_for(&self, db: &Database, mode: Mode) -> Vec<Row> {
        match mode {
            Mode::Tree => {
                let mut out = Vec::new();
                for root in &self.roots {
                    self.push_folder(db, root, 0, &mut out);
                }
                out
            }
            Mode::Albums => self.rows_albums(db),
            Mode::Consoles => self.rows_consoles(db),
        }
    }

    /// Every folder that directly holds music, flat and in one query.
    ///
    /// One query rather than walking the tree: the alternative reads every
    /// directory under the roots looking for one with audio in it, and on a
    /// phone that is a lot of `readdir` for an answer the database already has.
    fn albums(&self, db: &Database) -> Vec<(String, usize)> {
        let mut count: BTreeMap<String, usize> = BTreeMap::new();
        for root in &self.roots {
            for t in db
                .get_tracks_by_folder(&root.to_string_lossy())
                .unwrap_or_default()
            {
                let real = vgm_path::parse_vgm_path(&t.path).0;
                if let Some(dir) = Path::new(real).parent() {
                    *count.entry(dir.to_string_lossy().to_string()).or_default() += 1;
                }
            }
        }
        count.into_iter().collect()
    }

    fn rows_albums(&self, db: &Database) -> Vec<Row> {
        let mut out = Vec::new();
        for (dir, n) in self.albums(db) {
            let abierto = self.is_expanded(&dir);
            out.push(Row {
                label: Path::new(&dir)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| dir.clone()),
                detail: pistas(n),
                depth: 0,
                is_folder: true,
                expanded: abierto,
                path: dir.clone(),
            });
            if abierto {
                let tracks = self.contents(db, &dir, Path::new(&dir)).tracks;
                self.push_tracks(tracks, 1, &mut out);
            }
        }
        out
    }

    fn rows_consoles(&self, db: &Database) -> Vec<Row> {
        // console -> album folder -> how many tracks of it
        let mut by_console: BTreeMap<&'static str, BTreeMap<String, usize>> = BTreeMap::new();
        for root in &self.roots {
            for t in db
                .get_tracks_by_folder(&root.to_string_lossy())
                .unwrap_or_default()
            {
                let real = vgm_path::parse_vgm_path(&t.path).0;
                let Some(dir) = Path::new(real).parent() else { continue };
                *by_console
                    .entry(console_of(&t.path))
                    .or_default()
                    .entry(dir.to_string_lossy().to_string())
                    .or_default() += 1;
            }
        }

        let mut out = Vec::new();
        for (console, albums) in by_console {
            // A key that cannot collide with a path, so the same `expanded` set
            // serves all three views without them stepping on each other.
            let key = format!("consola:{console}");
            let abierto = self.is_expanded(&key);
            let total: usize = albums.values().sum();

            out.push(Row {
                label: console.to_string(),
                detail: format!("{} · {}", juegos(albums.len()), pistas(total)),
                depth: 0,
                is_folder: true,
                expanded: abierto,
                path: key,
            });
            if !abierto {
                continue;
            }

            for (dir, n) in albums {
                // The same folder can appear under two consoles — a directory
                // with both .spc rips and mp3s — so the expansion key carries
                // the console as well.
                let sub = format!("{console}\u{1}{dir}");
                let sub_abierto = self.is_expanded(&sub);
                out.push(Row {
                    label: Path::new(&dir)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| dir.clone()),
                    detail: pistas(n),
                    depth: 1,
                    is_folder: true,
                    expanded: sub_abierto,
                    path: sub,
                });
                if sub_abierto {
                    let tracks: Vec<Track> = self
                        .contents(db, &dir, Path::new(&dir))
                        .tracks
                        .into_iter()
                        .filter(|t| console_of(&t.path) == console)
                        .collect();
                    self.push_tracks(tracks, 2, &mut out);
                }
            }
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

        let FolderContents { tracks, total, subdirs } = self.contents(db, &key, dir);

        out.push(Row {
            label,
            detail: if total == 0 { String::new() } else { pistas(total) },
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

        self.push_tracks(tracks, depth + 1, out);
    }

    /// Emit a folder's tracks, grouping a file's subsongs under one row.
    ///
    /// Shared by all three views: a disc is a disc whether you reached it
    /// through the tree, through the album list or through its console.
    fn push_tracks(&self, tracks: Vec<Track>, depth: usize, out: &mut Vec<Row>) {
        // Agrupar por fichero real. Un .nsf o un .gsflib traen decenas de temas
        // dentro, todos con la misma ruta y distinto `#n`: listarlos sueltos
        // inunda la carpeta y esconde lo demás que hay en ella.
        let mut por_fichero: BTreeMap<String, Vec<Track>> = BTreeMap::new();
        for t in tracks {
            let real = vgm_path::parse_vgm_path(&t.path).0.to_string();
            por_fichero.entry(real).or_default().push(t);
        }

        for (fichero, mut subs) in por_fichero {
            if subs.len() == 1 {
                let t = subs.remove(0);
                out.push(Row {
                    label: if t.title.is_empty() { file_label(&t.path) } else { t.title.clone() },
                    detail: format_duration(t.duration_ms),
                    depth,
                    is_folder: false,
                    expanded: false,
                    path: t.path,
                });
                continue;
            }

            // Cabecera del conjunto: se despliega como una carpeta, aunque sea
            // un fichero. Para quien escucha, un .nsf *es* un disco.
            let abierto = self.is_expanded(&fichero);
            let nombre = Path::new(&fichero)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| fichero.clone());

            out.push(Row {
                label: nombre,
                detail: if subs.len() == 1 {
                    "1 tema".to_string()
                } else {
                    format!("{} temas", subs.len())
                },
                depth,
                is_folder: true,
                expanded: abierto,
                path: fichero.clone(),
            });

            if abierto {
                subs.sort_by_key(|t| vgm_path::parse_vgm_path(&t.path).1.unwrap_or(0));
                for t in subs {
                    out.push(Row {
                        label: if t.title.is_empty() {
                            file_label(&t.path)
                        } else {
                            t.title.clone()
                        },
                        detail: format_duration(t.duration_ms),
                        depth: depth + 1,
                        is_folder: false,
                        expanded: false,
                        path: t.path,
                    });
                }
            }
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
