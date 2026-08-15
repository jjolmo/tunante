//! Where ratings are read from and written to, and in what order.
//!
//! Three destinations, chosen by the user in Settings:
//!
//! | Clave    | Dónde vive                                             |
//! |----------|--------------------------------------------------------|
//! | `file`   | tags del propio fichero de audio                        |
//! | `folder` | `_ratings.m3u` en la carpeta de la canción              |
//! | `db`     | la base de datos SQLite de la app                       |
//!
//! The same ordered list drives both directions:
//!
//! - **Reading**: the first destination that yields a non-zero rating wins.
//! - **Writing**: the first destination that *can* store it wins; if it can't
//!   (a NSF has no writable tag area, for instance) we fall back to the next.
//!
//! The DB is always written too, regardless of order — it is the index the UI
//! sorts and filters on, and keeping it in sync is what makes listing fast.
//! What the order decides is which destination is *authoritative*.

use std::fmt;
use std::path::Path;

use crate::metadata::vgmstream_reader::parse_folder_m3u_ratings;
use crate::metadata::writer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatingSource {
    File,
    Folder,
    Db,
}

impl RatingSource {
    pub fn as_key(&self) -> &'static str {
        match self {
            RatingSource::File => "file",
            RatingSource::Folder => "folder",
            RatingSource::Db => "db",
        }
    }

    fn from_key(s: &str) -> Option<Self> {
        match s.trim() {
            "file" => Some(RatingSource::File),
            "folder" => Some(RatingSource::Folder),
            "db" => Some(RatingSource::Db),
            _ => None,
        }
    }
}

impl fmt::Display for RatingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

/// Order used when the user has never configured one: the database first, which
/// is the app's own storage and always works. Requested default.
pub const DEFAULT_ORDER: [RatingSource; 3] =
    [RatingSource::Db, RatingSource::File, RatingSource::Folder];

pub const SETTING_KEY: &str = "rating_source_priority";

/// Parse a stored `"db,file,folder"` string into an order.
///
/// Unknown or duplicated entries are dropped, and any destination the user's
/// stored value is missing gets appended in default order — so the result is
/// always all three, exactly once. A corrupt setting degrades to the default
/// instead of losing a destination.
pub fn parse_order(raw: Option<&str>) -> Vec<RatingSource> {
    let mut out: Vec<RatingSource> = Vec::with_capacity(3);
    if let Some(raw) = raw {
        for part in raw.split(',') {
            if let Some(src) = RatingSource::from_key(part) {
                if !out.contains(&src) {
                    out.push(src);
                }
            }
        }
    }
    for src in DEFAULT_ORDER {
        if !out.contains(&src) {
            out.push(src);
        }
    }
    out
}

/// Serialize an order back to the `"db,file,folder"` form stored in settings.
pub fn serialize_order(order: &[RatingSource]) -> String {
    order
        .iter()
        .map(|s| s.as_key())
        .collect::<Vec<_>>()
        .join(",")
}

/// Read the rating stored in the audio file's own tags, if any.
///
/// ⚠️ This opens and parses the file, so it is far more expensive than the other
/// two destinations. It is only reached when `file` sits ahead of whichever
/// destination would otherwise answer, which is why the default order (db
/// first) never pays this cost.
fn read_embedded(path_str: &str) -> Option<i32> {
    if !writer::supports_embedded_rating(path_str) {
        return None;
    }
    let real = writer::real_path_of(path_str);
    crate::metadata::read_metadata(Path::new(real))
        .ok()
        .map(|t| t.rating)
        .filter(|r| *r > 0)
}

/// Read the rating for this track from the folder's `_ratings.m3u`.
fn read_folder(path_str: &str) -> Option<i32> {
    let real = writer::real_path_of(path_str);
    let path = Path::new(real);
    let folder = path.parent()?;
    let filename = path.file_name()?.to_str()?;
    let m3u = folder.join("_ratings.m3u");
    if !m3u.exists() {
        return None;
    }
    let ratings = parse_folder_m3u_ratings(&m3u, filename);
    ratings
        .get(&writer::m3u_track_number(path_str))
        .copied()
        .filter(|r| *r > 0)
}

/// Resolve a track's rating following the user's priority order.
///
/// `db_rating` is what the database holds. Returns the first non-zero value
/// found; 0 when no destination has one.
pub fn resolve_rating(path_str: &str, db_rating: i32, order: &[RatingSource]) -> i32 {
    for src in order {
        let found = match src {
            RatingSource::Db => Some(db_rating).filter(|r| *r > 0),
            RatingSource::Folder => read_folder(path_str),
            RatingSource::File => read_embedded(path_str),
        };
        if let Some(r) = found {
            return r;
        }
    }
    0
}

/// Where a write ended up.
pub struct WriteOutcome {
    /// Destination that actually stored the rating, if any beyond the DB.
    pub stored_in: Option<RatingSource>,
    /// Destinations that were tried and could not take it.
    pub skipped: Vec<RatingSource>,
}

/// Persist a rating following the user's priority order, falling back when a
/// destination cannot hold it.
///
/// The DB is handled by the caller (it is always updated); this deals with the
/// two on-disk destinations. If the order puts `db` first, nothing is written to
/// disk at all — which is the point of choosing it.
pub fn write_rating(path_str: &str, rating: i32, order: &[RatingSource]) -> WriteOutcome {
    let mut skipped = Vec::new();

    for src in order {
        match src {
            // The DB is authoritative and always written by the caller, so
            // reaching it here means "don't touch the disk".
            RatingSource::Db => {
                return WriteOutcome {
                    stored_in: Some(RatingSource::Db),
                    skipped,
                }
            }
            RatingSource::File => match writer::write_embedded_rating(path_str, rating) {
                Ok(true) => {
                    return WriteOutcome {
                        stored_in: Some(RatingSource::File),
                        skipped,
                    }
                }
                // Format has no writable tag area — try the next destination.
                Ok(false) => skipped.push(RatingSource::File),
                Err(e) => {
                    log::warn!("No se pudo escribir el rating en los tags de {path_str}: {e}");
                    skipped.push(RatingSource::File);
                }
            },
            RatingSource::Folder => match writer::write_folder_rating(path_str, rating) {
                Ok(_) => {
                    return WriteOutcome {
                        stored_in: Some(RatingSource::Folder),
                        skipped,
                    }
                }
                Err(e) => {
                    log::warn!("No se pudo escribir el rating en _ratings.m3u de {path_str}: {e}");
                    skipped.push(RatingSource::Folder);
                }
            },
        }
    }

    WriteOutcome {
        stored_in: None,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_when_unset() {
        assert_eq!(parse_order(None), DEFAULT_ORDER.to_vec());
    }

    #[test]
    fn respects_user_order() {
        assert_eq!(
            parse_order(Some("file,folder,db")),
            vec![RatingSource::File, RatingSource::Folder, RatingSource::Db]
        );
    }

    #[test]
    fn completes_partial_and_drops_junk() {
        // Solo una entrada válida: el resto se completa en orden por defecto.
        assert_eq!(
            parse_order(Some("folder,nonsense,folder")),
            vec![RatingSource::Folder, RatingSource::Db, RatingSource::File]
        );
    }

    #[test]
    fn round_trips() {
        let order = parse_order(Some("file,db,folder"));
        assert_eq!(serialize_order(&order), "file,db,folder");
    }
}
