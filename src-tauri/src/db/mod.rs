pub mod models;
mod schema;

use models::{MonitoredFolder, Playlist, Setting, Track};
use rusqlite::{params, Connection};
use std::path::Path;
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

        Ok(Self { conn })
    }

    // --- Tracks ---

    pub fn insert_track(&self, track: &Track) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO tracks (id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, modified_at, has_artwork, rating)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(path) DO UPDATE SET
               id = excluded.id,
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

        // Update FTS index
        self.conn.execute(
            "INSERT OR REPLACE INTO tracks_fts (rowid, title, artist, album, album_artist)
             SELECT rowid, title, artist, album, album_artist FROM tracks WHERE id = ?1",
            params![track.id],
        )?;

        Ok(())
    }

    pub fn get_all_tracks(&self) -> Result<Vec<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork, rating
             FROM tracks ORDER BY album_artist, album, disc_number, track_number, title",
        )?;

        let tracks = stmt
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

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

        let tracks = stmt
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tracks)
    }

    // --- Playlists ---

    pub fn get_playlists(&self) -> Result<Vec<Playlist>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.created_at, p.updated_at,
                    (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) as track_count
             FROM playlists p ORDER BY p.name",
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
        self.conn.execute(
            "INSERT INTO playlists (id, name) VALUES (?1, ?2)",
            params![id, name],
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

    pub fn get_playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_number, t.disc_number, t.duration_ms, t.sample_rate, t.channels, t.bitrate, t.codec, t.file_size, t.has_artwork, t.rating
             FROM tracks t
             JOIN playlist_tracks pt ON pt.track_id = t.id
             WHERE pt.playlist_id = ?1
             ORDER BY pt.position",
        )?;

        let tracks = stmt
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tracks)
    }

    pub fn add_track_to_playlist(
        &self,
        id: &str,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<(), DbError> {
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

    pub fn add_monitored_folder(&self, id: &str, path: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO monitored_folders (id, path) VALUES (?1, ?2)",
            params![id, path],
        )?;
        Ok(())
    }

    pub fn remove_monitored_folder(&self, id: &str) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM monitored_folders WHERE id = ?1",
            params![id],
        )?;
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

        let tracks = stmt
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

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

        let tracks_map: std::collections::HashMap<String, Track> = stmt
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();

        // Preserve input order
        Ok(ids.iter().filter_map(|id| tracks_map.get(id).cloned()).collect())
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
            "DELETE FROM tracks WHERE path = ?1 OR path LIKE ?2",
            params![base_path, format!("{}#%", base_path)],
        )?;
        Ok(())
    }
}
