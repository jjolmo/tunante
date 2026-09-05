//! Scanning folders into the library database.
//!
//! The scan reads metadata by spawning `tunante-decoder probe`, one process per
//! file. That is slower per file than an in-process read, and it buys two things
//! worth the cost: the console RAM an emulator-backed reader allocates never
//! lands in this process, and a reader that hangs can be killed. The desktop app
//! wraps its in-process reads in a timeout that cannot actually interrupt a loop
//! running in C — it can only abandon the thread and leak it.
//!
//! This lives beside the decoder client rather than in `tunante-core` because
//! that is what it is: the whole function is "drive N helper processes over a
//! directory". `tunante-core` knows nothing about spawning anything.
//!
//! # What a scan actually costs
//!
//! Measured on a Galaxy S23, 200 files, one process each:
//!
//! ```text
//! 200 × /system/bin/true      918 ms    4 ms/exec
//! 200 × probe                 849 ms    4 ms/file
//! 200 × probe --fast          682 ms    3 ms/file
//! ```
//!
//! A probe costs the same as executing a binary that does nothing, so the price
//! is process startup and nothing else. Two thousand tracks is about eight
//! seconds on one thread, and this runs on several. Worth knowing before anyone
//! is tempted to add a batch mode to the helper to "fix" the spawn count.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tunante_core::db::models::Track;
use tunante_core::db::Database;
use tunante_core::vgm_path;
use walkdir::WalkDir;

/// How long a single file gets before the scanner gives up on it.
///
/// Generous, because a PSF2 or USF set legitimately takes seconds to open. The
/// point is not speed, it is that a file which never returns cannot stall the
/// whole scan.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
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
/// already puts every decode in its own process, and on a phone's eight cores
/// it is the difference between a scan of minutes and one of half an hour.
///
/// SQLite is left on this thread: the writes are trivial next to the probes, and
/// keeping one writer avoids `SQLITE_BUSY` entirely.
pub fn scan_folder(
    db: &Database,
    root: &Path,
    on_progress: impl FnMut(&ScanProgress),
) -> Result<usize, String> {
    scan_folder_with(
        db,
        root,
        &crate::ProbeOpts {
            fast: true,
            ..Default::default()
        },
        on_progress,
    )
}

/// [`scan_folder`], with the caller's scan knobs travelling into every probe.
pub fn scan_folder_with(
    db: &Database,
    root: &Path,
    opts: &crate::ProbeOpts,
    on_progress: impl FnMut(&ScanProgress),
) -> Result<usize, String> {
    static NEVER: AtomicBool = AtomicBool::new(false);
    scan_folder_cancellable(db, root, opts, &NEVER, on_progress)
}

