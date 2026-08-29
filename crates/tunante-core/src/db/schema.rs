pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    album_artist TEXT NOT NULL DEFAULT '',
    track_number INTEGER,
    disc_number INTEGER,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    sample_rate INTEGER,
    channels INTEGER,
    bitrate INTEGER,
    codec TEXT NOT NULL DEFAULT '',
    file_size INTEGER NOT NULL DEFAULT 0,
    modified_at INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    has_artwork INTEGER NOT NULL DEFAULT 0,
    -- The game named by the file's own header, kept apart from `album`.
    --
    -- Console formats carry it as a field of its own: `game=` in a PSF `[TAG]`,
    -- the game name in a VGM's GD3, the game title in an SPC's ID666. Every
    -- reader here used to write it straight into `album`, which destroyed the
    -- distinction — and `album` is not the same thing. A soundtrack release has
    -- an album title that is often not the game's name at all, which is how
    -- "Final Fantasy Tactics A2: The Sealed Grimoire" ended up being searched
    -- for as if it were a game.
    --
    -- Empty for anything whose format has no such field: vgmstream streams,
    -- and ordinary MP3 or FLAC.
    header_game TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_tracks_path ON tracks(path);
CREATE INDEX IF NOT EXISTS idx_tracks_album_artist ON tracks(album_artist, album, disc_number, track_number);

CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
    title, artist, album, album_artist,
    content='tracks',
    content_rowid='rowid'
);

CREATE TABLE IF NOT EXISTS playlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    id TEXT PRIMARY KEY,
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    added_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_playlist_tracks_playlist ON playlist_tracks(playlist_id, position);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE IF NOT EXISTS monitored_folders (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    watching_enabled INTEGER NOT NULL DEFAULT 1,
    last_scanned_at INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_monitored_folders_path ON monitored_folders(path);

CREATE TABLE IF NOT EXISTS pinned_folders (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    added_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_pinned_folders_path ON pinned_folders(path);

-- What the user has corrected about which machine a track came from and which
-- game it belongs to. A `scope` of 'folder' applies to a whole subtree, with
-- the nearest ancestor winning; 'track' applies to one exact path.
--
-- Deliberately has NO foreign key to `tracks`. This is the one table here
-- holding something the user typed rather than something derived from a file,
-- so it has to outlive `clear_all_tracks`, `prune_missing`, an unplugged drive
-- and a full rescan. Losing a franchise folder's flags because an external disk
-- was unmounted at the wrong moment would be worse than any orphaned row.
--
-- Either column may be NULL, meaning "leave that half to the rules": flagging
-- Megaten/Persona 5 as a PS4 game should not also freeze the game name.
CREATE TABLE IF NOT EXISTS classification_overrides (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    target TEXT NOT NULL,
    console_id TEXT,
    game_name TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    UNIQUE(scope, target)
);

CREATE INDEX IF NOT EXISTS idx_class_overrides_scope ON classification_overrides(scope, target);

-- Pure cache. Every row is recomputable from the track's path, its album tag
-- and the overrides above, so it can be dropped at any time — and is, whenever
-- the rules change. See `classifier_version` in `db/classification.rs`.
--
-- The foreign key is what keeps it honest: rows disappear along with their
-- tracks through every delete path there is, without any of those paths having
-- to know this table exists.
CREATE TABLE IF NOT EXISTS track_classification (
    path TEXT PRIMARY KEY REFERENCES tracks(path) ON DELETE CASCADE,
    console_id TEXT NOT NULL DEFAULT '',
    console_source TEXT NOT NULL DEFAULT '',
    game_name TEXT NOT NULL DEFAULT '',
    game_source TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_track_classification_console ON track_classification(console_id);
"#;
