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
use std::rc::Rc;

use tunante_core::db::models::Track;
use tunante_core::db::Database;
use tunante_core::vgm_path;


// The scan moved to `tunante-helper::scan`, which tunante-android needs too.
// Re-exported so the rest of this module and its callers read unchanged.

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
    /// One row per game, from the album tag rather than from the directory.
    ///
    /// Not the same index as Albums: that one is the disk's opinion and this is
    /// the ripper's. They disagree for a rip split across `Disc 1` and `Disc 2`,
    /// for a folder holding several games, and for anything tagged properly and
    /// filed loose.
    Games,
    /// The saved playlists. Not an index over the library like the two above:
    /// the only view whose contents and order the user chose by hand.
    Playlists,
}

impl Mode {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Mode::Albums,
            2 => Mode::Consoles,
            3 => Mode::Games,
            4 => Mode::Playlists,
            _ => Mode::Tree,
        }
    }
}

/// "1 pista" y no "1 pistas". Translated like the rest of the UI.
pub fn pistas(n: usize) -> String {
    if n == 1 {
        crate::i18n::tr("1 pista")
    } else {
        crate::i18n::tr("{} pistas").replace("{}", &n.to_string())
    }
}

fn juegos(n: usize) -> String {
    if n == 1 {
        crate::i18n::tr("1 juego")
    } else {
        crate::i18n::tr("{} juegos").replace("{}", &n.to_string())
    }
}