/// [`scan_folder_with`] that stops when `cancel` is raised: what was already
/// inserted stays, the files still queued are dropped, the workers finish the
/// probe in their hands and return. The UI's "Cancelar" sets the flag from
/// another thread; the scan polls it once per file.
pub fn scan_folder_cancellable(
    db: &Database,
    root: &Path,
    opts: &crate::ProbeOpts,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(&ScanProgress),
) -> Result<usize, String> {
    let all: Vec<PathBuf> = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| vgm_path::is_audio_file(p))
        .collect();

    // Incremental: a file whose size, mtime and probe options match what the
    // last scan stamped is not probed again. New and changed files are; so is
    // everything when the scan options change (the flavor), because slow scan
    // versus fast changes what a probe reports.
    let flavor = flavor_of(opts);
    let mut stamps: std::collections::HashMap<PathBuf, (i64, i64)> = std::collections::HashMap::new();
    let mut files: Vec<PathBuf> = Vec::with_capacity(all.len());
    let mut skipped = 0usize;
    for p in all {
        let Some((size, mtime)) = file_stamp(&p) else {
            files.push(p);
            continue;
        };
        let known = db
            .scan_stamp(&p.to_string_lossy())
            .ok()
            .flatten()
            .is_some_and(|(s, m, f)| s == size && m == mtime && f == flavor);
        if known {
            skipped += 1;
        } else {
            stamps.insert(p.clone(), (size, mtime));
            files.push(p);
        }
    }

    let mut progress = ScanProgress {
        scanned: skipped,
        total: skipped + files.len(),
        added: 0,
        failed: 0,
        current: String::new(),
    };
    if skipped > 0 {
        on_progress(&progress);
    }

    // Leave a core or two for the session — this runs on a phone the user is
    // holding, and a scan that makes the interface stutter is worse than a slow
    // one.
    let workers = std::thread::available_parallelism()
        .map(|n| (n.get().saturating_sub(2)).max(2))
        .unwrap_or(2);

    let queue = std::sync::Arc::new(std::sync::Mutex::new(files.into_iter()));
    let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, String, Result<Vec<serde_json::Value>, String>)>();

    let mut handles = Vec::new();
    for _ in 0..workers {
        let queue = queue.clone();
        let tx = tx.clone();
        let opts = opts.clone();
        handles.push(std::thread::spawn(move || loop {
            let next = { queue.lock().ok().and_then(|mut q| q.next()) };
            let Some(path) = next else { return };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let result = crate::probe_with(&path, PROBE_TIMEOUT, &opts);
            if tx.send((path, name, result)).is_err() {
                return;
            }
        }));
    }
    drop(tx);

    // Writes go in batches inside one transaction: one fsync per batch, not
    // per row. The batch is bounded by rows and by time, so the write lock is
    // never held long enough to stall the UI thread's own writes.
    let mut in_tx = db.begin().is_ok();
    let mut batch_rows = 0usize;
    let mut batch_started = std::time::Instant::now();

    for (path, name, result) in rx {
        if cancel.load(Ordering::Relaxed) {
            // Empty the queue so every worker returns after its current
            // probe; the receiver then runs dry and the loop ends.
            if let Ok(mut q) = queue.lock() {
                q.by_ref().for_each(drop);
            }
            break;
        }
        progress.scanned += 1;
        progress.current = name;

        match result {
            Ok(tracks) => {
                // One file can hold many tracks: a GME set or a vgmstream
                // container has one per subsong, each addressed as `path#n`.
                let mut all_in = true;
                for value in tracks {
                    match serde_json::from_value::<Track>(value) {
                        Ok(track) => {
                            if db.insert_track(&track).is_ok() {
                                progress.added += 1;
                                batch_rows += 1;
                            } else {
                                all_in = false;
                            }
                        }
                        Err(_) => {
                            progress.failed += 1;
                            all_in = false;
                        }
                    }
                }
                // Stamp only what went in whole, so a partial file is retried.
                if all_in {
                    if let Some((size, mtime)) = stamps.get(&path) {
                        let _ = db.set_scan_stamp(&path.to_string_lossy(), *size, *mtime, flavor);
                    }
                }
            }
            Err(_) => progress.failed += 1,
        }

        if in_tx && (batch_rows >= 200 || batch_started.elapsed() > Duration::from_millis(250)) {
            let _ = db.commit();
            in_tx = db.begin().is_ok();
            batch_rows = 0;
            batch_started = std::time::Instant::now();
        }

        on_progress(&progress);
    }
    if in_tx {
        let _ = db.commit();
    }

    for h in handles {
        let _ = h.join();
    }

    Ok(progress.added)
}

/// Size and mtime of a file, as the stamp remembers them.
///
/// The mtime is the newest of the file and its sidecars — the `.m3u` a GME set
/// takes its track names and lengths from (`<stem>.m3u` or `<file>.m3u`) and
/// the folder's `_ratings.m3u` — because a probe reads those too, and editing
/// one changes what it reports without touching the audio file.
fn file_stamp(path: &Path) -> Option<(i64, i64)> {
    fn mtime_of(p: &Path) -> Option<i64> {
        std::fs::metadata(p)
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    }
    let meta = std::fs::metadata(path).ok()?;
    let mut newest = mtime_of(path)?;
    let mut sidecars: Vec<PathBuf> = vec![path.with_extension("m3u")];
    if let Some(name) = path.file_name() {
        let mut n = name.to_os_string();
        n.push(".m3u");
        sidecars.push(path.with_file_name(n));
    }
    if let Some(dir) = path.parent() {
        sidecars.push(dir.join("_ratings.m3u"));
    }
    for sc in sidecars {
        if let Some(m) = mtime_of(&sc) {
            newest = newest.max(m);
        }
    }
    Some((meta.len() as i64, newest))
}

/// The probe options folded into one number: the same options give the same
/// flavor, and a change in any of them makes every stamp stale.
pub fn flavor_of(opts: &crate::ProbeOpts) -> i64 {
    let mut f: i64 = 1; // 0 is "never stamped"
    if opts.fast {
        f |= 1 << 1;
    }
    if opts.caps_all {
        f |= 1 << 2;
    }
    f ^= opts.loop_max_ms.unwrap_or(-1).wrapping_mul(31).wrapping_shl(3);
    f ^= (opts.vgm_loop_count.map(|v| (v * 100.0) as i64).unwrap_or(-1)).wrapping_mul(1_000_003);
    f
}

