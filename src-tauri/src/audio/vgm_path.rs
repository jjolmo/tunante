use std::path::Path;

/// GME-supported format extensions
const GME_EXTENSIONS: &[&str] = &[
    "nsf", "nsfe", "spc", "gbs", "vgm", "vgz", "hes", "kss", "ay", "sap", "gym",
];

/// Parse a potentially multi-track path into (file_path, sub_track_index).
/// Format: "/path/to/file.nsf#3" → ("/path/to/file.nsf", Some(3))
/// Regular paths return None for the index.
pub fn parse_vgm_path(path: &str) -> (&str, Option<usize>) {
    if let Some(pos) = path.rfind('#') {
        if let Ok(index) = path[pos + 1..].parse::<usize>() {
            return (&path[..pos], Some(index));
        }
    }
    (path, None)
}

/// Build a multi-track virtual path
pub fn build_vgm_path(file_path: &str, track_index: usize) -> String {
    format!("{}#{}", file_path, track_index)
}

/// Check if an extension is a GME-supported format
pub fn is_gme_format(ext: &str) -> bool {
    GME_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file path is a GME-supported format
pub fn is_gme_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_gme_format(e))
        .unwrap_or(false)
}
