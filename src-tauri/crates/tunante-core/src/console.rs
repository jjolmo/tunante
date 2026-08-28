//! Which machine a track came from, from the name of its file.
//!
//! The library screen's "Consoles" index. Derived from the extension and not
//! from anything inside the file: a scan already knows the path, and opening
//! every rip to ask would cost a decoder process each.
//!
//! Moved out of `tunante-mini` so `tunante-android` groups the library the same
//! way rather than growing its own table of extensions.

use std::path::Path;

/// The console a path belongs to.
pub fn console_of(path: &str) -> &'static str {
    let real = crate::vgm_path::parse_vgm_path(path).0;
    let ext = Path::new(real)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "nsf" | "nsfe" => "NES",
        "spc" => "Super Nintendo",
        "gbs" => "Game Boy",
        "gsf" | "minigsf" | "gsflib" => "Game Boy Advance",
        "2sf" | "mini2sf" | "2sflib" => "Nintendo DS",
        "usf" | "miniusf" => "Nintendo 64",
        "psf" | "minipsf" | "psflib" => "PlayStation",
        "psf2" | "minipsf2" | "psf2lib" => "PlayStation 2",
        "vgm" | "vgz" => "VGM (Mega Drive y compañía)",
        "sid" => "Commodore 64",
        "ay" => "ZX Spectrum",
        "hes" => "PC Engine",
        "kss" => "MSX",
        "xa" => "PlayStation (streams)",
        "adx" | "ast" | "dsp" | "brstm" | "bcstm" | "strm" | "bfstm" | "hps" => {
            "Rips de GameCube, Wii y 3DS"
        }
        _ => "Otros",
    }
}

#[cfg(test)]
mod tests {
    use super::console_of;

    #[test]
    fn it_knows_the_formats_this_library_is_made_of() {
        assert_eq!(console_of("/m/a.nsf"), "NES");
        assert_eq!(console_of("/m/a.spc"), "Super Nintendo");
        assert_eq!(console_of("/m/a.psf2"), "PlayStation 2");
        assert_eq!(console_of("/m/a.vgz"), "VGM (Mega Drive y compañía)");
    }

    /// `.psf2` must not be read as `.psf`, and the match is on the whole
    /// extension rather than a prefix.
    #[test]
    fn psf2_is_not_psf() {
        assert_ne!(console_of("/m/a.psf2"), console_of("/m/a.psf"));
    }

    /// A subsong address is not an extension.
    #[test]
    fn a_subsong_suffix_does_not_hide_the_format() {
        assert_eq!(console_of("/m/pokemon.gbs#7"), "Game Boy");
    }

    #[test]
    fn case_does_not_matter() {
        assert_eq!(console_of("/m/A.SPC"), "Super Nintendo");
    }

    #[test]
    fn anything_else_lands_somewhere_rather_than_vanishing() {
        assert_eq!(console_of("/m/a.mp3"), "Otros");
        assert_eq!(console_of("/m/no-extension"), "Otros");
    }
}
