use std::path::Path;

/// GME-supported format extensions
const GME_EXTENSIONS: &[&str] = &[
    "nsf", "nsfe", "spc", "gbs", "vgm", "vgz", "hes", "kss", "ay", "sap", "gym",
];

/// GSF (GBA Sound Format) extensions
const GSF_EXTENSIONS: &[&str] = &["gsf", "minigsf"];

/// 2SF (NDS Sound Format) extensions
const TWOSF_EXTENSIONS: &[&str] = &["2sf", "mini2sf"];

/// PSF (PlayStation Sound Format) extensions — PS1 only
const PSF_EXTENSIONS: &[&str] = &["psf", "minipsf"];

/// PSF2 (PlayStation 2 Sound Format) extensions
const PSF2_EXTENSIONS: &[&str] = &["psf2", "minipsf2"];

/// USF (N64 Sound Format) extensions
const USF_EXTENSIONS: &[&str] = &["usf", "miniusf"];

/// Standard audio formats symphonia handles well.
///
/// These are routed straight to symphonia and never through vgmstream, which may
/// mishandle them.
const STANDARD_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "wav", "aac", "aiff", "wma", "m4a", "ape", "wv",
];

/// Every extension the library scanner will pick up.
///
/// This is the static list. It is not the whole truth: vgmstream carries its own,
/// much broader list built into the library, and a scanner should consult that
/// too for anything this misses — see `tunante_codec::vgmstream_accepts`. Kept
/// here, in the core, because both the desktop app and tunante scan folders.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    // Standard audio
    "mp3", "flac", "ogg", "wav", "aac", "aiff", "wma", "m4a", "opus", "ape", "wv",
    // GME chiptune
    "nsf", "nsfe", "spc", "gbs", "vgm", "vgz", "hes", "kss", "ay", "sap", "gym",
    // vgmstream (Nintendo, common game audio)
    "bcstm", "bfstm", "brstm", "bcwav", "bfwav", "brwav",
    "adx", "hca", "aax", "scd", "at3", "at9",
    "dsp", "idsp", "bfsar", "bars", "strm", "csmp", "cstm",
    "fsb", "bnk", "wem", "mus",
    "xma", "xma2", "xwb",
    "genh", "txth", "txtp",
    "nub", "nus3bank", "lopus",
    "rwsd", "rwar", "rwav",
    "sad", "sgd", "sab",
    "acb", "awb",
    "ktss", "kvs",
    "ast", "xa", "svag", "ras", "sts",
    // PSF family (GBA, NDS, PS1, PS2, N64, Saturn, Dreamcast)
    "gsf", "minigsf",
    "2sf", "mini2sf",
    "psf", "minipsf",
    "psf2", "minipsf2",
    "usf", "miniusf",
    "ssf", "minissf",
    "dsf", "minidsf",
    "qsf", "miniqsf",
    "ncsf", "minincsf",
];

/// Whether this extension is in the static scanner list.
pub fn is_scannable_extension(ext: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Whether the scanner should pick this file up, by extension alone.
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(is_scannable_extension)
        .unwrap_or(false)
}

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

/// Check if an extension is one symphonia handles well
pub fn is_standard_format(ext: &str) -> bool {
    STANDARD_EXTENSIONS.contains(&ext.to_lowercase().as_str())
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

/// Check if an extension is a USF format (N64 Sound Format)
pub fn is_usf_format(ext: &str) -> bool {
    USF_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file path is a USF format
pub fn is_usf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_usf_format(e))
        .unwrap_or(false)
}

/// Check if an extension is a PSF format (PlayStation 1 Sound Format)
pub fn is_psf_format(ext: &str) -> bool {
    PSF_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file path is a PSF format (PS1)
pub fn is_psf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_psf_format(e))
        .unwrap_or(false)
}

/// Check if an extension is a PSF2 format (PlayStation 2 Sound Format)
pub fn is_psf2_format(ext: &str) -> bool {
    PSF2_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file path is a PSF2 format (PS2)
pub fn is_psf2_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_psf2_format(e))
        .unwrap_or(false)
}
