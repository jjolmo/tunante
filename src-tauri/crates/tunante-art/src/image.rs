//! Deciding whether some bytes off the network are a cover, before they are
//! allowed anywhere near the user's library.
//!
//! The check this replaces was `bytes.len() > 100`. That accepts an HTML error
//! page, a redirect stub, a 40 MB Wikimedia scan and a 16×16 favicon, and three
//! separate places in the old cover pipeline relied on it.

use crate::ArtError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    WebP,
}

impl Format {
    pub fn mime(self) -> &'static str {
        match self {
            Format::Jpeg => "image/jpeg",
            Format::Png => "image/png",
            Format::WebP => "image/webp",
        }
    }

    /// The extension to write this as. Honest about PNG rather than calling
    /// everything `.jpg`: the Libretro archive serves PNG, and every reader in
    /// this project sniffs the bytes anyway, so lying in the filename buys
    /// nothing and misleads anyone who looks in the folder.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::WebP => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ImageInfo {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

/// Smaller than this on either side and it is a favicon or a spacer, not art.
pub const MIN_SIDE: u32 = 128;
/// Larger than this and it is a museum scan that has no business being synced
/// to a phone.
pub const MAX_SIDE: u32 = 6000;
/// Hard ceiling on a downloaded body. Enforced while reading, not after.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
const MIN_ASPECT: f64 = 0.4;
const MAX_ASPECT: f64 = 2.5;

fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(Format::Png)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::WebP)
    } else {
        None
    }
}

/// Is this a usable cover?
pub fn inspect(bytes: &[u8]) -> Result<ImageInfo, ArtError> {
    if bytes.len() > MAX_BYTES {
        return Err(ArtError::Rejected(format!("{} bytes is too large", bytes.len())));
    }
    // An HTML error page served with a 200 is the single most common thing that
    // is not an image, and it is what the old length check waved through.
    if bytes.starts_with(b"<") || bytes.starts_with(b"\n<") {
        return Err(ArtError::NotAnImage("looks like markup, not an image".into()));
    }
    let Some(format) = sniff(bytes) else {
        return Err(ArtError::NotAnImage(format!(
            "unrecognised magic bytes {:02x?}",
            &bytes[..bytes.len().min(8)]
        )));
    };
    let size = imagesize::blob_size(bytes)
        .map_err(|e| ArtError::NotAnImage(format!("no readable dimensions: {e}")))?;
    let (width, height) = (size.width as u32, size.height as u32);

    if width < MIN_SIDE || height < MIN_SIDE {
        return Err(ArtError::Rejected(format!("{width}x{height} is too small")));
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(ArtError::Rejected(format!("{width}x{height} is too large")));
    }
    let aspect = width as f64 / height as f64;
    if !(MIN_ASPECT..=MAX_ASPECT).contains(&aspect) {
        return Err(ArtError::Rejected(format!("{width}x{height} is the wrong shape")));
    }
    Ok(ImageInfo { format, width, height, bytes: bytes.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1x1 PNG, then grown to a plausible size by lying in the header — the
    /// dimension check reads the header, so that is the honest way to test it.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(&[0, 0, 0, 13]);
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, 6, 0, 0, 0]);
        v.extend_from_slice(&[0, 0, 0, 0]); // CRC, unchecked by a header reader
        v
    }

    #[test]
    fn a_real_cover_passes() {
        let info = inspect(&png(600, 600)).unwrap();
        assert_eq!(info.format, Format::Png);
        assert_eq!((info.width, info.height), (600, 600));
    }

    /// The case the old `len() > 100` check waved straight through into the
    /// user's music folder.
    #[test]
    fn an_html_error_page_is_not_a_cover() {
        let page = b"<!DOCTYPE html><html><body>404 Not Found</body></html>".repeat(10);
        assert!(matches!(inspect(&page), Err(ArtError::NotAnImage(_))));
    }

    #[test]
    fn a_favicon_is_not_a_cover() {
        assert!(matches!(inspect(&png(16, 16)), Err(ArtError::Rejected(_))));
        assert!(matches!(inspect(&png(127, 127)), Err(ArtError::Rejected(_))));
    }

    /// A Commons original can be an enormous scan, and it must not land in a
    /// folder that syncs to a phone.
    #[test]
    fn a_museum_scan_is_not_a_cover() {
        assert!(matches!(inspect(&png(8000, 8000)), Err(ArtError::Rejected(_))));
    }

    /// Wikipedia's page image is often a wide screenshot strip rather than box art.
    #[test]
    fn something_the_wrong_shape_is_not_a_cover() {
        assert!(matches!(inspect(&png(2000, 300)), Err(ArtError::Rejected(_))));
        assert!(matches!(inspect(&png(300, 2000)), Err(ArtError::Rejected(_))));
        // Box art is not always square, though, and near-square must pass.
        assert!(inspect(&png(600, 840)).is_ok());
    }

    #[test]
    fn unrecognised_bytes_are_not_a_cover() {
        assert!(matches!(inspect(b"GIF89a...........").as_ref(), Err(ArtError::NotAnImage(_))));
        assert!(matches!(inspect(&[0u8; 64]), Err(ArtError::NotAnImage(_))));
        assert!(inspect(b"").is_err());
    }

    /// Libretro serves PNG. Writing those bytes to `cover.jpg` was the old
    /// behaviour and it made the filename a lie.
    #[test]
    fn the_extension_follows_the_bytes() {
        assert_eq!(Format::Png.extension(), "png");
        assert_eq!(Format::Jpeg.extension(), "jpg");
    }
}
