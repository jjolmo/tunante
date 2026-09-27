pub(crate) mod gme_reader;
mod gsf_reader;
mod psf_reader;
mod psf2_reader;
pub mod rating_source;
pub mod ratings_sync;
mod reader;
mod twosf_reader;
mod usf_reader;
pub(crate) mod vgmstream_reader;
pub(crate) mod writer;

pub use gme_reader::read_gme_metadata;
pub use gsf_reader::read_gsf_metadata;
pub use psf_reader::read_psf_metadata;
pub use psf2_reader::read_psf2_metadata;
pub use reader::{
    extract_artwork_base64, read_metadata, read_metadata_all, read_metadata_all_with_opts, ScanOpts,
};
pub use twosf_reader::read_twosf_metadata;
pub use usf_reader::read_usf_metadata;
pub use vgmstream_reader::read_vgmstream_metadata;
pub use writer::{real_path_of, write_export_tags, write_rating_to_file, ExportTags};

/// Standard base64, for the `data:` URIs the artwork reader hands out.
pub fn decode_base64(b64: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()
}

/// Built-in fallback play time for tracks with no determinable length, used
/// when the user has not set one in Settings.
pub fn gme_reader_default_duration_ms() -> i64 {
    gme_reader::DEFAULT_DURATION_MS
}
