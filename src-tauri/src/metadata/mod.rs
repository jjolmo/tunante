mod gme_reader;
mod reader;
mod vgmstream_reader;

pub use gme_reader::read_gme_metadata;
pub use reader::{extract_artwork_base64, read_metadata, read_metadata_all};
pub use vgmstream_reader::read_vgmstream_metadata;
