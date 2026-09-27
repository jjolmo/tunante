//! The native "choose your music folders" dialog, for the desktop shell — and
//! its "save as" sibling, for converting a track.
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
//! and macOS use `rfd`, which is IFileDialog on one and NSOpenPanel on the
//! other.
//!
//! The dialog runs on its own thread and reports through a channel that the
//! 500 ms timer in main.rs drains — the same shape as every other worker.
//! macOS is the exception, and it is not a stylistic one: NSOpenPanel may only
//! be touched from the main thread, so opening it from that worker is a hang,
//! not a warning. There the work hops back onto the event loop instead. The
//! channel is the same either way, so nothing upstream knows which happened.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// Open the dialog; the chosen folders (none when cancelled) arrive on `tx`.
#[cfg(not(target_os = "macos"))]
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

/// Open the dialog; the chosen folders (none when cancelled) arrive on `tx`.
///
/// No worker thread here. AppKit puts NSOpenPanel on the main thread and
/// nowhere else, so this asks the event loop to run it — which is also where
/// a modal dialog belongs, since it owns the window while it is open. The
/// caller still just waits on the channel.
#[cfg(target_os = "macos")]
pub fn pick_folders(title: String, tx: Sender<Vec<PathBuf>>) {
    let sent = slint::invoke_from_event_loop(move || {
        let picked = rfd::FileDialog::new().set_title(title).pick_folders().unwrap_or_default();
        let _ = tx.send(picked);
    });
    // The loop is gone (the window is closing). Nobody is left to answer, and
    // the caller must not wait forever for a folder that is never coming.
    if sent.is_err() {
        log::warn!("folder dialog: no event loop to open it on");
    }
}

#[cfg(target_os = "linux")]
fn pick(title: &str) -> Result<Vec<PathBuf>, String> {
    use zbus::zvariant::Value;
    portal("OpenFile", title, |opts| {
        opts.insert("directory", Value::from(true));
        opts.insert("multiple", Value::from(true));
    })
}

/// One FileChooser portal call: `OpenFile` or `SaveFile`, with the options
/// `fill` adds to the handle token every call carries. The chosen paths, none
/// when the user cancelled.
#[cfg(target_os = "linux")]
fn portal(
    method: &str,
    title: &str,
    fill: impl FnOnce(&mut std::collections::HashMap<&'static str, zbus::zvariant::Value<'static>>),
) -> Result<Vec<PathBuf>, String> {
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
        opts.insert("handle_token", Value::from(token.clone()));
        fill(&mut opts);
        // No parent window handle: Slint does not hand one out portably, and
        // the portal then centres the dialog on the screen, which is fine.
        chooser
            .call_method(method, &("", title, opts))
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

/// What a "save as" dialog is asking for.
#[derive(Clone, Copy, PartialEq)]
pub enum SaveKind {
    /// One MP3: the dialog filters to `.mp3`, and a name typed without the
    /// extension gets it added.
    Mp3,
    /// A folder to be created, named and placed by the user — where several
    /// converted tracks go together. No filter, no extension.
    NewFolder,
}

/// Ask where to save, offering `name` in `folder`; the chosen path (`None`
/// when cancelled) arrives on `tx`.
#[cfg(not(target_os = "macos"))]
pub fn save_as(
    title: String,
    name: String,
    folder: Option<PathBuf>,
    kind: SaveKind,
    tx: Sender<Option<PathBuf>>,
) {
    std::thread::Builder::new()
        .name("save-dialog".into())
        .stack_size(1024 * 1024)
        .spawn(move || {
            let picked = save(&title, &name, folder.as_deref(), kind).unwrap_or_else(|e| {
                log::warn!("save dialog: {e}");
                None
            });
            let _ = tx.send(picked.map(|p| finish(p, kind)));
        })
        .ok();
}

/// See the other `save_as`; on the main thread for the same reason as
/// `pick_folders`.
#[cfg(target_os = "macos")]
pub fn save_as(
    title: String,
    name: String,
    folder: Option<PathBuf>,
    kind: SaveKind,
    tx: Sender<Option<PathBuf>>,
) {
    let sent = slint::invoke_from_event_loop(move || {
        let mut dialog = rfd::FileDialog::new().set_title(title).set_file_name(name);
        if kind == SaveKind::Mp3 {
            dialog = dialog.add_filter("MP3", &["mp3"]);
        }
        if let Some(folder) = folder {
            dialog = dialog.set_directory(folder);
        }
        let _ = tx.send(dialog.save_file().map(|p| finish(p, kind)));
    });
    if sent.is_err() {
        log::warn!("save dialog: no event loop to open it on");
    }
}

fn finish(path: PathBuf, kind: SaveKind) -> PathBuf {
    match kind {
        SaveKind::Mp3 => with_mp3_extension(path),
        SaveKind::NewFolder => path,
    }
}

fn with_mp3_extension(path: PathBuf) -> PathBuf {
    let is_mp3 = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    if is_mp3 {
        path
    } else {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".mp3");
        path.with_file_name(name)
    }
}

#[cfg(target_os = "linux")]
fn save(title: &str, name: &str, folder: Option<&std::path::Path>, kind: SaveKind) -> Result<Option<PathBuf>, String> {
    use std::os::unix::ffi::OsStrExt;
    use zbus::zvariant::Value;
    let name = name.to_string();
    // `current_folder` is a byte string with its NUL: a path, not text.
    let folder: Option<Vec<u8>> = folder.map(|f| {
        let mut bytes = f.as_os_str().as_bytes().to_vec();
        bytes.push(0);
        bytes
    });
    let picked = portal("SaveFile", title, move |opts| {
        opts.insert("current_name", Value::from(name));
        if let Some(folder) = folder {
            opts.insert("current_folder", Value::from(folder));
        }
        if kind == SaveKind::Mp3 {
            let filters: Vec<(String, Vec<(u32, String)>)> =
                vec![("MP3".to_string(), vec![(0, "*.mp3".to_string())])];
            opts.insert("filters", Value::from(filters));
        }
    })?;
    Ok(picked.into_iter().next())
}

#[cfg(target_os = "windows")]
fn save(title: &str, name: &str, folder: Option<&std::path::Path>, kind: SaveKind) -> Result<Option<PathBuf>, String> {
    let mut dialog = rfd::FileDialog::new().set_title(title).set_file_name(name);
    if kind == SaveKind::Mp3 {
        dialog = dialog.add_filter("MP3", &["mp3"]);
    }
    if let Some(folder) = folder {
        dialog = dialog.set_directory(folder);
    }
    Ok(dialog.save_file())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn save(_title: &str, _name: &str, _folder: Option<&std::path::Path>, _kind: SaveKind) -> Result<Option<PathBuf>, String> {
    Err(tunante_core::i18n::tr("no disponible aquí"))
}

#[cfg(target_os = "windows")]
fn pick(title: &str) -> Result<Vec<PathBuf>, String> {
    Ok(rfd::FileDialog::new()
        .set_title(title)
        .pick_folders()
        .unwrap_or_default())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
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
