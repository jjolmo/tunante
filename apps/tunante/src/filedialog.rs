//! The native "choose your music folders" dialog, for the desktop shell.
//!
//! The phone shell keeps its own browser (`picker.rs`): a portal dialog on a
//! phone is a desktop window squeezed into a phone-sized hole. The desktop is
//! the other way round — the folder chooser people know is the system's, and a
//! full-screen list of directories with tick boxes is not how a desktop app
//! asks where the music is.
//!
//! Linux talks to the XDG FileChooser portal over the zbus the app already
//! speaks for the tray, MPRIS and the shortcuts: no GTK, and it works under
//! Wayland, X11, KDE and GNOME alike, with each desktop's own dialog. Windows
//! uses IFileDialog through `rfd`.
//!
//! The dialog runs on its own thread and reports through a channel that the
//! 500 ms timer in main.rs drains — the same shape as every other worker.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// Open the dialog; the chosen folders (none when cancelled) arrive on `tx`.
pub fn pick_folders(title: String, tx: Sender<Vec<PathBuf>>) {
    std::thread::Builder::new()
        .name("folder-dialog".into())
        // zbus's async machinery wants more than musl's 128 KB default.
        .stack_size(1024 * 1024)
        .spawn(move || {
            let picked = pick(&title).unwrap_or_else(|e| {
                log::warn!("folder dialog: {e}");
                Vec::new()
            });
            let _ = tx.send(picked);
        })
        .ok();
}

#[cfg(target_os = "linux")]
fn pick(title: &str) -> Result<Vec<PathBuf>, String> {
    use futures_lite::StreamExt;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zbus::zvariant::{OwnedValue, Value};

    // One token per call: the portal files the reply under it, and two dialogs
    // in one process must not read each other's answer.
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let token = format!("tunante_fc{}", CALLS.fetch_add(1, Ordering::Relaxed));

    async_io::block_on(async move {
        let conn = zbus::Connection::session().await.map_err(|e| e.to_string())?;
        // The portal answers through a Response signal on a Request object
        // whose path is derived from our unique name and the token — so the
        // listener exists before the call does. Same dance as shortcuts.rs.
        let unique = conn
            .unique_name()
            .map(|n| n.trim_start_matches(':').replace('.', "_"))
            .unwrap_or_default();
        let request = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            format!("/org/freedesktop/portal/desktop/request/{unique}/{token}"),
            "org.freedesktop.portal.Request",
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut responses = request.receive_signal("Response").await.map_err(|e| e.to_string())?;

        let chooser = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.FileChooser",
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut opts: HashMap<&str, Value> = HashMap::new();
        opts.insert("handle_token", Value::from(token.as_str()));
        opts.insert("directory", Value::from(true));
        opts.insert("multiple", Value::from(true));
        // No parent window handle: Slint does not hand one out portably, and
        // the portal then centres the dialog on the screen, which is fine.
        chooser
            .call_method("OpenFile", &("", title, opts))
            .await
            .map_err(|e| e.to_string())?;

        let msg = responses
            .next()
            .await
            .ok_or_else(|| tunante_core::i18n::tr("el portal colgó sin contestar"))?;
        let (code, results): (u32, HashMap<String, OwnedValue>) =
            msg.body().deserialize().map_err(|e| e.to_string())?;
        // 1 is the user cancelling: a decision, not a failure, so no folders
        // and no complaint.
        if code != 0 {
            return Ok(Vec::new());
        }
        let uris: Vec<String> = results
            .get("uris")
            .and_then(|v| Vec::<String>::try_from(v.clone()).ok())
            .unwrap_or_default();
        Ok(uris.iter().filter_map(|u| path_from_file_uri(u)).collect())
    })
}

#[cfg(target_os = "windows")]
fn pick(title: &str) -> Result<Vec<PathBuf>, String> {
    Ok(rfd::FileDialog::new()
        .set_title(title)
        .pick_folders()
        .unwrap_or_default())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn pick(_title: &str) -> Result<Vec<PathBuf>, String> {
    Err(tunante_core::i18n::tr("no disponible aquí"))
}

/// `file:///home/x/M%C3%BAsica` → `/home/x/Música`. Percent-decoding by hand:
/// the bytes are a path, not text, so they go straight into an OsString.
#[cfg(target_os = "linux")]
fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?;
    // A host part ("file://localhost/…") is legal; only a local one is ours.
    let path = if rest.starts_with('/') { rest } else { rest.split_once('/').map(|(_, p)| p)? };
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 0 && i + 2 <= bytes.len() - 1 {
            if let Ok(b) = u8::from_str_radix(&path[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    if !path.starts_with('/') {
        out.insert(0, b'/');
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

/// Where the music most plausibly already is: the XDG music directory when
/// the desktop declares one, else the usual names under $HOME. Offered as the
/// first folder of the onboarding, ticked, so the common case is one click.
pub fn default_music_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let dirs = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(dirs) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix("XDG_MUSIC_DIR=") {
                let v = v.trim_matches('"').replace("$HOME", &home.to_string_lossy());
                let p = PathBuf::from(v);
                if p.is_dir() && p != home {
                    return Some(p);
                }
            }
        }
    }
    ["Music", "Música", "Musica"]
        .iter()
        .map(|d| home.join(d))
        .find(|p| p.is_dir())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    /// Opens the real portal dialog — needs a desktop session, so it is
    /// ignored by default: `cargo test -p tunante filedialog -- --ignored`.
    #[test]
    #[ignore]
    fn opens_the_portal_dialog() {
        let picked = super::pick("Tunante test").expect("portal reachable");
        eprintln!("picked: {picked:?}");
    }

    #[test]
    fn decodes_file_uris() {
        let p = super::path_from_file_uri("file:///home/x/M%C3%BAsica/Juegos").unwrap();
        assert_eq!(p, std::path::PathBuf::from("/home/x/Música/Juegos"));
        assert_eq!(
            super::path_from_file_uri("file://localhost/tmp/a%20b").unwrap(),
            std::path::PathBuf::from("/tmp/a b")
        );
        assert!(super::path_from_file_uri("http://x/").is_none());
    }
}
