mod classification;
pub mod models;
mod schema;

pub use classification::{ClassificationOverride, UnclassifiedFolder, CLASSIFIER_VERSION};

use crate::classify::Classifier;
use models::{MonitoredFolder, PinnedFolder, Playlist, Setting, Track};
use rusqlite::{params, Connection};
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Database {
    conn: Connection,
    /// Built on first use from the registered roots and the stored overrides,
    /// and dropped whenever either changes. `RefCell` rather than a lock
    /// because `Connection` is already `!Sync`, so a `Database` is only ever
    /// reachable through a mutex anyway.
    classifier: RefCell<Option<Arc<Classifier>>>,
}

/// Escape a path so it can be used as a literal prefix inside a `LIKE` pattern.
///
/// SQLite's `LIKE` treats `_` as "any one character" and `%` as "anything at
/// all", and neither is special in a filename. Underscores are everywhere in a
/// game-music library — `sky_temple-the-ark`, `boot_hwio` — so this is not a
/// theoretical case.
///
/// Left unescaped it was worse than a wrong search result: removing the tracks
/// under `/m/sky_temple` also removed those under `/m/skyXtemple`, because the
/// `_` matched the `X`. Every query below that builds a pattern out of a path
/// goes through here and pairs it with `ESCAPE '\'`.
fn like_prefix(path: &str) -> String {
    path.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Database {
    pub fn new(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(schema::SCHEMA)?;

        // Migration: add rating column (ignore error if already exists)
        let _ = conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN rating INTEGER NOT NULL DEFAULT 0;",
        );

        // Migration: add position column to playlists for manual ordering.
        //
        // Whether this succeeds is the one durable record of which build got here
        // first: it fails if and only if the column is already there, which means
        // some earlier build already added it — and, back then, already ran the
        // alphabetical seeding below.
        let column_is_new = conn
            .execute_batch("ALTER TABLE playlists ADD COLUMN position INTEGER NOT NULL DEFAULT 0;")
            .is_ok();

        // Seed initial positions alphabetically, for playlists that predate the
        // column. Exactly once, ever.
        //
        // Re-running it corrupts the very order it exists to establish:
        // `create_playlist` legitimately hands position 0 to the first playlist,
        // and a later pass re-seeds that row to its alphabetical rank, colliding
        // with whoever already sits there. Earlier builds ran it on every open,
        // which is why a manual reordering never survived a restart.
        //
        // The flag alone is not enough to stop it. A database written by one of
        // those builds has no flag, so a first open here would run the seeding one
        // final time and then freeze that result — the same corruption, once,
        // made permanent. `column_is_new` is what distinguishes "never seeded"
        // from "seeded by an older build": in the second case the right move is to
        // set the flag and leave the stored order exactly as it is.
        let seeded: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM settings WHERE key = 'playlist_positions_seeded')",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !seeded {
            if column_is_new {
                let _ = conn.execute_batch(
                    "UPDATE playlists SET position = (
                         SELECT COUNT(*) FROM playlists p2 WHERE p2.name < playlists.name
                     ) WHERE position = 0;",
                );
            }
            let _ = conn.execute_batch(
                "INSERT OR REPLACE INTO settings (key, value, updated_at)
                 VALUES ('playlist_positions_seeded', '1', strftime('%s', 'now'));",
            );
        }

        let db = Self { conn, classifier: RefCell::new(None) };
        // Rebuilds the derived console/game table when the rules have changed.
        // Idempotent, and a no-op on every open but the first after an upgrade.
        db.ensure_classified()?;
        Ok(db)
    }

    // --- Tracks ---

    /// Insert a track, upserting on path conflict. Returns the actual stored track ID
    /// (which may differ from track.id if the path already existed).
    pub fn insert_track(&self, track: &Track) -> Result<String, DbError> {
        self.conn.execute(
            "INSERT INTO tracks (id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, modified_at, has_artwork, rating)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(path) DO UPDATE SET
               title = excluded.title,
               artist = excluded.artist,
               album = excluded.album,
               album_artist = excluded.album_artist,
               track_number = excluded.track_number,
               disc_number = excluded.disc_number,
               duration_ms = excluded.duration_ms,
               sample_rate = excluded.sample_rate,
               channels = excluded.channels,
               bitrate = excluded.bitrate,
               codec = excluded.codec,
               file_size = excluded.file_size,
               modified_at = excluded.modified_at,
               has_artwork = excluded.has_artwork,
               rating = CASE WHEN tracks.rating = 0 THEN excluded.rating ELSE tracks.rating END",
            params![
                track.id,
                track.path,
                track.title,
                track.artist,
                track.album,
                track.album_artist,
                track.track_number,
                track.disc_number,
                track.duration_ms,
                track.sample_rate,
                track.channels,
                track.bitrate,
                track.codec,
                track.file_size,
                track.modified_at,
                track.has_artwork,
                track.rating,
            ],
        )?;

        // Get the actual stored ID (may be the existing one on conflict)
        let actual_id: String = self.conn.query_row(
            "SELECT id FROM tracks WHERE path = ?1",
            params![track.path],
            |row| row.get(0),
        )?;

        // Update FTS index
        self.conn.execute(
            "INSERT OR REPLACE INTO tracks_fts (rowid, title, artist, album, album_artist)
             SELECT rowid, title, artist, album, album_artist FROM tracks WHERE id = ?1",
            params![actual_id],
        )?;

        // Keep the derived console/game row in step with the track it describes.
        // Doing it here means the folder watcher stays correct one file at a
        // time, without anything ever having to rebuild the whole table.
        self.classify_path(&track.path, &track.album, &track.codec)?;

        Ok(actual_id)
    }

    pub fn get_all_tracks(&self) -> Result<Vec<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork, rating
             FROM tracks ORDER BY album_artist, album, disc_number, track_number, title",
        )?;

        let mut tracks = stmt
            .query_map([], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut tracks)?;
        Ok(tracks)
    }

    pub fn get_track_by_id(&self, id: &str) -> Result<Option<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork, rating
             FROM tracks WHERE id = ?1",
        )?;

        let mut tracks = stmt
            .query_map(params![id], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut tracks)?;
        Ok(tracks.pop())
    }

    pub fn get_track_by_path(&self, path: &str) -> Result<Option<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork, rating
             FROM tracks WHERE path = ?1",
        )?;

        let mut tracks = stmt
            .query_map(params![path], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut tracks)?;
        Ok(tracks.pop())
    }

    pub fn search_tracks(&self, query: &str) -> Result<Vec<Track>, DbError> {
        let fts_query = query
            .split_whitespace()
            .map(|w| format!("{}*", w))
            .collect::<Vec<_>>()
            .join(" ");

        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_number, t.disc_number, t.duration_ms, t.sample_rate, t.channels, t.bitrate, t.codec, t.file_size, t.has_artwork, t.rating
             FROM tracks t
             JOIN tracks_fts ON tracks_fts.rowid = t.rowid
             WHERE tracks_fts MATCH ?1
             ORDER BY rank",
        )?;

        let mut tracks = stmt
            .query_map(params![fts_query], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut tracks)?;
        Ok(tracks)
    }

    // --- Playlists ---

    pub fn get_playlists(&self) -> Result<Vec<Playlist>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.created_at, p.updated_at,
                    (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) as track_count
             FROM playlists p ORDER BY p.position, p.name",
        )?;

        let playlists = stmt
            .query_map([], |row| {
                Ok(Playlist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    track_count: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(playlists)
    }

    pub fn create_playlist(&self, id: &str, name: &str) -> Result<(), DbError> {
        // Append new playlist at the end (position = max + 1)
        let next_pos: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlists",
            [],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO playlists (id, name, position) VALUES (?1, ?2, ?3)",
            params![id, name, next_pos],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: &str) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn rename_playlist(&self, id: &str, name: &str) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE playlists SET name = ?2, updated_at = strftime('%s', 'now') WHERE id = ?1",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn reorder_playlists(&self, ordered_ids: &[String]) -> Result<(), DbError> {
        for (position, id) in ordered_ids.iter().enumerate() {
            self.conn.execute(
                "UPDATE playlists SET position = ?1 WHERE id = ?2",
                params![position as i64, id],
            )?;
        }
        Ok(())
    }

    pub fn get_playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_number, t.disc_number, t.duration_ms, t.sample_rate, t.channels, t.bitrate, t.codec, t.file_size, t.has_artwork, t.rating
             FROM tracks t
             JOIN playlist_tracks pt ON pt.track_id = t.id
             WHERE pt.playlist_id = ?1
             ORDER BY pt.position",
        )?;

        let mut tracks = stmt
            .query_map(params![playlist_id], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut tracks)?;
        Ok(tracks)
    }

    pub fn add_track_to_playlist(
        &self,
        id: &str,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<(), DbError> {
        // Ignorar si la pista ya esta en la playlist (evita duplicados).
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2)",
            params![playlist_id, track_id],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(());
        }

        let position: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
                params![playlist_id],
                |row| row.get(0),
            )?;

        self.conn.execute(
            "INSERT INTO playlist_tracks (id, playlist_id, track_id, position) VALUES (?1, ?2, ?3, ?4)",
            params![id, playlist_id, track_id, position],
        )?;

        self.conn.execute(
            "UPDATE playlists SET updated_at = strftime('%s', 'now') WHERE id = ?1",
            params![playlist_id],
        )?;

        Ok(())
    }

    pub fn remove_track_from_playlist(
        &self,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
            params![playlist_id, track_id],
        )?;

        self.conn.execute(
            "UPDATE playlists SET updated_at = strftime('%s', 'now') WHERE id = ?1",
            params![playlist_id],
        )?;

        Ok(())
    }

    /// One playlist by id, or `None` if it is gone.
    pub fn get_playlist(&self, id: &str) -> Result<Option<Playlist>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.created_at, p.updated_at,
                    (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) as track_count
             FROM playlists p WHERE p.id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                track_count: row.get(4)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Create a playlist and hand back the id it was given.
    ///
    /// The id is minted in SQL rather than by the caller so that crates without a
    /// uuid dependency — `tunante-mini` — can create playlists too.
    pub fn create_playlist_named(&self, name: &str) -> Result<String, DbError> {
        let id: String = self.conn.query_row(
            "INSERT INTO playlists (id, name, position)
             SELECT lower(hex(randomblob(16))), ?1, COALESCE(MAX(position), -1) + 1
             FROM playlists
             RETURNING id",
            params![name],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Append tracks to a playlist, skipping ones already in it. Returns how many
    /// were actually added.
    ///
    /// One transaction and four statements plus the inserts, where the per-track
    /// `add_track_to_playlist` would be four *committed* transactions each. That
    /// difference is the whole feature on a phone: adding a folder tree is
    /// thousands of tracks, and thousands of WAL fsyncs on eMMC are seconds of a
    /// frozen UI.
    pub fn add_tracks_to_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<usize, DbError> {
        if track_ids.is_empty() {
            return Ok(0);
        }

        let tx = self.conn.unchecked_transaction()?;

        let mut seen: std::collections::HashSet<String> = tx
            .prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1")?
            .query_map(params![playlist_id], |row| row.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;

        let mut position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )?;

        let mut added = 0usize;
        {
            // The `WHERE EXISTS` is load-bearing, not belt-and-braces. `foreign_keys`
            // is ON, and a foreign key violation aborts the statement — `INSERT OR
            // IGNORE` does not swallow it. Without the guard a single stale track id
            // rolls back the entire batch.
            let mut stmt = tx.prepare(
                "INSERT INTO playlist_tracks (id, playlist_id, track_id, position)
                 SELECT lower(hex(randomblob(16))), ?1, ?2, ?3
                 WHERE EXISTS (SELECT 1 FROM tracks WHERE id = ?2)",
            )?;
            for track_id in track_ids {
                // Inserting as we go dedups within the incoming batch too, not just
                // against what the playlist already holds.
                if !seen.insert(track_id.clone()) {
                    continue;
                }
                if stmt.execute(params![playlist_id, track_id, position])? > 0 {
                    position += 1;
                    added += 1;
                }
            }
        }

        tx.execute(
            "UPDATE playlists SET updated_at = strftime('%s', 'now') WHERE id = ?1",
            params![playlist_id],
        )?;

        tx.commit()?;
        Ok(added)
    }

    /// Put a playlist's tracks in exactly this order, renumbering from zero.
    ///
    /// Renumbering densely is safe because `idx_playlist_tracks_playlist` is not a
    /// unique index: a plain sequential pass cannot trip over a position another
    /// row still holds. It also closes the gaps that `remove_track_from_playlist`
    /// leaves behind.
    pub fn reorder_playlist_tracks(
        &self,
        playlist_id: &str,
        ordered_track_ids: &[String],
    ) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE playlist_tracks SET position = ?1
                 WHERE playlist_id = ?2 AND track_id = ?3",
            )?;
            for (position, track_id) in ordered_track_ids.iter().enumerate() {
                stmt.execute(params![position as i64, playlist_id, track_id])?;
            }
        }
        tx.execute(
            "UPDATE playlists SET updated_at = strftime('%s', 'now') WHERE id = ?1",
            params![playlist_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    // --- Settings ---

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value, updated_at)
             VALUES (?1, ?2, strftime('%s', 'now'))",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_all_settings(&self) -> Result<Vec<Setting>, DbError> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let settings = stmt
            .query_map([], |row| {
                Ok(Setting {
                    key: row.get(0)?,
                    value: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(settings)
    }

    // --- Monitored Folders ---

    pub fn get_monitored_folders(&self) -> Result<Vec<MonitoredFolder>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, watching_enabled, last_scanned_at, added_at
             FROM monitored_folders ORDER BY path",
        )?;
        let folders = stmt
            .query_map([], |row| {
                Ok(MonitoredFolder {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    watching_enabled: row.get(2)?,
                    last_scanned_at: row.get(3)?,
                    added_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(folders)
    }

    /// The set of roots is an input to the classifier — the `<root>/<console>/`
    /// rule cannot fire without knowing where a root begins — so registering or
    /// forgetting one invalidates every derived row. Not optional: skipping the
    /// rebuild leaves tracks classified against a root that no longer exists.
    pub fn add_monitored_folder(&self, id: &str, path: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO monitored_folders (id, path) VALUES (?1, ?2)",
            params![id, path],
        )?;
        self.invalidate_classifier();
        self.reclassify_all()?;
        Ok(())
    }

    pub fn remove_monitored_folder(&self, id: &str) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM monitored_folders WHERE id = ?1",
            params![id],
        )?;
        self.invalidate_classifier();
        self.reclassify_all()?;
        Ok(())
    }

    /// Remove all tracks whose path starts with the given folder path.
    /// Also cleans up FTS entries and playlist references for removed tracks.
    pub fn remove_tracks_by_folder_path(&self, folder_path: &str) -> Result<usize, DbError> {
        // Ensure folder path ends with separator for prefix matching
        let prefix = if folder_path.ends_with('/') || folder_path.ends_with('\\') {
            folder_path.to_string()
        } else {
            format!("{}/", folder_path)
        };

        // Remove FTS entries for matching tracks
        self.conn.execute(
            "DELETE FROM tracks_fts WHERE rowid IN (SELECT rowid FROM tracks WHERE path LIKE ?1 ESCAPE '\\' OR path = ?2)",
            params![format!("{}%", like_prefix(&prefix)), folder_path],
        )?;

        // Remove playlist_tracks references for matching tracks
        self.conn.execute(
            "DELETE FROM playlist_tracks WHERE track_id IN (SELECT id FROM tracks WHERE path LIKE ?1 ESCAPE '\\' OR path = ?2)",
            params![format!("{}%", like_prefix(&prefix)), folder_path],
        )?;

        // Remove the tracks themselves
        let deleted = self.conn.execute(
            "DELETE FROM tracks WHERE path LIKE ?1 ESCAPE '\\' OR path = ?2",
            params![format!("{}%", like_prefix(&prefix)), folder_path],
        )?;

        Ok(deleted)
    }

    /// Remove all tracks under `folder_path`, EXCEPT those that also fall under
    /// one of `keep_prefixes` (other monitored folders that still cover them).
    /// This prevents data loss when removing a folder that overlaps another
    /// monitored folder. Also cleans up FTS and playlist references.
    pub fn remove_tracks_by_folder_path_excluding(
        &self,
        folder_path: &str,
        keep_prefixes: &[String],
    ) -> Result<usize, DbError> {
        let prefix = if folder_path.ends_with('/') || folder_path.ends_with('\\') {
            folder_path.to_string()
        } else {
            format!("{}/", folder_path)
        };

        // Build the shared selector predicate and its parameters once.
        // ?1 = "<prefix>%", ?2 = exact folder path, ?3.. = keep prefixes ("<kp>%").
        let mut sel = String::from("(path LIKE ?1 ESCAPE '\\' OR path = ?2)");
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(format!("{}%", like_prefix(&prefix))),
            Box::new(folder_path.to_string()),
        ];
        for kp in keep_prefixes {
            let kp_prefix = if kp.ends_with('/') || kp.ends_with('\\') {
                kp.clone()
            } else {
                format!("{}/", kp)
            };
            // Escaped too, and this one is the dangerous direction: a keep-prefix
            // that fails to match is a folder deleted when it should have been
            // spared.
            params.push(Box::new(format!("{}%", like_prefix(&kp_prefix))));
            sel.push_str(&format!(" AND path NOT LIKE ?{} ESCAPE '\\'", params.len()));
        }

        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        // FTS entries for matching tracks
        self.conn.execute(
            &format!(
                "DELETE FROM tracks_fts WHERE rowid IN (SELECT rowid FROM tracks WHERE {})",
                sel
            ),
            refs.as_slice(),
        )?;

        // Playlist references for matching tracks
        self.conn.execute(
            &format!(
                "DELETE FROM playlist_tracks WHERE track_id IN (SELECT id FROM tracks WHERE {})",
                sel
            ),
            refs.as_slice(),
        )?;

        // The tracks themselves
        let deleted = self.conn.execute(
            &format!("DELETE FROM tracks WHERE {}", sel),
            refs.as_slice(),
        )?;

        Ok(deleted)
    }

    // --- Pinned Folders ---

    pub fn get_pinned_folders(&self) -> Result<Vec<PinnedFolder>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, added_at FROM pinned_folders ORDER BY added_at, path",
        )?;
        let folders = stmt
            .query_map([], |row| {
                Ok(PinnedFolder {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    added_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(folders)
    }

    pub fn add_pinned_folder(&self, id: &str, path: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO pinned_folders (id, path) VALUES (?1, ?2)",
            params![id, path],
        )?;
        Ok(())
    }

    pub fn remove_pinned_folder(&self, id: &str) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM pinned_folders WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn toggle_folder_watching(&self, id: &str, enabled: bool) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE monitored_folders SET watching_enabled = ?2 WHERE id = ?1",
            params![id, enabled as i32],
        )?;
        Ok(())
    }

    pub fn update_folder_scan_time(&self, id: &str) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE monitored_folders SET last_scanned_at = strftime('%s', 'now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn set_track_rating(&self, track_id: &str, rating: i32) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE tracks SET rating = ?2 WHERE id = ?1",
            params![track_id, rating],
        )?;
        Ok(())
    }

    pub fn get_faved_tracks(&self) -> Result<Vec<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork, rating
             FROM tracks WHERE rating > 0
             ORDER BY album_artist, album, disc_number, track_number, title",
        )?;

        let mut tracks = stmt
            .query_map([], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut tracks)?;
        Ok(tracks)
    }

    /// Fetch tracks by their IDs, preserving the input order.
    pub fn get_tracks_by_ids(&self, ids: &[String]) -> Result<Vec<Track>, DbError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Build placeholders: (?1, ?2, ?3, ...)
        let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork, rating
             FROM tracks WHERE id IN ({})",
            placeholders.join(", ")
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();

        let mut found: Vec<Track> = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut found)?;
        let tracks_map: std::collections::HashMap<String, Track> =
            found.into_iter().map(|t| (t.id.clone(), t)).collect();

        // Preserve input order
        Ok(ids.iter().filter_map(|id| tracks_map.get(id).cloned()).collect())
    }

    /// Fetch all tracks whose path starts with the given folder prefix,
    /// ordered by disc/track number (album order).
    pub fn get_tracks_by_folder(&self, folder: &str) -> Result<Vec<Track>, DbError> {
        let prefix = if folder.ends_with('/') {
            folder.to_string()
        } else {
            format!("{}/", folder)
        };
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork, rating
             FROM tracks WHERE path LIKE ?1 ESCAPE '\\'
             ORDER BY disc_number, track_number, title",
        )?;

        let mut tracks = stmt
            .query_map(params![format!("{}%", like_prefix(&prefix))], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    album_artist: row.get(5)?,
                    track_number: row.get(6)?,
                    disc_number: row.get(7)?,
                    duration_ms: row.get(8)?,
                    sample_rate: row.get(9)?,
                    channels: row.get(10)?,
                    bitrate: row.get(11)?,
                    codec: row.get(12)?,
                    file_size: row.get(13)?,
                    has_artwork: row.get(14)?,
                    rating: row.get(15)?,
                    modified_at: 0,
                    ..Default::default()
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        self.stamp(&mut tracks)?;
        Ok(tracks)
    }

    pub fn clear_all_tracks(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            "DELETE FROM tracks; DELETE FROM tracks_fts;"
        )?;
        Ok(())
    }

    pub fn remove_track_by_path(&self, path: &str) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM tracks WHERE path = ?1", params![path])?;
        Ok(())
    }

    pub fn update_track_metadata(
        &self,
        track_id: &str,
        title: Option<&str>,
        artist: Option<&str>,
        album: Option<&str>,
        album_artist: Option<&str>,
        track_number: Option<Option<i32>>,
        disc_number: Option<Option<i32>>,
    ) -> Result<(), DbError> {
        let mut sets = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(v) = title {
            sets.push("title = ?");
            params_vec.push(Box::new(v.to_string()));
        }
        if let Some(v) = artist {
            sets.push("artist = ?");
            params_vec.push(Box::new(v.to_string()));
        }
        if let Some(v) = album {
            sets.push("album = ?");
            params_vec.push(Box::new(v.to_string()));
        }
        if let Some(v) = album_artist {
            sets.push("album_artist = ?");
            params_vec.push(Box::new(v.to_string()));
        }
        if let Some(v) = track_number {
            sets.push("track_number = ?");
            params_vec.push(Box::new(v));
        }
        if let Some(v) = disc_number {
            sets.push("disc_number = ?");
            params_vec.push(Box::new(v));
        }

        if sets.is_empty() {
            return Ok(());
        }

        // Number placeholders
        let mut sql = String::from("UPDATE tracks SET ");
        for (i, set) in sets.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&set.replace('?', &format!("?{}", i + 1)));
        }
        sql.push_str(&format!(" WHERE id = ?{}", params_vec.len() + 1));
        params_vec.push(Box::new(track_id.to_string()));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, params_refs.as_slice())?;

        // Update FTS index
        self.conn.execute(
            "INSERT OR REPLACE INTO tracks_fts (rowid, title, artist, album, album_artist)
             SELECT rowid, title, artist, album, album_artist FROM tracks WHERE id = ?1",
            params![track_id],
        )?;

        Ok(())
    }

    /// Remove all tracks whose path matches the given base path or has #N suffix.
    /// This handles both single-track files and multi-track VGM files.
    pub fn remove_tracks_by_base_path(&self, base_path: &str) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM tracks WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            params![base_path, format!("{}#%", like_prefix(base_path))],
        )?;
        Ok(())
    }

    /// Every track path stored under `folder_path`, including `#N` subtune paths.
    pub fn get_track_paths_under(&self, folder_path: &str) -> Result<Vec<String>, DbError> {
        let prefix = if folder_path.ends_with('/') {
            folder_path.to_string()
        } else {
            format!("{}/", folder_path)
        };

        let mut stmt = self
            .conn
            .prepare("SELECT path FROM tracks WHERE path LIKE ?1 ESCAPE '\\'")?;
        let paths = stmt
            .query_map(params![format!("{}%", like_prefix(&prefix))], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(paths)
    }

    /// Remove the tracks stored at exactly these paths.
    ///
    /// `tracks_fts` is an external-content FTS5 index, so its rows have to be
    /// deleted explicitly and *before* the tracks themselves — otherwise the
    /// index keeps pointing at rowids that have been reused, and searches start
    /// returning unrelated tracks.
    pub fn remove_tracks_by_paths(&self, paths: &[String]) -> Result<usize, DbError> {
        let mut deleted = 0usize;

        for chunk in paths.chunks(400) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(",");

            self.conn.execute(
                &format!(
                    "DELETE FROM tracks_fts WHERE rowid IN \
                     (SELECT rowid FROM tracks WHERE path IN ({}))",
                    placeholders
                ),
                rusqlite::params_from_iter(chunk.iter()),
            )?;

            self.conn.execute(
                &format!(
                    "DELETE FROM playlist_tracks WHERE track_id IN \
                     (SELECT id FROM tracks WHERE path IN ({}))",
                    placeholders
                ),
                rusqlite::params_from_iter(chunk.iter()),
            )?;

            deleted += self.conn.execute(
                &format!("DELETE FROM tracks WHERE path IN ({})", placeholders),
                rusqlite::params_from_iter(chunk.iter()),
            )?;
        }

        Ok(deleted)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A database on disk, deleted when the guard drops.
    ///
    /// On disk and not `:memory:` because the thing most worth testing here is
    /// what a *reopen* does, and an in-memory database is a fresh one every time.
    pub(crate) struct TempDb(std::path::PathBuf);

    impl TempDb {
        pub(crate) fn new(tag: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let mut p = std::env::temp_dir();
            p.push(format!(
                "tunante-test-{}-{}-{}.db",
                tag,
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_file(&p);
            Self(p)
        }

        pub(crate) fn open(&self) -> Database {
            Database::new(&self.0).expect("open")
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            for suffix in ["-wal", "-shm"] {
                let mut p = self.0.clone();
                p.set_file_name(format!(
                    "{}{}",
                    self.0.file_name().unwrap().to_string_lossy(),
                    suffix
                ));
                let _ = std::fs::remove_file(&p);
            }
        }
    }

    /// SQLite's LIKE treats `_` as "any one character", and folder names are
    /// full of underscores — this library has a `sky_temple-the-ark` in it.
    ///
    /// Reading one folder and getting another's tracks is a display bug. The
    /// next test is the same flaw where it deletes.
    #[test]
    fn an_underscore_in_a_folder_name_is_not_a_wildcard() {
        let tmp = TempDb::new("like-read");
        let db = tmp.open();
        db.insert_track(&track("1", "/m/sky_temple/a.mp3")).unwrap();
        db.insert_track(&track("2", "/m/skyXtemple/b.mp3")).unwrap();

        assert_eq!(
            db.get_track_paths_under("/m/sky_temple").unwrap(),
            ["/m/sky_temple/a.mp3"],
            "`_` matched the X and dragged in a folder nobody asked about"
        );
        assert_eq!(db.get_tracks_by_folder("/m/sky_temple").unwrap().len(), 1);
    }

    /// The same flaw, but this one deletes.
    #[test]
    fn removing_a_folder_leaves_its_wildcard_lookalikes_alone() {
        let tmp = TempDb::new("like-delete");
        let db = tmp.open();
        db.insert_track(&track("1", "/m/sky_temple/a.mp3")).unwrap();
        db.insert_track(&track("2", "/m/skyXtemple/b.mp3")).unwrap();

        db.remove_tracks_by_folder_path("/m/sky_temple").unwrap();

        let left: Vec<String> =
            db.get_all_tracks().unwrap().into_iter().map(|t| t.path).collect();
        assert_eq!(
            left,
            ["/m/skyXtemple/b.mp3"],
            "deleting one folder took another one with it"
        );
    }

    /// `%` is rarer in a path than `_` but means "anything at all", so it is the
    /// one that could empty a library from one folder.
    #[test]
    fn a_percent_in_a_folder_name_does_not_match_everything() {
        let tmp = TempDb::new("like-percent");
        let db = tmp.open();
        db.insert_track(&track("1", "/m/100%/a.mp3")).unwrap();
        db.insert_track(&track("2", "/m/other/b.mp3")).unwrap();

        assert_eq!(
            db.get_track_paths_under("/m/100%").unwrap(),
            ["/m/100%/a.mp3"],
        );
    }

    fn track(id: &str, path: &str) -> Track {
        Track {
            id: id.into(),
            path: path.into(),
            title: path.into(),
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
        }
    }

    fn positions(db: &Database, playlist_id: &str) -> Vec<i64> {
        db.conn
            .prepare("SELECT position FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position")
            .unwrap()
            .query_map(params![playlist_id], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// The regression that motivated the one-shot flag: the alphabetical seeding
    /// used to run on every open, and would drag whichever playlist held position
    /// 0 back to its alphabetical rank.
    #[test]
    fn playlist_order_survives_reopen() {
        let tmp = TempDb::new("order");

        {
            let db = tmp.open();
            for name in ["Zeta", "Alfa", "Beta"] {
                db.create_playlist_named(name).unwrap();
            }
        }

        let expected = vec!["Zeta", "Alfa", "Beta"];
        for pass in 0..3 {
            let db = tmp.open();
            let got: Vec<String> = db
                .get_playlists()
                .unwrap()
                .into_iter()
                .map(|p| p.name)
                .collect();
            assert_eq!(got, expected, "creation order lost on reopen {pass}");
        }
    }

    #[test]
    fn manual_reorder_survives_reopen() {
        let tmp = TempDb::new("reorder-pl");

        // Names chosen so the manual order is the exact reverse of the alphabetical
        // one. Pick them carelessly and the broken re-seed happens to land on the
        // right answer anyway, and the test passes with the bug still in place.
        let ids: Vec<String> = {
            let db = tmp.open();
            ["Alfa", "Beta", "Gamma"]
                .iter()
                .map(|n| db.create_playlist_named(n).unwrap())
                .collect()
        };

        {
            // Reversing parks "Gamma" on position 0 — exactly the row the old
            // seeding pass would have dragged back to its alphabetical rank.
            let db = tmp.open();
            db.reorder_playlists(&[ids[2].clone(), ids[1].clone(), ids[0].clone()])
                .unwrap();
        }

        let db = tmp.open();
        let got: Vec<String> = db
            .get_playlists()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(got, vec!["Gamma", "Beta", "Alfa"]);
    }

    /// Opening a database written by an older build must not reorder it.
    ///
    /// The one path that actually runs the alphabetical seeding, and the one the
    /// other two tests cannot reach: `TempDb` always starts from a deleted file,
    /// so its first open writes the flag while `playlists` is still empty and
    /// every later open takes the already-seeded branch. Only an upgrade arrives
    /// with playlists present and no flag.
    #[test]
    fn upgrading_an_older_database_leaves_the_order_alone() {
        let tmp = TempDb::new("upgrade");

        let ids: Vec<String> = {
            let db = tmp.open();
            ["Alfa", "Beta", "Gamma"]
                .iter()
                .map(|n| db.create_playlist_named(n).unwrap())
                .collect()
        };

        {
            let db = tmp.open();
            db.reorder_playlists(&[ids[2].clone(), ids[1].clone(), ids[0].clone()])
                .unwrap();
            // Back-date the database to what an older build would have left: the
            // `position` column present, and nothing recording that it was seeded.
            db.conn
                .execute(
                    "DELETE FROM settings WHERE key = 'playlist_positions_seeded'",
                    [],
                )
                .unwrap();
        }

        let db = tmp.open();
        let got: Vec<String> = db
            .get_playlists()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(got, vec!["Gamma", "Beta", "Alfa"]);
    }

    #[test]
    fn bulk_add_dedups_and_packs_positions() {
        let tmp = TempDb::new("bulk");
        let db = tmp.open();

        let ids: Vec<String> = (0..500)
            .map(|i| db.insert_track(&track(&format!("t{i}"), &format!("/m/{i}.flac"))).unwrap())
            .collect();

        let pl = db.create_playlist_named("Lote").unwrap();

        assert_eq!(db.add_tracks_to_playlist(&pl, &ids).unwrap(), 500);
        // Second pass adds nothing: every track is already in.
        assert_eq!(db.add_tracks_to_playlist(&pl, &ids).unwrap(), 0);
        // Duplicates inside one batch collapse too.
        let mut doubled = ids.clone();
        doubled.extend(ids.clone());
        assert_eq!(db.add_tracks_to_playlist(&pl, &doubled).unwrap(), 0);

        assert_eq!(db.get_playlist_tracks(&pl).unwrap().len(), 500);
        assert_eq!(positions(&db, &pl), (0..500).collect::<Vec<i64>>());
        assert_eq!(db.get_playlist(&pl).unwrap().unwrap().track_count, 500);
    }

    /// The batch must land exactly where the old per-track loop did.
    ///
    /// The desktop swapped `add_track_to_playlist` in a loop for one call to
    /// `add_tracks_to_playlist`, and the two only differ in how they report a
    /// bad id. Everything a user can see — which tracks are in, in what order,
    /// at which positions — has to match, so this builds two playlists the two
    /// ways and compares them.
    #[test]
    fn batch_add_matches_the_per_track_loop() {
        let tmp = TempDb::new("equiv");
        let db = tmp.open();

        let ids: Vec<String> = (0..40)
            .map(|i| db.insert_track(&track(&format!("t{i}"), &format!("/m/{i}.flac"))).unwrap())
            .collect();

        let uno_a_uno = db.create_playlist_named("bucle").unwrap();
        let en_lote = db.create_playlist_named("lote").unwrap();

        // El orden de entrada no es el alfabético ni el de inserción, y trae
        // repetidos: si el lote reordenase o duplicase, aquí se ve.
        let entrada: Vec<String> = [7usize, 3, 39, 3, 0, 21, 7, 12]
            .iter()
            .map(|i| ids[*i].clone())
            .collect();

        for (n, id) in entrada.iter().enumerate() {
            db.add_track_to_playlist(&format!("entry{n}"), &uno_a_uno, id)
                .unwrap();
        }
        db.add_tracks_to_playlist(&en_lote, &entrada).unwrap();

        let leer = |pl: &str| -> Vec<String> {
            db.get_playlist_tracks(pl)
                .unwrap()
                .into_iter()
                .map(|t| t.id)
                .collect()
        };

        assert_eq!(leer(&uno_a_uno), leer(&en_lote));
        assert_eq!(positions(&db, &uno_a_uno), positions(&db, &en_lote));
        assert_eq!(leer(&en_lote).len(), 6, "los repetidos deben colapsar");
    }

    /// A track id with no row in `tracks` must be skipped, not abort the batch.
    #[test]
    fn bulk_add_skips_unknown_track_ids() {
        let tmp = TempDb::new("stale");
        let db = tmp.open();

        let real = db.insert_track(&track("real", "/m/real.flac")).unwrap();
        let pl = db.create_playlist_named("Mixta").unwrap();

        let added = db
            .add_tracks_to_playlist(&pl, &["fantasma".to_string(), real.clone()])
            .unwrap();

        assert_eq!(added, 1);
        let tracks = db.get_playlist_tracks(&pl).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, real);
    }

    #[test]
    fn reorder_tracks_round_trips_and_closes_gaps() {
        let tmp = TempDb::new("reorder-tr");
        let db = tmp.open();

        let ids: Vec<String> = (0..5)
            .map(|i| db.insert_track(&track(&format!("t{i}"), &format!("/m/{i}.flac"))).unwrap())
            .collect();
        let pl = db.create_playlist_named("Orden").unwrap();
        db.add_tracks_to_playlist(&pl, &ids).unwrap();

        // Removing from the middle leaves a hole at position 2.
        db.remove_track_from_playlist(&pl, &ids[2]).unwrap();
        assert_eq!(positions(&db, &pl), vec![0, 1, 3, 4]);

        // Reversing renumbers densely and the order reads back as written.
        let reversed: Vec<String> = vec![
            ids[4].clone(),
            ids[3].clone(),
            ids[1].clone(),
            ids[0].clone(),
        ];
        db.reorder_playlist_tracks(&pl, &reversed).unwrap();

        assert_eq!(positions(&db, &pl), vec![0, 1, 2, 3]);
        let got: Vec<String> = db
            .get_playlist_tracks(&pl)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(got, reversed);
    }
}
