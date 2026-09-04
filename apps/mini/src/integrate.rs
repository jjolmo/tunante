//! Desktop integration odds and ends: reveal-in-file-manager and the
//! .desktop entry. Linux-flavoured by nature; other platforms get the
//! fallbacks.

use std::path::Path;

/// Show a file *selected* in the file manager, not just its folder open.
///
/// `org.freedesktop.FileManager1.ShowItems` is what Dolphin, Nautilus and
/// Thunar all answer; when nobody does (or D-Bus is elsewhere), fall back to
/// opening the parent folder plain. Runs on its own thread because a D-Bus
/// activation attempt can take a couple of seconds deciding nobody is home.
pub fn reveal(path: &Path) {
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            let ok = std::process::Command::new("busctl")
                .args([
                    "--user",
                    "call",
                    "org.freedesktop.FileManager1",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1",
                    "ShowItems",
                    "ass",
                    "1",
                    &file_uri(&path),
                    "",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                if let Some(folder) = path.parent() {
                    let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
                }
            }
        }
        #[cfg(target_os = "windows")]
        {
            // Explorer's own reveal-with-selection. The comma is part of the
            // switch; no space after it, or the path becomes a second arg.
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            if let Some(folder) = path.parent() {
                let _ = std::process::Command::new("open").arg(folder).spawn();
            }
        }
    });
}

/// A file:// URI with the minimum encoding file managers require: spaces and
/// the URI-reserved bytes, nothing clever. Non-UTF-8 paths percent-encode
/// byte by byte, which is what the spec wants anyway.
#[cfg(unix)]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(b as char)
            }
            _ => uri.push_str(&format!("%{b:02X}")),
        }
    }
    uri
}

/// Write the launcher entry and its icon under the user's XDG dirs, pointing
/// at the binary that is running right now. Idempotent: run it again after
/// moving the install and the entry follows.
#[cfg(target_os = "linux")]
pub fn make_desktop_entry() -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|_| "sin $HOME".to_string())?;
    let data = std::env::var("XDG_DATA_HOME")
        .unwrap_or_else(|_| format!("{home}/.local/share"));

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;

    let icon_dir = Path::new(&data).join("icons/hicolor/128x128/apps");
    std::fs::create_dir_all(&icon_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        icon_dir.join("tunante.png"),
        include_bytes!("../dist/icons/128x128/tunante-mini.png"),
    )
    .map_err(|e| e.to_string())?;

    let apps = Path::new(&data).join("applications");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Tunante\n\
         Comment=Game music player\n\
         Exec={} %F\n\
         Icon=tunante\n\
         Terminal=false\n\
         Categories=AudioVideo;Audio;Player;\n\
         MimeType=audio/x-nsf;audio/x-spc;audio/x-psf;audio/x-gbs;audio/x-vgm;\n\
         StartupWMClass=tunante-mini\n",
        exe.display()
    );
    // The file's basename must equal the window's app_id (`tunante-mini`, set in
    // main.rs): KWin dresses the *titlebar* by matching the app_id to
    // `<app_id>.desktop`, not to StartupWMClass — that only covers the taskbar.
    // A `tunante.desktop` from an older build would leave a second menu entry
    // (and the wrong-named one), so retire it.
    let _ = std::fs::remove_file(apps.join("tunante.desktop"));
    std::fs::write(apps.join("tunante-mini.desktop"), entry).map_err(|e| e.to_string())?;

    // Best effort: without it the menu still picks the entry up on next login.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps)
        .status();
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn make_desktop_entry() -> Result<(), String> {
    Err("solo tiene sentido en Linux".to_string())
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::file_uri;
    use std::path::Path;

    #[test]
    fn uris_encode_spaces_and_survive_plain_paths() {
        assert_eq!(
            file_uri(Path::new("/home/u/Game Music/Sonic 2.vgm")),
            "file:///home/u/Game%20Music/Sonic%202.vgm"
        );
        assert_eq!(file_uri(Path::new("/a/b.nsf")), "file:///a/b.nsf");
        // The characters file managers actually choke on.
        assert_eq!(file_uri(Path::new("/a/#5&x.spc")), "file:///a/%235%26x.spc");
    }
}
