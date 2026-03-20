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
        Ok(Self { conn })
    }

    // --- Tracks ---

    pub fn insert_track(&self, track: &Track) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO tracks (id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, modified_at, has_artwork)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork
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
                    modified_at: 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tracks)
    }

    pub fn get_track_by_path(&self, path: &str) -> Result<Option<Track>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_number, disc_number, duration_ms, sample_rate, channels, bitrate, codec, file_size, has_artwork
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
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_number, t.disc_number, t.duration_ms, t.sample_rate, t.channels, t.bitrate, t.codec, t.file_size, t.has_artwork
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
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_number, t.disc_number, t.duration_ms, t.sample_rate, t.channels, t.bitrate, t.codec, t.file_size, t.has_artwork
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

    pub fn remove_track_by_path(&self, path: &str) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM tracks WHERE path = ?1", params![path])?;
        Ok(())
    }
}
