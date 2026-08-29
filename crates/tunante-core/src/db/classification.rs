//! Storing what [`crate::classify`] decided, and what the user decided instead.
//!
//! Two tables, with opposite lifetimes. `classification_overrides` is typed by
//! a person and must survive everything; `track_classification` is derived and
//! is thrown away whenever the rules change. See `schema.rs` for why one has a
//! foreign key and the other must not.
//!
//! Kept out of `db/mod.rs` because that file is already 1400 lines.

use super::{like_prefix, Database, DbError};
use crate::classify::{Classification, Classifier, Override};
use crate::db::models::Track;
use rusqlite::params;
use std::collections::HashMap;
use std::sync::Arc;

/// Bumped whenever the console table or the resolution rules change.
///
/// The entire release procedure for "I added a folder alias" is to increment
/// this: the next open notices the mismatch and rebuilds the derived table.
/// Rebuilding is idempotent and destroys nothing a user typed, so unlike the
/// playlist-position seeding next door there is no need to distinguish "never
/// run" from "run by an older build".
// 2: the console may be named by any path segment, not only the first one
//    below the registered root. Libraries whose root sits above the console
//    folders were classified as Unknown wholesale, so every row has to be
//    stamped again for the fix to reach an existing database.
// 3: the game named by the file's own header is a field of its own and outranks
//    the album, which names a release rather than a game.
pub const CLASSIFIER_VERSION: u32 = 3;

const VERSION_KEY: &str = "classifier_version";

/// A correction, as it travels to a UI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClassificationOverride {
    pub id: String,
    /// `"track"` or `"folder"`.
    pub scope: String,
    pub target: String,
    pub console_id: Option<String>,
    pub game_name: Option<String>,
    pub created_at: i64,
}

/// A folder holding tracks nothing could classify — the worklist for flagging.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnclassifiedFolder {
    pub folder: String,
    pub track_count: i64,
    /// One track in it, so a UI can offer to play or inspect it.
    pub sample_path: String,
}

/// SQLite's default `SQLITE_MAX_VARIABLE_NUMBER` is 999.
const PARAM_CHUNK: usize = 500;

impl Database {
    // --- the classifier itself ---

    /// The rules, with the registered roots and the user's corrections folded
    /// in. Built once and cached until something it depends on changes.
    pub fn classifier(&self) -> Result<Arc<Classifier>, DbError> {
        if let Some(c) = self.classifier.borrow().as_ref() {
            return Ok(Arc::clone(c));
        }
        let built = Arc::new(self.build_classifier()?);
        *self.classifier.borrow_mut() = Some(Arc::clone(&built));
        Ok(built)
    }

