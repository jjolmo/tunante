#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub id: String,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub duration_ms: i64,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub bitrate: Option<i32>,
    pub codec: String,
    pub file_size: i64,
    pub has_artwork: bool,
    pub rating: i32,
    /// Not sent to a UI, which has no use for it. The `default` matters as much
    /// as the `skip_serializing`: without it a Track that went out over a wire —
    /// as it does between tunante-mini and its decoder helper — could not be
    /// read back, because the field would be missing rather than absent.
    #[serde(skip_serializing, default)]
    pub modified_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub track_count: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonitoredFolder {
    pub id: String,
    pub path: String,
    pub watching_enabled: bool,
    pub last_scanned_at: i64,
    pub added_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PinnedFolder {
    pub id: String,
    pub path: String,
    pub added_at: i64,
}
