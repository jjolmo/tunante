//! Which face the tray wears, and who decides it.
//!
//! There is no single answer, because the three desktops solve light-versus-dark
//! in three ways that do not compose:
//!
//! **macOS** does it for you, and doing it yourself is the mistake. A template
//! image is a mask: the system paints it black on a light menu bar, white on a
//! dark one, and inverts it again while the menu is open. That last state is
//! what gives away an app swapping files by hand — it cannot be reproduced.
//! Note also that `Window::theme()` reports the *window's* appearance, not the
//! menu bar's, and on macOS they can differ.
//!
//! **Windows** has no such mechanism, and the setting to read is not the one
//! that looks obvious. The notification area follows `SystemUsesLightTheme`,
//! which is separate from `AppsUseLightTheme` — and Windows 11 ships with light
//! apps on a dark taskbar by default. Keying off the window theme therefore
//! puts a black glyph on a black taskbar for the *default* configuration.
//!
//! **Linux** recolours nothing it is handed as a pixmap. It recolours when it
//! is handed a *name*: StatusNotifierItem defines `IconName` beside
//! `IconPixmap`, hosts prefer the name, and a name ending in `-symbolic`
//! resolving to an SVG is what Plasma 6, GTK and XApp all agree to restyle.
//! `tauri::tray` cannot express that, which is why the vendored `tray-icon`
//! carries `set_symbolic_icon`.

use serde::{Deserialize, Serialize};

/// What the user asked for in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TrayStyle {
    /// Let each platform do what it does natively. The default, and on every
    /// platform the answer that needs the least from us.
    #[default]
    System,
    /// The monochrome glyph, always.
    Symbolic,
    /// The pixel-art cartridge, always. Immune to every theme question below,
    /// at the price of not looking like part of the panel.
    Logo,
}

impl TrayStyle {
    pub fn parse(v: Option<&str>) -> Self {
        match v {
            Some("symbolic") => Self::Symbolic,
            Some("logo") => Self::Logo,
            _ => Self::System,
        }
    }

    fn monochrome(self) -> bool {
        !matches!(self, Self::Logo)
    }
}

/// Is the surface the icon sits on light?
///
/// Only meaningful where we have to choose a colour ourselves.
#[cfg(windows)]
pub fn panel_is_light() -> bool {
    // `SystemUsesLightTheme`, not `AppsUseLightTheme`. They are different keys
    // for different surfaces, and Windows 11's default is light apps on a dark
    // taskbar — so reading the app one gets the taskbar wrong out of the box.
    use std::process::Command;
    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "SystemUsesLightTheme",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            // `REG_DWORD    0x1` — the last token is the value.
            text.split_whitespace()
                .last()
                .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                .map(|v| v == 1)
                // The key is absent on a fresh profile, and its absence means
                // the default, which is a dark taskbar.
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// The PNG to hand the tray, and whether it should be treated as a template.
///
/// The template flag is macOS-only and ignored elsewhere.
pub fn icon_bytes(style: TrayStyle) -> (&'static [u8], bool) {
    #[allow(dead_code)]
    const LOGO: &[u8] = include_bytes!("../icons/tray-icon-big.png");
    #[allow(dead_code)]
    const TEMPLATE: &[u8] = include_bytes!("../icons/tray/template.png");
    #[allow(dead_code)]
    const BLACK: &[u8] = include_bytes!("../icons/tray/mono-black.png");
    #[allow(dead_code)]
    const WHITE: &[u8] = include_bytes!("../icons/tray/mono-white.png");

    if !style.monochrome() {
        return (LOGO, false);
    }

    #[cfg(target_os = "macos")]
    {
        // One image, marked as a mask. Everything else is the system's job.
        (TEMPLATE, true)
    }
    #[cfg(windows)]
    {
        (if panel_is_light() { BLACK } else { WHITE }, false)
    }
    #[cfg(target_os = "linux")]
    {
        // Only reached when the symbolic name could not be published — see
        // `install_symbolic`. White because Linux panels are overwhelmingly
        // dark, so it is the guess that is wrong least often.
        (WHITE, false)
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        (TEMPLATE, false)
    }
}

/// Put the symbolic SVG somewhere the panel can resolve it, and name it.
///
/// Returns whether the name was published. A `false` here is not an error: the
/// caller falls back to a pixmap, which is what every other tray icon in the
/// world does.
///
/// The file is written at runtime rather than installed by the package because
/// the same binary ships as a .deb, an AppImage and a `cargo run`, and only the
/// first of those has anywhere to install it.
#[cfg(target_os = "linux")]
pub fn install_symbolic(style: TrayStyle) -> bool {
    use std::io::Write;

    if !style.monochrome() {
        tray_icon::set_symbolic_icon(None);
        return false;
    }

    const SVG: &str = include_str!("../icons/tray/tunante-symbolic.svg");
    const NAME: &str = "tunante-symbolic";

    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("tunante-tray");

    // Two copies of the same file. A flat directory is what libayatana's
    // `IconThemePath` documents, but Plasma feeds that path to Qt's theme
    // search, which expects `<theme>/<size>/<context>/`. Writing both costs a
    // kilobyte and removes a guess about which host is on the other end.
    let themed = dir.join("hicolor/scalable/apps");
    if std::fs::create_dir_all(&themed).is_err() {
        return false;
    }
    for target in [dir.join(format!("{NAME}.svg")), themed.join(format!("{NAME}.svg"))] {
        match std::fs::File::create(&target).and_then(|mut f| f.write_all(SVG.as_bytes())) {
            Ok(()) => {}
            Err(e) => {
                log::warn!("tray: could not write {}: {e}", target.display());
                return false;
            }
        }
    }

    tray_icon::set_symbolic_icon(Some((NAME.to_string(), dir)));
    true
}

#[cfg(not(target_os = "linux"))]
pub fn install_symbolic(_style: TrayStyle) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_setting_falls_back_to_the_system_default() {
        assert_eq!(TrayStyle::parse(None), TrayStyle::System);
        assert_eq!(TrayStyle::parse(Some("")), TrayStyle::System);
        assert_eq!(TrayStyle::parse(Some("nonsense")), TrayStyle::System);
    }

    #[test]
    fn the_colour_logo_is_the_only_style_that_is_not_monochrome() {
        assert!(!TrayStyle::Logo.monochrome());
        assert!(TrayStyle::Symbolic.monochrome());
        assert!(TrayStyle::System.monochrome());
    }

    /// The template flag must never be set for the colour logo: macOS would
    /// reduce it to a silhouette of its own bounding box.
    #[test]
    fn the_colour_logo_is_never_a_template() {
        let (_, template) = icon_bytes(TrayStyle::Logo);
        assert!(!template);
    }

    #[test]
    fn every_style_yields_a_real_png() {
        for style in [TrayStyle::System, TrayStyle::Symbolic, TrayStyle::Logo] {
            let (bytes, _) = icon_bytes(style);
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{style:?} is not a PNG");
        }
    }
}