    fn build_classifier(&self) -> Result<Classifier, DbError> {
        let roots: Vec<String> = self
            .conn
            .prepare("SELECT path FROM monitored_folders")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut tracks = HashMap::new();
        let mut folders = HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT scope, target, console_id, game_name FROM classification_overrides")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (scope, target, console_id, game_name) = row?;
            let o = Override { console_id, game_name };
            match scope.as_str() {
                "track" => {
                    tracks.insert(target, o);
                }
                _ => {
                    folders.insert(target, o);
                }
            }
        }
        Ok(Classifier::new(roots, tracks, folders))
    }

    /// Forget the cached classifier. Call after anything it was built from
    /// changes — the roots or the overrides.
    pub fn invalidate_classifier(&self) {
        *self.classifier.borrow_mut() = None;
    }

    // --- overrides ---

    /// Record a correction. `scope` is `"track"` or `"folder"`.
    ///
    /// Both halves `None` is a request to forget the correction rather than to
    /// store an empty one, which would otherwise sit there overriding nothing.
    ///
    /// The target is normalised on write, not on read: an override stored with
    /// a trailing slash would silently match nothing forever.
    pub fn set_override(
        &self,
        id: &str,
        scope: &str,
        target: &str,
        console_id: Option<&str>,
        game_name: Option<&str>,
    ) -> Result<(), DbError> {
        let target = crate::classify::normalize_path(target);
        let console_id = console_id.map(str::trim).filter(|s| !s.is_empty());
        let game_name = game_name.map(str::trim).filter(|s| !s.is_empty());
        if console_id.is_none() && game_name.is_none() {
            return self.clear_override(scope, &target);
        }
        self.conn.execute(
            "INSERT INTO classification_overrides (id, scope, target, console_id, game_name)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(scope, target) DO UPDATE SET
               console_id = excluded.console_id,
               game_name = excluded.game_name",
            params![id, scope, target, console_id, game_name],
        )?;
        self.invalidate_classifier();
        self.reclassify_under(&target)?;
        Ok(())
    }

    pub fn clear_override(&self, scope: &str, target: &str) -> Result<(), DbError> {
        let target = crate::classify::normalize_path(target);
        self.conn.execute(
            "DELETE FROM classification_overrides WHERE scope = ?1 AND target = ?2",
            params![scope, target],
        )?;
        self.invalidate_classifier();
        self.reclassify_under(&target)?;
        Ok(())
    }

    pub fn get_overrides(&self) -> Result<Vec<ClassificationOverride>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, target, console_id, game_name, created_at
             FROM classification_overrides ORDER BY target",
        )?;
        let out = stmt
            .query_map([], |r| {
                Ok(ClassificationOverride {
                    id: r.get(0)?,
                    scope: r.get(1)?,
                    target: r.get(2)?,
                    console_id: r.get(3)?,
                    game_name: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(out)
    }

    // --- the derived table ---

    fn write_classification(&self, path: &str, c: &Classification) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO track_classification (path, console_id, console_source, game_name, game_source)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
               console_id = excluded.console_id,
               console_source = excluded.console_source,
               game_name = excluded.game_name,
               game_source = excluded.game_source",
            params![
                path,
                c.console_id.unwrap_or(""),
                c.console_source.as_str(),
                c.game,
                c.game_source.as_str(),
            ],
        )?;
        Ok(())
    }

    /// Classify one track that is already in `tracks`.
    ///
    /// Called from `insert_track`, so the watcher keeps the derived table
    /// correct one file at a time without ever rebuilding the library.
    pub fn classify_path(&self, path: &str, album: &str, codec: &str) -> Result<(), DbError> {
        self.classify_path_full(path, album, "", codec)
    }

    /// As [`Self::classify_path`], with the game the file's header names.
    pub fn classify_path_full(
        &self,
        path: &str,
        album: &str,
        header_game: &str,
        codec: &str,
    ) -> Result<(), DbError> {
        let classifier = self.classifier()?;
        let c = classifier.classify_full(path, album, header_game, codec);
        self.write_classification(path, &c)
    }

    /// Reclassify every track under a path prefix. The prefix may be a single
    /// track's path, in which case exactly one row is rewritten.
    pub fn reclassify_under(&self, prefix: &str) -> Result<usize, DbError> {
        let prefix = crate::classify::normalize_path(prefix);
        let pattern = format!("{}/%", like_prefix(&prefix));
        let rows: Vec<(String, String, String, String)> = self
            .conn
            .prepare(
                "SELECT path, album, header_game, codec FROM tracks
                 WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?
            .query_map(params![prefix, pattern], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        self.classify_rows(&rows)
    }

    /// Rebuild the whole derived table.
    pub fn reclassify_all(&self) -> Result<usize, DbError> {
        let rows: Vec<(String, String, String, String)> = self
            .conn
            .prepare("SELECT path, album, header_game, codec FROM tracks")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        self.conn.execute("DELETE FROM track_classification", [])?;
        self.classify_rows(&rows)
    }

    fn classify_rows(&self, rows: &[(String, String, String, String)]) -> Result<usize, DbError> {
        let classifier = self.classifier()?;
        self.conn.execute_batch("BEGIN")?;
        let result = (|| -> Result<usize, DbError> {
            for (path, album, header_game, codec) in rows {
                let c = classifier.classify_full(path, album, header_game, codec);
                self.write_classification(path, &c)?;
            }
            Ok(rows.len())
        })();
        match result {
            Ok(n) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(n)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Rebuild the derived table if the rules have changed since it was
    /// written. Called once, at open.
    pub(super) fn ensure_classified(&self) -> Result<(), DbError> {
        let stored = self.get_setting(VERSION_KEY)?;
        if stored.as_deref() == Some(CLASSIFIER_VERSION.to_string().as_str()) {
            return Ok(());
        }
        self.reclassify_all()?;
        self.set_setting(VERSION_KEY, &CLASSIFIER_VERSION.to_string())?;
        Ok(())
    }

    // --- reading ---

    /// Fill in `console_id` and `game` on tracks read from the database.
    ///
    /// They are not columns on `tracks`, so every read path calls this rather
    /// than growing its `SELECT` list.
    pub(super) fn stamp(&self, tracks: &mut [Track]) -> Result<(), DbError> {
        if tracks.is_empty() {
            return Ok(());
        }
        let mut found: HashMap<String, (String, String)> = HashMap::new();
        for chunk in tracks.chunks(PARAM_CHUNK) {
            let placeholders = std::iter::repeat("?").take(chunk.len()).collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT path, console_id, game_name FROM track_classification WHERE path IN ({placeholders})"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(
                rusqlite::params_from_iter(chunk.iter().map(|t| t.path.as_str())),
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
            )?;
            for row in rows {
                let (path, console_id, game) = row?;
                found.insert(path, (console_id, game));
            }
        }
        for t in tracks.iter_mut() {
            if let Some((console_id, game)) = found.get(&t.path) {
                t.console_id = console_id.clone();
                t.game = game.clone();
            }
        }
        Ok(())
    }

    /// Folders whose tracks nothing could classify, biggest first.
    ///
    /// Reported one level below the registered root — `Megaten/Persona 5`, not
    /// `Megaten` — because that is the level at which a correction is actually
    /// true. A franchise folder spans several machines, so flagging the whole
    /// thing at once would just be a different wrong answer.
    pub fn unclassified_folders(&self) -> Result<Vec<UnclassifiedFolder>, DbError> {
        let roots: Vec<String> = self
            .conn
            .prepare("SELECT path FROM monitored_folders")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut roots: Vec<String> = roots.iter().map(|r| crate::classify::normalize_path(r)).collect();
        roots.sort_by_key(|r| std::cmp::Reverse(r.len()));

        let mut stmt = self.conn.prepare(
            "SELECT t.path FROM tracks t
             LEFT JOIN track_classification c ON c.path = t.path
             WHERE c.console_id IS NULL OR c.console_id = ''",
        )?;
        let paths = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut grouped: HashMap<String, (i64, String)> = HashMap::new();
        for path in paths {
            let normalized = crate::classify::normalize_path(&path);
            let root = roots.iter().find(|r| {
                normalized.len() > r.len()
                    && normalized.starts_with(r.as_str())
                    && normalized.as_bytes()[r.len()] == b'/'
            });
            // Two levels below the root is the game; one is the franchise. With
            // no root at all, the containing folder is the best on offer.
            let folder = match root {
                Some(r) => {
                    let rel = &normalized[r.len() + 1..];
                    let parts: Vec<&str> = rel.split('/').collect();
                    match parts.len() {
                        0 | 1 => r.clone(),
                        _ => format!("{}/{}", r, parts[..parts.len() - 1].join("/")),
                    }
                }
                None => match normalized.rfind('/') {
                    Some(i) if i > 0 => normalized[..i].to_string(),
                    _ => normalized.clone(),
                },
            };
            let entry = grouped.entry(folder).or_insert((0, path.clone()));
            entry.0 += 1;
        }

        let mut out: Vec<UnclassifiedFolder> = grouped
            .into_iter()
            .map(|(folder, (track_count, sample_path))| UnclassifiedFolder {
                folder,
                track_count,
                sample_path,
            })
            .collect();
        out.sort_by(|a, b| b.track_count.cmp(&a.track_count).then(a.folder.cmp(&b.folder)));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::TempDb;

    const ROOT: &str = "/lib";

    fn seed(db: &Database, paths: &[(&str, &str, &str)]) {
        db.add_monitored_folder("root", ROOT).unwrap();
        for (i, (path, album, codec)) in paths.iter().enumerate() {
            let t = Track {
                id: format!("id{i}"),
                path: (*path).into(),
                album: (*album).into(),
                codec: (*codec).into(),
                ..Default::default()
            };
            db.insert_track(&t).unwrap();
        }
    }

    #[test]
    fn a_track_is_classified_as_it_is_inserted() {
        let tmp = TempDb::new("classify-insert");
        let db = tmp.open();
        seed(&db, &[(&format!("{ROOT}/PSX/Ape Escape/01.mp3"), "", "MP3")]);

        let mut got = db.get_all_tracks().unwrap();
        assert_eq!(got.len(), 1);
        let t = got.pop().unwrap();
        assert_eq!(t.console_id, "ps1");
        assert_eq!(t.game, "Ape Escape");
    }

    /// The claim the schema comment makes about `track_classification`'s
    /// foreign key: rows leave with their tracks through delete paths that have
    /// never heard of this table.
    #[test]
    fn the_derived_row_leaves_with_its_track() {
        let tmp = TempDb::new("classify-cascade");
        let db = tmp.open();
        seed(&db, &[(&format!("{ROOT}/PSX/Grandia/01.mp3"), "", "MP3")]);
        let count = |db: &Database| -> i64 {
            db.conn
                .query_row("SELECT COUNT(*) FROM track_classification", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(&db), 1);
        db.clear_all_tracks().unwrap();
        assert_eq!(count(&db), 0);
    }

    /// And the opposite claim about `classification_overrides`: what the user
    /// typed outlives a full rescan, an unplugged drive, everything.
    #[test]
    fn a_correction_survives_the_library_being_wiped() {
        let tmp = TempDb::new("classify-override-survives");
        let db = tmp.open();
        seed(&db, &[(&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3")]);
        db.set_override("o1", "folder", &format!("{ROOT}/Megaten/Persona 5"), Some("ps4"), None)
            .unwrap();

        db.clear_all_tracks().unwrap();
        assert_eq!(db.get_overrides().unwrap().len(), 1);

        // And it applies again the moment the track comes back.
        seed(&db, &[(&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3")]);
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "ps4");
    }

    /// A correction has to take effect on tracks that are already in the
    /// library, not only on ones scanned afterwards.
    #[test]
    fn a_correction_reclassifies_what_is_already_there() {
        let tmp = TempDb::new("classify-override-retro");
        let db = tmp.open();
        seed(
            &db,
            &[
                (&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3"),
                (&format!("{ROOT}/Megaten/Persona 3/01.mp3"), "", "MP3"),
            ],
        );
        assert!(db.get_all_tracks().unwrap().iter().all(|t| t.console_id.is_empty()));

        db.set_override("o1", "folder", &format!("{ROOT}/Megaten/Persona 5"), Some("ps4"), None)
            .unwrap();

        let tracks = db.get_all_tracks().unwrap();
        let p5 = tracks.iter().find(|t| t.path.contains("Persona 5")).unwrap();
        let p3 = tracks.iter().find(|t| t.path.contains("Persona 3")).unwrap();
        assert_eq!(p5.console_id, "ps4");
        // The sibling is untouched: the override was scoped to one subtree.
        assert_eq!(p3.console_id, "");
    }

    #[test]
    fn clearing_a_correction_puts_the_rules_back_in_charge() {
        let tmp = TempDb::new("classify-override-clear");
        let db = tmp.open();
        seed(&db, &[(&format!("{ROOT}/PSX/Grandia/01.mp3"), "", "MP3")]);
        db.set_override("o1", "folder", &format!("{ROOT}/PSX/Grandia"), Some("switch"), None)
            .unwrap();
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "switch");

        db.clear_override("folder", &format!("{ROOT}/PSX/Grandia")).unwrap();
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "ps1");
    }

    /// Registering a root changes what the segment rule can see, so everything
    /// already stored has to be reconsidered.
    #[test]
    fn registering_a_root_reclassifies_the_library() {
        let tmp = TempDb::new("classify-root");
        let db = tmp.open();
        // Inserted before any root exists: the segment rule cannot fire.
        let t = Track {
            id: "id0".into(),
            path: format!("{ROOT}/PSX/Grandia/01.mp3"),
            codec: "MP3".into(),
            ..Default::default()
        };
        db.insert_track(&t).unwrap();
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "");

        db.add_monitored_folder("root", ROOT).unwrap();
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "ps1");

        db.remove_monitored_folder("root").unwrap();
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "");
    }

    /// Storing both halves empty is a request to forget the correction, not to
    /// keep one that overrides nothing.
    #[test]
    fn an_empty_correction_is_a_deletion() {
        let tmp = TempDb::new("classify-empty-override");
        let db = tmp.open();
        seed(&db, &[(&format!("{ROOT}/PSX/Grandia/01.mp3"), "", "MP3")]);
        db.set_override("o1", "folder", &format!("{ROOT}/PSX/Grandia"), Some("switch"), None)
            .unwrap();
        db.set_override("o1", "folder", &format!("{ROOT}/PSX/Grandia"), Some("  "), None)
            .unwrap();
        assert!(db.get_overrides().unwrap().is_empty());
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "ps1");
    }

    /// A path with `_` in it is a `LIKE` wildcard unless it is escaped, and a
    /// game-music library is full of them — `FF7_psf`, `Chrono_Cross_psf`.
    /// Same bug class `like_prefix` was written for.
    #[test]
    fn an_underscore_in_a_folder_does_not_reclassify_its_neighbours() {
        let tmp = TempDb::new("classify-underscore");
        let db = tmp.open();
        seed(
            &db,
            &[
                (&format!("{ROOT}/PSX/FF7_psf/01.mp3"), "", "MP3"),
                (&format!("{ROOT}/PSX/FF7Xpsf/01.mp3"), "", "MP3"),
            ],
        );
        db.set_override("o1", "folder", &format!("{ROOT}/PSX/FF7_psf"), Some("switch"), None)
            .unwrap();

        let tracks = db.get_all_tracks().unwrap();
        let under = tracks.iter().find(|t| t.path.contains("FF7_psf")).unwrap();
        let neighbour = tracks.iter().find(|t| t.path.contains("FF7Xpsf")).unwrap();
        assert_eq!(under.console_id, "switch");
        assert_eq!(neighbour.console_id, "ps1", "the `_` matched the `X`");
    }

    /// The worklist that drives the flagging UI, at the level a correction is
    /// actually true: one row per game, not one for the whole franchise.
    #[test]
    fn the_flagging_worklist_groups_by_game_and_is_biggest_first() {
        let tmp = TempDb::new("classify-worklist");
        let db = tmp.open();
        seed(
            &db,
            &[
                (&format!("{ROOT}/Megaten/Persona 5/01.mp3"), "", "MP3"),
                (&format!("{ROOT}/Megaten/Persona 5/02.mp3"), "", "MP3"),
                (&format!("{ROOT}/Pokemon/Sun Moon/01.mp3"), "", "MP3"),
                (&format!("{ROOT}/PSX/Grandia/01.mp3"), "", "MP3"),
            ],
        );
        let work = db.unclassified_folders().unwrap();
        let folders: Vec<&str> = work.iter().map(|w| w.folder.as_str()).collect();
        assert_eq!(
            folders,
            [
                format!("{ROOT}/Megaten/Persona 5"),
                format!("{ROOT}/Pokemon/Sun Moon")
            ]
        );
        assert_eq!(work[0].track_count, 2);
        // The one the rules already answered is not on the worklist.
        assert!(!folders.iter().any(|f| f.contains("Grandia")));
    }

    /// Bumping the version is the whole release procedure for a rule change, so
    /// a reopen after a bump has to actually rebuild.
    #[test]
    fn a_version_bump_rebuilds_the_derived_table() {
        let tmp = TempDb::new("classify-version");
        {
            let db = tmp.open();
            seed(&db, &[(&format!("{ROOT}/PSX/Grandia/01.mp3"), "", "MP3")]);
            // Simulate a database written by a build with different rules.
            db.conn.execute("DELETE FROM track_classification", []).unwrap();
            db.set_setting(VERSION_KEY, "0").unwrap();
        }
        let db = tmp.open();
        assert_eq!(db.get_all_tracks().unwrap()[0].console_id, "ps1");
        // Against the constant, not a literal: a hard-coded "1" turns every
        // rule change into a failing test that says nothing about the rule.
        assert_eq!(
            db.get_setting(VERSION_KEY).unwrap().as_deref(),
            Some(CLASSIFIER_VERSION.to_string().as_str())
        );
    }

    /// `stamp` chunks its parameters to stay under SQLite's limit, and the
    /// chunk boundary is exactly where an off-by-one hides.
    #[test]
    fn stamping_works_past_the_parameter_chunk_boundary() {
        let tmp = TempDb::new("classify-chunk");
        let db = tmp.open();
        db.add_monitored_folder("root", ROOT).unwrap();
        let n = PARAM_CHUNK * 2 + 7;
        for i in 0..n {
            db.insert_track(&Track {
                id: format!("id{i}"),
                path: format!("{ROOT}/PSX/Game {i}/01.mp3"),
                codec: "MP3".into(),
                ..Default::default()
            })
            .unwrap();
        }
        let tracks = db.get_all_tracks().unwrap();
        assert_eq!(tracks.len(), n);
        assert!(
            tracks.iter().all(|t| t.console_id == "ps1"),
            "{} of {n} tracks were not stamped",
            tracks.iter().filter(|t| t.console_id != "ps1").count()
        );
    }
}