/// Forget tracks under `root` whose files are no longer there.
///
/// Kept separate from [`scan_folder`] rather than folded into it, because the
/// two answer different questions and only one of them is safe to run blind: a
/// scan of a folder on an SD card that happens not to be mounted would, if it
/// pruned, quietly delete that half of the library.
///
/// Returns how many were removed.
pub fn prune_missing(db: &Database, root: &Path) -> Result<usize, String> {
    let root = root.to_string_lossy().to_string();
    let paths = db.get_track_paths_under(&root).map_err(|e| e.to_string())?;

    let mut removed = 0;
    for path in paths {
        // A subsong address (`path#3`) is several tracks over one file; what has
        // to exist is the file.
        let file = path.split('#').next().unwrap_or(&path);
        if !Path::new(file).exists() && db.remove_track_by_path(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tunante_core::db::models::Track;

    /// A database and a directory of real files, both removed on drop.
    ///
    /// Real files because that is the whole question `prune_missing` asks —
    /// `Path::exists` is not something a fake can answer.
    struct Fixture {
        dir: std::path::PathBuf,
        db_path: std::path::PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let mut dir = std::env::temp_dir();
            dir.push(format!("tunante-prune-{}-{}-{}", tag, std::process::id(), n));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let db_path = dir.join("library.db");
            Self { dir, db_path }
        }

        fn db(&self) -> Database {
            Database::new(&self.db_path).unwrap()
        }

        /// Create a file under the fixture and return its absolute path.
        fn file(&self, rel: &str) -> String {
            let p = self.dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"not really audio").unwrap();
            p.to_string_lossy().to_string()
        }

        /// A path under the fixture that deliberately does not exist.
        fn missing(&self, rel: &str) -> String {
            self.dir.join(rel).to_string_lossy().to_string()
        }

        fn root(&self) -> std::path::PathBuf {
            self.dir.clone()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn insert(db: &Database, id: &str, path: &str) {
        db.insert_track(&Track {
            id: id.into(),
            path: path.into(),
            title: id.into(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            track_number: None,
            disc_number: None,
            duration_ms: 1000,
            sample_rate: None,
            channels: None,
            bitrate: None,
            codec: "test".into(),
            file_size: 0,
            has_artwork: false,
            rating: 0,
            modified_at: 0,
            ..Default::default()
        })
        .unwrap();
    }

    fn paths(db: &Database) -> Vec<String> {
        let mut v: Vec<String> = db.get_all_tracks().unwrap().into_iter().map(|t| t.path).collect();
        v.sort();
        v
    }

    #[test]
    fn a_track_whose_file_is_still_there_survives() {
        let fx = Fixture::new("keep");
        let db = fx.db();
        let there = fx.file("album/a.mp3");
        insert(&db, "1", &there);

        assert_eq!(prune_missing(&db, &fx.root()).unwrap(), 0);
        assert_eq!(paths(&db), [there]);
    }

    #[test]
    fn a_track_whose_file_is_gone_is_forgotten() {
        let fx = Fixture::new("drop");
        let db = fx.db();
        let there = fx.file("album/a.mp3");
        let gone = fx.missing("album/b.mp3");
        insert(&db, "1", &there);
        insert(&db, "2", &gone);

        assert_eq!(prune_missing(&db, &fx.root()).unwrap(), 1);
        assert_eq!(paths(&db), [there]);
    }

    /// A subsong address is several tracks over one file. What has to exist is
    /// the file — `Path::exists` on `x.gbs#3` is always false, and taking that
    /// literally would delete every multi-track rip in the library.
    #[test]
    fn subsongs_are_kept_or_dropped_with_their_file() {
        let fx = Fixture::new("subsong");
        let db = fx.db();
        let real = fx.file("gb/pokemon.gbs");
        insert(&db, "1", &format!("{real}#1"));
        insert(&db, "2", &format!("{real}#2"));

        assert_eq!(
            prune_missing(&db, &fx.root()).unwrap(),
            0,
            "the file is there, so every subsong on it stays"
        );
        assert_eq!(paths(&db).len(), 2);

        std::fs::remove_file(&real).unwrap();
        assert_eq!(prune_missing(&db, &fx.root()).unwrap(), 2);
        assert!(paths(&db).is_empty());
    }

    /// The property that matters most: a scan of one folder must never reach
    /// outside it. An unplugged SD card is a folder full of files that all look
    /// missing, and pruning the whole library over it would be unrecoverable.
    #[test]
    fn pruning_one_folder_never_touches_another() {
        let fx = Fixture::new("scoped");
        let db = fx.db();
        let inside_gone = fx.missing("scanned/a.mp3");
        let outside_gone = fx.missing("elsewhere/b.mp3");
        insert(&db, "1", &inside_gone);
        insert(&db, "2", &outside_gone);

        let scanned = fx.root().join("scanned");
        assert_eq!(prune_missing(&db, &scanned).unwrap(), 1);
        assert_eq!(
            paths(&db),
            [outside_gone],
            "a folder outside the scan was pruned along with it"
        );
    }

    /// The bug this repository already had once, in the query underneath: `_`
    /// is a wildcard to SQLite and an ordinary character in a folder name.
    #[test]
    fn an_underscore_in_the_scanned_folder_does_not_widen_the_scan() {
        let fx = Fixture::new("wildcard");
        let db = fx.db();
        let neighbour = fx.missing("skyXtemple/b.mp3");
        insert(&db, "1", &neighbour);
        std::fs::create_dir_all(fx.root().join("sky_temple")).unwrap();

        let scanned = fx.root().join("sky_temple");
        assert_eq!(prune_missing(&db, &scanned).unwrap(), 0);
        assert_eq!(paths(&db), [neighbour]);
    }
}

#[cfg(test)]
mod incremental_tests {
    use super::*;

    #[test]
    fn flavor_changes_with_every_option() {
        let base = crate::ProbeOpts { fast: true, ..Default::default() };
        let slow = crate::ProbeOpts { fast: false, ..base.clone() };
        let capped = crate::ProbeOpts { loop_max_ms: Some(120_000), ..base.clone() };
        let looped = crate::ProbeOpts { vgm_loop_count: Some(2.0), ..base.clone() };
        let all = crate::ProbeOpts { caps_all: true, ..base.clone() };
        let fs: Vec<i64> = [&base, &slow, &capped, &looped, &all].iter().map(|o| flavor_of(o)).collect();
        for i in 0..fs.len() {
            assert_ne!(fs[i], 0);
            for j in 0..i {
                assert_ne!(fs[i], fs[j], "options {i} and {j} share a flavor");
            }
        }
        assert_eq!(flavor_of(&base), flavor_of(&base.clone()));
    }

    #[test]
    fn a_sidecar_edit_changes_the_stamp() {
        let dir = std::env::temp_dir().join(format!("tunante-sidecar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("game.nsf");
        std::fs::write(&audio, b"NESM").unwrap();
        let before = file_stamp(&audio).unwrap();
        // A sidecar written later than the audio moves the stamp forward.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.join("game.m3u"), b"# names").unwrap();
        let after = file_stamp(&audio).unwrap();
        assert_eq!(before.0, after.0);
        assert!(after.1 > before.1, "the .m3u's mtime must move the stamp");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.join("_ratings.m3u"), b"# ratings").unwrap();
        assert!(file_stamp(&audio).unwrap().1 > after.1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stamp_round_trips() {
        let dir = std::env::temp_dir().join(format!("tunante-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::new(&dir.join("t.db")).unwrap();
        assert_eq!(db.scan_stamp("/a/b.mp3").unwrap(), None);
        db.set_scan_stamp("/a/b.mp3", 10, 20, 7).unwrap();
        assert_eq!(db.scan_stamp("/a/b.mp3").unwrap(), Some((10, 20, 7)));
        db.set_scan_stamp("/a/b.mp3", 11, 20, 7).unwrap();
        assert_eq!(db.scan_stamp("/a/b.mp3").unwrap(), Some((11, 20, 7)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Times a real scan twice — full, then incremental — over the folder in
    /// `TUNANTE_BENCH_DIR`. Needs the decoder built in release; ignored so CI
    /// never waits on it: `cargo test -p tunante-helper --release bench_real -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_real_scan() {
        let Some(dir) = std::env::var_os("TUNANTE_BENCH_DIR") else { return };
        let tmp = std::env::temp_dir().join(format!("tunante-bench-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let db = Database::new(&tmp.join("t.db")).unwrap();
        let opts = crate::ProbeOpts { fast: true, ..Default::default() };
        for pass in ["full", "incremental"] {
            let t = std::time::Instant::now();
            let mut last = ScanProgress { scanned: 0, total: 0, added: 0, failed: 0, current: String::new() };
            let added = scan_folder_with(&db, Path::new(&dir), &opts, |p| last = p.clone()).unwrap();
            eprintln!(
                "{pass}: {} files, {added} tracks added, {} failed, in {:.1} s",
                last.total, last.failed, t.elapsed().as_secs_f64()
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
