//! Keeping the window where it was across a hide and a show, on KDE Wayland.
//!
//! Wayland has no hidden windows, so Slint destroys ours on hide and builds a
//! new one on show — and a Wayland client cannot say where a window goes, so
//! KWin places the new one as if it had never existed: centred, every time
//! it comes back from the tray.
//!
//! KWin can be told, though, by a script of its own. This loads one that
//! watches this process's main window, remembers its frame whenever it moves
//! or resizes, and puts every new one of ours back in that frame the moment it
//! appears — before its first frame is on screen, so there is no jump.
//!
//! Anywhere else this does nothing: X11, Windows and macOS keep the window
//! alive while it is hidden, and it comes back where it was on its own.

#[cfg(all(target_os = "linux", feature = "tray"))]
pub fn spawn() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_uppercase().contains("KDE"));
    if !wayland || !kde {
        return;
    }
    std::thread::spawn(|| {
        if let Err(e) = async_io::block_on(load()) {
            eprintln!("placement: no se pudo cargar el script de KWin: {e}");
        }
    });
}

#[cfg(not(all(target_os = "linux", feature = "tray")))]
pub fn spawn() {}

/// The script's name in KWin, one per process.
///
/// Not one fixed name with the last session's script unloaded first: KWin
/// takes a script loaded under a name it has just unloaded, reports it as
/// loaded, and never runs its handlers — so the window came back centred with
/// the script sitting there. A stale script from an earlier session only ever
/// watches a process id that no longer exists, and goes with the session.
#[cfg(all(target_os = "linux", feature = "tray"))]
fn name() -> String {
    format!("tunante-placement-{}", std::process::id())
}

/// The script. `skipTaskbar` leaves out the volume panel's layer surface,
/// which is the same process and also counts as a normal window to KWin.
#[cfg(all(target_os = "linux", feature = "tray"))]
const SCRIPT: &str = r#"
var saved = null;
function remember(w) {
    var g = w.frameGeometry;
    saved = {x: g.x, y: g.y, width: g.width, height: g.height};
}
function track(w) {
    if (w.pid != PID || !w.normalWindow || w.skipTaskbar) return;
    if (saved) w.frameGeometry = saved;
    remember(w);
    w.frameGeometryChanged.connect(function() { remember(w); });
}
workspace.windowList().forEach(track);
workspace.windowAdded.connect(track);
"#;

#[cfg(all(target_os = "linux", feature = "tray"))]
async fn load() -> zbus::Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let name = name();
    let file = dir.join(format!("{name}.js"));
    let script = format!("var PID = {};\n{SCRIPT}", std::process::id());
    std::fs::write(&file, script).map_err(|e| zbus::Error::Failure(e.to_string()))?;

    let conn = zbus::Connection::session().await?;
    let id: i32 = conn
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "loadScript",
            &(file.to_string_lossy().as_ref(), name.as_str()),
        )
        .await?
        .body()
        .deserialize()?;
    if id < 0 {
        return Err(zbus::Error::Failure("KWin rechazó el script".into()));
    }
    conn.call_method(
        Some("org.kde.KWin"),
        format!("/Scripting/Script{id}").as_str(),
        Some("org.kde.kwin.Script"),
        "run",
        &(),
    )
    .await?;
    Ok(())
}