// Every view groups tracks by console the same way, and tunante-android needs
// it too, so the grouping lives in `tunante_core::console` rather than here.
// Everything with no console is grouped rather than dropped: the point of this
// view is to reach music, and hiding a third of the library because it is an
// mp3 would defeat it.
pub use tunante_core::console::{
    display_order as console_order, key_of as console_key, label_es as console_label,
};

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
    /// Qué vista está activa.
    ///
    /// Aquí y no leyéndola de la interfaz cuando hace falta: el estado es de
    /// Rust, la interfaz sólo avisa de que cambió. Releer la propiedad hacía
    /// que entrar en un disco de la rejilla mostrase el árbol, porque en ese
    /// instante devolvía todavía el modo anterior.
    pub mode: Mode,
    /// Filtro sobre lo que se ve, en las vistas de rejilla. Vacío = todo.
    pub filter: String,
    /// Every track under the roots, loaded once. Discos, Juegos and Consolas
    /// all group over the whole library; without this each was re-querying
    /// SQLite for 30 000 rows on every view switch AND every filter keystroke.
    all_cache: std::cell::RefCell<Option<Rc<Vec<Track>>>>,
    /// The unfiltered grid for each (mode, nav) already computed. Filtering is
    /// then a cheap pass over the cached cells instead of rebuilding the index.
    grid_cache: std::cell::RefCell<std::collections::HashMap<String, Vec<Cell>>>,
    /// Dónde estamos dentro de una vista de rejilla.
    ///
    /// Vacía = el nivel de arriba. En Consolas, `[consola]` son sus juegos y
    /// `[consola, carpeta]` son las pistas de uno; en Discos, `[carpeta]` son
    /// las pistas. Separada de `expanded` a propósito: el árbol despliega en el
    /// sitio y la rejilla entra dentro, y mezclarlas haría que volver de una
    /// consola dejase medio árbol abierto.
    pub nav: Vec<String>,
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
        Self {
            roots,
            expanded,
            cache: Default::default(),
            mode: Mode::Tree,
            filter: String::new(),
            nav: Vec::new(),
            all_cache: Default::default(),
            grid_cache: Default::default(),
        }
    }

    /// Every track under the roots, loaded once and kept. Callers that group
    /// the whole library (Discos/Juegos/Consolas) share this instead of each
    /// re-reading SQLite. Invalidated by [`Tree::invalidate`] on a rescan.
    fn all_tracks(&self, db: &Database) -> Rc<Vec<Track>> {
        if let Some(a) = self.all_cache.borrow().as_ref() {
            return a.clone();
        }
        let mut all = Vec::new();
        for root in &self.roots {
            all.extend(
                db.get_tracks_by_folder(&root.to_string_lossy())
                    .unwrap_or_default(),
            );
        }
        let rc = Rc::new(all);
        *self.all_cache.borrow_mut() = Some(rc.clone());
        rc
    }

    /// Drop every cache. Called when the library changes on disk (a scan, a
    /// folder added or removed), so the next read rebuilds from the database.
    pub fn invalidate(&self) {
        self.all_cache.borrow_mut().take();
        self.grid_cache.borrow_mut().clear();
        self.cache.borrow_mut().clear();
    }

    pub fn toggle(&mut self, path: &str) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_string());
        }
    }

    /// The expanded set, for persisting — the old desktop remembered it
    /// under files_expanded_folders and so does this one.
    pub fn expanded_list(&self) -> Vec<String> {
        let mut v: Vec<String> = self.expanded.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn set_expanded_paths(&mut self, paths: impl IntoIterator<Item = String>) {
        self.expanded.extend(paths);
    }

    /// Expand a folder AND every ancestor, so a search hit becomes visible
    /// in the tree instead of silently expanded under a collapsed parent.
    pub fn reveal(&mut self, path: &str) {
        let mut p = std::path::Path::new(path);
        loop {
            self.expanded.insert(p.to_string_lossy().to_string());
            let Some(parent) = p.parent() else { break };
            if self.roots.iter().any(|r| r == p) {
                break;
            }
            p = parent;
        }
    }

    /// Folders whose name matches, flat and capped — the old desktop's
    /// «Find folder…» box, riding the same query `albums` uses.
    pub fn matching_folders(&self, db: &Database, q: &str, cap: usize) -> Vec<(String, usize)> {
        let q = plegar(q);
        self.albums(db)
            .into_iter()
            .filter(|(path, _)| {
                std::path::Path::new(path)
                    .file_name()
                    .map(|n| plegar(&n.to_string_lossy()).contains(&q))
                    .unwrap_or(false)
            })
            .take(cap)
            .collect()
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
                let needle = plegar(self.filter.trim());
                if !needle.is_empty() {
                    // Filtering the tree flattens it to the matching tracks —
                    // the old app's behaviour, and what the search box promises.
                    // Title or filename, plus the path so an untagged folder
                    // full of rips is findable by its name.
                    let matched: Vec<Track> = self
                        .all_tracks(db)
                        .iter()
                        .filter(|t| {
                            let name = if t.title.is_empty() {
                                file_label(&t.path)
                            } else {
                                t.title.clone()
                            };
                            plegar(&name).contains(&needle)
                                || plegar(&t.path).contains(&needle)
                        })
                        .take(2000)
                        .cloned()
                        .collect();
                    let mut out = Vec::new();
                    self.push_tracks(matched, 0, &mut out);
                    return out;
                }
                let mut out = Vec::new();
                for root in &self.roots {
                    self.push_folder(db, root, 0, &mut out);
                }
                out
            }
            Mode::Albums => self.rows_albums(db),
            Mode::Consoles => self.rows_consoles(db),
            // Games has no arm here on purpose. Every mode but Tree draws as a
            // grid — `grid_unfiltered` answers for the top level and
            // `grid_tracks` for the one below — so `rows_for` is only ever
            // called with Tree. rows_albums and rows_consoles above are already
            // unreachable for the same reason; adding a third would be adding
            // to a mistake rather than matching a pattern.
            Mode::Games => Vec::new(),
            // Las listas no salen del árbol ni de un índice sobre él: las arma
            // `refresh_library` desde la base, en el orden que alguien eligió.
            Mode::Playlists => Vec::new(),
        }
    }

    /// Every folder that directly holds music, flat and in one query.
    ///
    /// One query rather than walking the tree: the alternative reads every
    /// directory under the roots looking for one with audio in it, and on a
    /// phone that is a lot of `readdir` for an answer the database already has.
    fn albums(&self, db: &Database) -> Vec<(String, usize)> {
        let mut count: BTreeMap<String, usize> = BTreeMap::new();
        for t in self.all_tracks(db).iter() {
            let real = vgm_path::parse_vgm_path(&t.path).0;
            if let Some(dir) = Path::new(real).parent() {
                *count.entry(dir.to_string_lossy().to_string()).or_default() += 1;
            }
        }
        count.into_iter().collect()
    }

    /// Every game in the library, from `tunante_core::games`.
    ///
    /// Shared with tunante-android rather than written twice: the awkward parts
    /// — an untagged rip falling back to its folder, a subsong suffix that is
    /// not part of any name, one bad tag not renaming a whole game's composer —
    /// are tested there.
    fn games(&self, db: &Database) -> Vec<tunante_core::games::Game> {
        tunante_core::games::index(&self.all_tracks(db))
    }

    fn game_tracks(&self, db: &Database, game: &str) -> Vec<tunante_core::db::models::Track> {
        tunante_core::games::tracks_of(&self.all_tracks(db), game)
            .into_iter()
            .cloned()
            .collect()
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
        let mut by_console: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for root in &self.roots {
            for t in db
                .get_tracks_by_folder(&root.to_string_lossy())
                .unwrap_or_default()
            {
                let real = vgm_path::parse_vgm_path(&t.path).0;
                let Some(dir) = Path::new(real).parent() else { continue };
                *by_console
                    .entry(console_key(&t).to_string())
                    .or_default()
                    .entry(dir.to_string_lossy().to_string())
                    .or_default() += 1;
            }
        }

        let mut out = Vec::new();
        let mut consoles: Vec<(String, BTreeMap<String, usize>)> = by_console.into_iter().collect();
        consoles.sort_by(|a, b| console_order(&a.0).cmp(&console_order(&b.0)));
        for (console, albums) in consoles {
            // A key that cannot collide with a path, so the same `expanded` set
            // serves all three views without them stepping on each other.
            let key = format!("consola:{console}");
            let abierto = self.is_expanded(&key);
            let total: usize = albums.values().sum();

            out.push(Row {
                label: console_label(&console).to_string(),
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
                        .filter(|t| console_key(t) == console)
                        .collect();
                    self.push_tracks(tracks, 2, &mut out);
                }
            }
        }
        out
    }

    fn push_folder(&self, db: &Database, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        let mut key = dir.to_string_lossy().to_string();
        let mut label = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| key.clone());

        let mut contents = self.contents(db, &key, dir);
        // Single-child chains compact into one row, VS Code style: a/b/c
        // instead of three clicks through folders that hold nothing but the
        // next folder.
        let mut dir = dir.to_path_buf();
        while contents.tracks.is_empty() && contents.subdirs.len() == 1 {
            let only = contents.subdirs[0].clone();
            let Some(name) = only.file_name().map(|n| n.to_string_lossy().to_string()) else {
                break;
            };
            label = format!("{label}/{name}");
            key = only.to_string_lossy().to_string();
            contents = self.contents(db, &key, &only);
            dir = only;
        }
        let _ = &dir;
        let expanded = self.is_expanded(&key);

        let FolderContents { tracks, total, subdirs } = contents;

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

