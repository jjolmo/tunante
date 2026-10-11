//! Files dropped on the window from a file manager.
//!
//! Two sources, one message. On X11, Windows and macOS winit reports drops as
//! `WindowEvent::DroppedFile`, one event per file, and main.rs batches those.
//! On Wayland winit 0.30 reports nothing at all, and a data device of our own
//! would hear nothing either: KWin hands a drag only to a client's first
//! `wl_data_device`, which is the one Slint's clipboard creates before any of
//! this code runs. So the drop arrives through that clipboard, patched for it
//! (vendor/smithay-clipboard-patch), as a `text/uri-list` body for
//! [`parse_uri_list`].

use std::path::PathBuf;

pub enum Msg {
    /// A drag carrying files is over the window (true) or left it (false).
    Hover(bool),
    /// Files were dropped.
    Files(Vec<PathBuf>),
}

/// The local paths in a `text/uri-list` body (RFC 2483): one URI per line,
/// `#` lines are comments, and anything that is not a `file:` URI is skipped.
pub fn parse_uri_list(body: &str) -> Vec<PathBuf> {
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let rest = l.strip_prefix("file://")?;
            // `file:///x` and `file://localhost/x`; another host is not ours.
            let path = if rest.starts_with('/') {
                rest
            } else {
                rest.strip_prefix("localhost")?
            };
            Some(PathBuf::from(percent_decode(path)))
        })
        .collect()
}

#[cfg(unix)]
fn percent_decode(s: &str) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    std::ffi::OsString::from_vec(out)
}

#[cfg(not(unix))]
fn percent_decode(s: &str) -> std::ffi::OsString {
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_list_keeps_local_files_and_decodes_them() {
        let body = "# from Dolphin\r\nfile:///home/me/M%C3%BAsica/a%20b.flac\r\n\
                    file://localhost/tmp/x.spc\r\nhttps://example.com/y.mp3\r\n\
                    file://otherhost/z.mp3\r\n\r\n";
        assert_eq!(
            parse_uri_list(body),
            [PathBuf::from("/home/me/Música/a b.flac"), PathBuf::from("/tmp/x.spc")]
        );
    }

    #[test]
    fn a_stray_percent_is_kept_as_is() {
        assert_eq!(parse_uri_list("file:///a/100%/b%2"), [PathBuf::from("/a/100%/b%2")]);
    }
}
