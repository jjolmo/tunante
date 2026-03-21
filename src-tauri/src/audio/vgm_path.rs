use std::path::Path;

/// GME-supported format extensions
const GME_EXTENSIONS: &[&str] = &[
    "nsf", "nsfe", "spc", "gbs", "vgm", "vgz", "hes", "kss", "ay", "sap", "gym",
];

/// GSF (GBA Sound Format) extensions
const GSF_EXTENSIONS: &[&str] = &["gsf", "minigsf"];

/// 2SF (NDS Sound Format) extensions
const TWOSF_EXTENSIONS: &[&str] = &["2sf", "mini2sf"];

/// PSF (PlayStation Sound Format) extensions
const PSF_EXTENSIONS: &[&str] = &["psf", "minipsf"];

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

/// Check if an extension is a GSF format (GBA Sound Format)
pub fn is_gsf_format(ext: &str) -> bool {
    GSF_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file path is a GSF format
pub fn is_gsf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_gsf_format(e))
        .unwrap_or(false)
}

/// Check if an extension is a 2SF format (NDS Sound Format)
pub fn is_twosf_format(ext: &str) -> bool {
    TWOSF_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file path is a 2SF format
pub fn is_twosf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_twosf_format(e))
        .unwrap_or(false)
}

/// Check if an extension is a PSF format (PlayStation Sound Format)
pub fn is_psf_format(ext: &str) -> bool {
    PSF_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file path is a PSF format
pub fn is_psf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_psf_format(e))
        .unwrap_or(false)
}