/// A playlist's entries as rows: flat, one per entry, in the order stored.
///
/// Deliberately not `push_tracks`. That one groups a file's subsongs under a
/// collapsible header and sorts by real path, which is right for a folder and
/// wrong here twice over: a playlist's order is the one thing the user chose,
/// and a subsong they picked one by one should not be folded back into the file
/// it came from.
pub fn playlist_rows(tracks: &[Track]) -> Vec<Row> {
    tracks
        .iter()
        .map(|t| Row {
            label: if t.title.is_empty() { file_label(&t.path) } else { t.title.clone() },
            detail: format_duration(t.duration_ms),
            depth: 0,
            is_folder: false,
            expanded: false,
            path: t.path.clone(),
        })
        .collect()
}

pub fn format_duration(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Una tarjeta de la rejilla.
#[derive(Clone)]
pub struct Cell {
    pub title: String,
    pub subtitle: String,
    /// Ruta real, o sintética (`consola:NES`).
    pub path: String,
    /// De qué carpeta sacar la portada. Vacío en las consolas: ésas se dibujan.
    pub art_dir: String,
    /// El nombre de la consola cuando la celda es una consola. Vacío si no.
    pub console: String,
}

impl Tree {
    /// Qué se ve ahora mismo en una vista de rejilla, y si es rejilla siquiera.
    ///
    /// Devuelve `None` cuando el nivel actual son pistas: eso se dibuja como
    /// lista, porque un disco puede tener trescientas y una rejilla de trescientas
    /// tarjetas no se lee.
    pub fn grid(&self, db: &Database, mode: Mode) -> Option<Vec<Cell>> {
        let celdas = self.grid_unfiltered(db, mode)?;
        if self.filter.trim().is_empty() {
            return Some(celdas);
        }
        // Sobre el título, que es lo que se ve. Sin acentos ni mayúsculas:
        // "pokemon" tiene que encontrar "Pokémon".
        let q = plegar(self.filter.trim());
        Some(
            celdas
                .into_iter()
                .filter(|c| plegar(&c.title).contains(&q))
                .collect(),
        )
    }

    fn grid_unfiltered(&self, db: &Database, mode: Mode) -> Option<Vec<Cell>> {
        // Keyed by view and where we are in it: the same grid is asked for on
        // every filter keystroke, and rebuilding the whole index each time is
        // what made typing lag. Cleared on a rescan by `invalidate`.
        let key = format!("{}|{}", mode as u8, self.nav.join("\u{1f}"));
        if let Some(hit) = self.grid_cache.borrow().get(&key) {
            return Some(hit.clone());
        }
        let built = self.grid_unfiltered_build(db, mode);
        if let Some(cells) = &built {
            self.grid_cache
                .borrow_mut()
                .insert(key, cells.clone());
        }
        built
    }

    fn grid_unfiltered_build(&self, db: &Database, mode: Mode) -> Option<Vec<Cell>> {
        match (mode, self.nav.len()) {
            (Mode::Albums, 0) => Some(
                self.albums(db)
                    .into_iter()
                    .map(|(dir, n)| Cell {
                        title: nombre_de(&dir),
                        subtitle: pistas(n),
                        art_dir: dir.clone(),
                        console: String::new(),
                        path: dir,
                    })
                    .collect(),
            ),
            (Mode::Games, 0) => Some(
                self.games(db)
                    .into_iter()
                    .map(|g| Cell {
                        title: g.name.clone(),
                        // Just the track count — the Games tab shows the game,
                        // not who composed it.
                        subtitle: pistas(g.count),
                        // The cover comes from wherever the first track lives.
                        // A game split across discs takes disc one's, which is
                        // the one that has the artwork in practice.
                        art_dir: Path::new(vgm_path::parse_vgm_path(&g.first_track).0)
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        console: String::new(),
                        // Synthetic, like `consola:NES`. A game is a name and
                        // the rest of the app resolves a row by treating its
                        // path as one; without a prefix to say otherwise,
                        // playing or queueing a game silently does nothing.
                        path: format!("juego:{}", g.name),
                    })
                    .collect(),
            ),
            (Mode::Consoles, 0) => {
                let mut por_consola: BTreeMap<String, (usize, usize)> = BTreeMap::new();
                for (consola, _dir, n) in self.console_index(db) {
                    let e = por_consola.entry(consola).or_default();
                    e.0 += 1;
                    e.1 += n;
                }
                let mut consolas: Vec<(String, (usize, usize))> = por_consola.into_iter().collect();
                consolas.sort_by(|a, b| console_order(&a.0).cmp(&console_order(&b.0)));
                Some(
                    consolas
                        .into_iter()
                        .map(|(c, (_juegos_n, pistas_n))| Cell {
                            title: console_label(&c).to_string(),
                            // Sólo las pistas: "4 juegos · 489 pistas" no cabe
                            // en una tarjeta de tres columnas y se cortaba en
                            // "489 pista". Cuántos juegos hay se ve al entrar.
                            subtitle: pistas(pistas_n),
                            path: format!("consola:{c}"),
                            // El aparato se dibuja. La portada del primer juego
                            // era un parche: decía "Sonic" donde pone "NES".
                            art_dir: String::new(),
                            console: c.clone(),
                        })
                        .collect(),
                )
            }
            // A console no longer drills into its folders: level 1 is its whole
            // track list (None → the list view, filled by grid_tracks).
            _ => None,
        }
    }

    /// Migas: cómo se llama el sitio donde estamos, vacío en el nivel de arriba.
    pub fn crumb(&self) -> String {
        match self.nav.last() {
            None => String::new(),
            Some(k) => {
                if let Some(c) = k.strip_prefix("consola:") {
                    c.to_string()
                } else if let Some(g) = k.strip_prefix("juego:") {
                    // Whole, not `file_name`: a game tagged "Hack//Sign" is not
                    // a path and has no last component to take.
                    g.to_string()
                } else {
                    nombre_de(k)
                }
            }
        }
    }

    /// Las pistas del nivel actual, cuando el nivel actual son pistas.
    pub fn grid_tracks(&self, db: &Database, mode: Mode) -> Vec<Row> {
        let Some(dir) = self.nav.last() else { return Vec::new() };
        // A console opens straight to every track it holds, flat — not down
        // into its folders. Desktop shows the same set in the powerful table.
        if mode == Mode::Consoles && self.nav.len() == 1 {
            let quiero = self.nav[0].trim_start_matches("consola:").to_string();
            let tracks: Vec<Track> = self
                .all_tracks(db)
                .iter()
                .filter(|t| console_key(t) == quiero)
                .cloned()
                .collect();
            let mut out = Vec::new();
            self.push_tracks(tracks, 0, &mut out);
            if !self.filter.trim().is_empty() {
                let q = plegar(self.filter.trim());
                out.retain(|r| plegar(&r.label).contains(&q));
            }
            return out;
        }
        if mode == Mode::Consoles && self.nav.is_empty() {
            return Vec::new();
        }
        // A game is a name, not a directory, so its tracks cannot come from
        // read_dir the way every other grid level's do.
        if mode == Mode::Games {
            let mut out = Vec::new();
            let game = dir.strip_prefix("juego:").unwrap_or(dir);
            self.push_tracks(self.game_tracks(db, game), 0, &mut out);
            if !self.filter.trim().is_empty() {
                let q = plegar(self.filter.trim());
                out.retain(|r| plegar(&r.label).contains(&q));
            }
            return out;
        }
        let mut tracks = self.contents(db, dir, Path::new(dir)).tracks;
        if mode == Mode::Consoles {
            let quiero = self.nav[0].trim_start_matches("consola:").to_string();
            tracks.retain(|t| console_key(t) == quiero);
        }
        let mut out = Vec::new();
        self.push_tracks(tracks, 0, &mut out);
        if !self.filter.trim().is_empty() {
            let q = plegar(self.filter.trim());
            out.retain(|r| plegar(&r.label).contains(&q));
        }
        out
    }

    /// (consola, carpeta, cuántas pistas de esa consola hay en ella)
    /// (console id, es label, track count) for the sidebar, catalog order,
    /// only consoles that actually hold music.
    pub fn console_counts(&self, db: &Database) -> Vec<(String, String, usize)> {
        let mut acc: BTreeMap<String, usize> = BTreeMap::new();
        for root in &self.roots {
            for t in db
                .get_tracks_by_folder(&root.to_string_lossy())
                .unwrap_or_default()
            {
                *acc.entry(console_key(&t).to_string()).or_default() += 1;
            }
        }
        let mut out: Vec<(String, String, usize)> = acc
            .into_iter()
            .map(|(c, n)| {
                let label = console_label(&c).to_string();
                (c, label, n)
            })
            .collect();
        out.sort_by(|a, b| console_order(&a.0).cmp(&console_order(&b.0)));
        out
    }

    fn console_index(&self, db: &Database) -> Vec<(String, String, usize)> {
        let mut acc: BTreeMap<(String, String), usize> = BTreeMap::new();
        for t in self.all_tracks(db).iter() {
            let real = vgm_path::parse_vgm_path(&t.path).0;
            let Some(dir) = Path::new(real).parent() else { continue };
            *acc.entry((
                console_key(&t).to_string(),
                dir.to_string_lossy().to_string(),
            ))
            .or_default() += 1;
        }
        let mut out: Vec<(String, String, usize)> =
            acc.into_iter().map(|((c, d), n)| (c, d, n)).collect();
        out.sort_by(|a, b| console_order(&a.0).cmp(&console_order(&b.0)).then(a.1.cmp(&b.1)));
        out
    }
}

/// Minúsculas y sin acentos, para comparar como compara la gente.
pub fn plegar(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            otro => otro,
        })
        .collect()
}

fn nombre_de(ruta: &str) -> String {
    Path::new(ruta)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| ruta.to_string())
}

/// La imagen que hay en una carpeta, si la hay.
///
/// Deliberadamente aquí y no pidiéndosela al decodificador: es leer un
/// directorio, no emular nada, y un proceso por tarjeta para pintar una rejilla
/// de veintiocho sería absurdo. La carátula incrustada en un fichero sigue
/// siendo cosa del decodificador, y sólo se pide para lo que está sonando.

// Moved down to `tunante-art`, which the desktop app needs too and which does
// not drag in the decoder-process client the way `tunante-helper` would.
pub use tunante_art::folder::folder_image;

