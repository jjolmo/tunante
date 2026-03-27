use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

/// Timestamp (ms since epoch) until which the tray tooltip should show volume
/// instead of track info. Set by scroll handlers, checked by state update thread.
static VOLUME_TOOLTIP_UNTIL: AtomicU64 = AtomicU64::new(0);

pub mod audio;
pub mod commands;
pub mod db;
pub mod debug_log;
pub mod metadata;
pub mod shortcuts;
pub mod updater;
pub mod watcher;

#[derive(Clone, serde::Serialize)]
struct PlaybackErrorPayload {
    message: String,
    path: String,
}

pub struct AppState {
    pub audio: parking_lot::Mutex<audio::AudioEngine>,
    pub db: parking_lot::Mutex<db::Database>,
    pub queue: parking_lot::Mutex<audio::PlayQueue>,
    pub watcher: parking_lot::Mutex<Option<watcher::FolderWatcher>>,
    /// Maps global shortcut ID → action_id for keyboard shortcuts
    pub shortcut_map: parking_lot::Mutex<std::collections::HashMap<u32, String>>,
    pub mouse_bindings: parking_lot::Mutex<std::collections::HashMap<String, String>>,
}

#[derive(Clone, serde::Serialize)]
struct PlayerStatePayload {
    is_playing: bool,
    position_ms: u64,
    duration_ms: u64,
    volume: f32,
}

fn setup_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Call default hook first (prints to stderr)
        default_hook(info);

        // Only show crash dialog for the main thread - spawned threads (like decoder)
        // have their panics caught by join() and handled gracefully
        let is_main = std::thread::current().name() == Some("main");
        if !is_main {
            return;
        }

        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let crash_text = format!(
            "Tunante crashed!\n\nError: {}\nLocation: {}\n\nPlease report this bug.",
            message, location
        );

        // Write crash log
        if let Ok(home) = std::env::var("HOME") {
            let crash_path = std::path::PathBuf::from(home)
                .join(".local/share/com.tunante.app/crash.log");
            let _ = std::fs::write(&crash_path, &crash_text);
        } else {
            let _ = std::fs::write("/tmp/tunante-crash.log", &crash_text);
        }

        // Show dialog via zenity/kdialog (works even if Tauri event loop is dead)
        let _ = std::process::Command::new("zenity")
            .args(["--error", "--title=Tunante Crash", "--text", &crash_text, "--width=500"])
            .status()
            .or_else(|_| {
                std::process::Command::new("kdialog")
                    .args(["--error", &crash_text, "--title", "Tunante Crash"])
                    .status()
            })
            .or_else(|_| {
                std::process::Command::new("xmessage")
                    .args(["-center", &crash_text])
                    .status()
            });
    }));
}

/// Handle a tray scroll event: adjust volume by ±5% per tick and show
/// a popup near the tray icon with the current volume level.
fn handle_tray_scroll(app: &tauri::AppHandle, state: &Arc<AppState>, delta: f64) {
    let mut audio = state.audio.lock();
    let step = 0.05_f32;
    let new_vol = if delta > 0.0 {
        (audio.volume() + step).min(1.0)
    } else if delta < 0.0 {
        (audio.volume() - step).max(0.0)
    } else {
        return;
    };
    audio.set_volume(new_vol);
    drop(audio);

    // Emit to main frontend so volume slider updates immediately
    let _ = app.emit("volume-scrolled", new_vol);

    // Show volume popup near tray icon
    show_volume_popup(app, new_vol);
}

/// Show or update the volume popup window near the tray icon.
fn show_volume_popup(app: &tauri::AppHandle, volume: f32) {
    use tauri::WebviewWindowBuilder;

    let popup_label = "volume-popup";
    let popup = app.get_webview_window(popup_label);

    // Create popup window if it doesn't exist yet
    let popup = match popup {
        Some(w) => w,
        None => {
            let url = tauri::WebviewUrl::App("volume-popup.html".into());
            match WebviewWindowBuilder::new(app, popup_label, url)
                .title("")
                .inner_size(200.0, 38.0)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .minimizable(false)
                .closable(false)
                .focused(false)
                .visible(false)
                .transparent(true)
                .shadow(false)
                .build()
            {
                Ok(w) => w,
                Err(e) => {
                    log::warn!("Failed to create volume popup: {}", e);
                    return;
                }
            }
        }
    };

    // Position near tray icon
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(Some(rect)) = tray.rect() {
            // Extract physical coordinates from the rect
            let (px, py) = match rect.position {
                tauri::Position::Physical(p) => (p.x as f64, p.y as f64),
                tauri::Position::Logical(p) => (p.x, p.y),
            };
            let (sw, sh) = match rect.size {
                tauri::Size::Physical(s) => (s.width as f64, s.height as f64),
                tauri::Size::Logical(s) => (s.width, s.height),
            };
            let popup_w = 200.0;
            let popup_h = 38.0;
            let x = px - popup_w / 2.0 + sw / 2.0;
            // macOS: tray is at top, popup goes below. Others: tray at bottom, popup above.
            #[cfg(target_os = "macos")]
            let y = py + sh + 4.0;
            #[cfg(not(target_os = "macos"))]
            let y = py - popup_h - 4.0;
            let _ = popup.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
        }
    }

    // Send volume to the popup page
    let _ = popup.emit("volume-popup-update", volume);
    let _ = popup.show();

    // Schedule hide after 1.5 seconds
    let suppress_until = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        + 1500;
    VOLUME_TOOLTIP_UNTIL.store(suppress_until, Ordering::Relaxed);

    // Spawn thread to hide popup after timeout
    let app_clone = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        // Only hide if no newer scroll happened
        if now >= VOLUME_TOOLTIP_UNTIL.load(Ordering::Relaxed) {
            if let Some(popup) = app_clone.get_webview_window("volume-popup") {
                let _ = popup.hide();
            }
        }
    });
}

/// Prevent macOS App Nap from suspending the process when the window is
/// minimized or hidden. Without this, the main run loop pauses and global
/// shortcuts / CGEventTap stop firing.
#[cfg(target_os = "macos")]
fn disable_app_nap() {
    use objc2_foundation::{NSProcessInfo, NSActivityOptions, NSString};

    let info = NSProcessInfo::processInfo();
    let reason = NSString::from_str("Audio playback and global shortcuts");
    // UserInitiatedAllowingIdleSystemSleep: keeps CPU active but lets display sleep
    let options = NSActivityOptions::UserInitiatedAllowingIdleSystemSleep;
    let token = info.beginActivityWithOptions_reason(options, &reason);
    // Leak the token so the activity is never ended
    std::mem::forget(token);
}

/// Runs the bundled update_mac.sh script in Terminal.app.
/// Copies the script to /tmp first (since the app bundle will be replaced).
#[tauri::command]
fn run_macos_update_script(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        return Err("This command is only available on macOS".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| format!("Cannot find resource dir: {}", e))?;
        let script_path = resource_dir.join("update_mac.sh");

        if !script_path.exists() {
            return Err(format!(
                "Update script not found at {}",
                script_path.display()
            ));
        }

        // Copy to temp so it survives app bundle replacement
        let tmp_script = std::env::temp_dir().join("tunante_update_mac.sh");
        std::fs::copy(&script_path, &tmp_script)
            .map_err(|e| format!("Failed to copy script: {}", e))?;

        // Make executable
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_script, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set permissions: {}", e))?;

        // Run in Terminal.app so user sees progress
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(format!(
                r#"tell application "Terminal"
                    activate
                    do script "bash '{}'"
                end tell"#,
                tmp_script.display()
            ))
            .spawn()
            .map_err(|e| format!("Failed to open Terminal: {}", e))?;

        Ok(())
    }
}

/// Installs a D-Bus message filter on the session bus that handles
/// StatusNotifierItem methods: `Activate` (left-click) and `Scroll` (wheel).
///
/// libayatana-appindicator 0.5.x does not implement `Activate` in its D-Bus
/// interface, so KDE falls back to showing the context menu on left-click.
/// This filter intercepts both methods and sends success replies.
#[cfg(target_os = "linux")]
fn setup_dbus_tray_handler(handle: tauri::AppHandle) {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        log::warn!("Could not get D-Bus session bus for tray handler");
        return;
    };

    connection.add_filter(move |conn, msg, incoming| {
        if !incoming {
            return Some(msg.to_owned());
        }

        let is_sni = msg.message_type() == gio::DBusMessageType::MethodCall
            && msg.interface().as_deref() == Some("org.kde.StatusNotifierItem");

        if !is_sni {
            return Some(msg.to_owned());
        }

        match msg.member().as_deref() {
            Some("Activate") => {
                // Toggle main window visibility
                if let Some(window) = handle.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            }
            Some("Scroll") => {
                // D-Bus Scroll signature: (i32 delta, String orientation)
                use gio::glib::prelude::*;
                if let Some(body) = msg.body() {
                    let delta: i32 = body.child_value(0).get().unwrap_or(0);
                    let orientation: String = body.child_value(1).get().unwrap_or_default();
                    if orientation == "vertical" && delta != 0 {
                        let state = handle.state::<Arc<AppState>>();
                        handle_tray_scroll(&handle, &state, delta as f64);
                    }
                }
            }
            _ => return Some(msg.to_owned()),
        }

        // Send a success reply
        let reply = msg.to_owned();
        let method_reply = gio::DBusMessage::new_method_reply(&reply);
        let _ = conn.send_message(&method_reply, gio::DBusSendMessageFlags::NONE);
        None
    });
}

/// Build the global-shortcut plugin with media key + user shortcut handlers.
fn setup_global_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_global_shortcut::{Code, ShortcutState};

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }

            let state = app.state::<Arc<AppState>>();

            // Check hardcoded media keys first
            match shortcut.key {
                Code::MediaPlayPause => {
                    shortcuts::handle_action("play_pause", app, &state);
                    return;
                }
                Code::MediaTrackNext => {
                    shortcuts::handle_action("next_track", app, &state);
                    return;
                }
                Code::MediaTrackPrevious => {
                    shortcuts::handle_action("prev_track", app, &state);
                    return;
                }
                Code::MediaStop => {
                    shortcuts::handle_action("stop", app, &state);
                    return;
                }
                _ => {}
            }

            // Check user-configured shortcuts
            let shortcut_map = state.shortcut_map.lock();
            if let Some(action_id) = shortcut_map.get(&shortcut.id()) {
                let action = action_id.clone();
                drop(shortcut_map);
                shortcuts::handle_action(&action, app, &state);
            }
        })
        .build()
}

#[tauri::command]
fn get_debug_logs() -> Vec<debug_log::LogEntry> {
    debug_log::get_logs()
}

#[tauri::command]
fn clear_debug_logs() {
    debug_log::clear_logs();
}

/// Raise the per-process file descriptor soft limit on macOS.
///
/// macOS defaults to 256 open files, but the kqueue-based file watcher
/// (notify crate) opens one FD per watched directory. A large music library
/// with hundreds of album folders exhausts this limit, causing "Too many
/// open files" (EMFILE / os error 24) during both watching and scanning.
///
/// We raise the soft limit to the hard limit (typically 10240 on macOS),
/// which is safe and matches what many other apps do (Chrome, VS Code, etc.).
#[cfg(target_os = "macos")]
fn raise_fd_limit() {
    unsafe {
        let mut rlim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 {
            let old = rlim.rlim_cur;
            // Cap at 10240 to be safe — the hard limit on macOS is usually
            // the same or higher.  Going above 10240 requires root on some
            // macOS versions.
            let target = if rlim.rlim_max > 10240 {
                10240
            } else {
                rlim.rlim_max
            };
            if rlim.rlim_cur < target {
                rlim.rlim_cur = target;
                if libc::setrlimit(libc::RLIMIT_NOFILE, &rlim) == 0 {
                    log::info!("Raised file descriptor limit: {} → {}", old, target);
                } else {
                    log::warn!("Failed to raise file descriptor limit from {}", old);
                }
            }
        }
    }
}

pub fn run() {
    debug_log::init();
    setup_panic_hook();

    // Raise macOS FD limit before anything opens files
    #[cfg(target_os = "macos")]
    raise_fd_limit();

    // Force X11 backend on Linux for native window decorations.
    //
    // On Wayland, tao (Tauri's windowing library) explicitly sets a GTK3
    // client-side-decoration (CSD) header bar (tao PR #979). This overrides
    // GTK_CSD=0 and renders GNOME-style buttons instead of the window
    // manager's native titlebar (KDE Breeze, etc.).
    //
    // Forcing X11 (via XWayland) lets the window manager draw decorations,
    // matching the user's system theme. The performance impact is negligible
    // for a music player. This is the recommended workaround until tao
    // supports xdg-decoration negotiation (tracked in tao#1046).
    #[cfg(target_os = "linux")]
    {
        if std::env::var("GDK_BACKEND").is_err() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(setup_global_shortcut_plugin())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // When a second instance is launched, focus the existing window
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("tunante.db");

            let db = db::Database::new(&db_path).expect("Failed to initialize database");
            let audio_engine =
                audio::AudioEngine::new().expect("Failed to initialize audio engine");
            let queue = audio::PlayQueue::new();

            // Load user shortcut bindings from DB
            let user_bindings: std::collections::HashMap<String, String> = db
                .get_setting("shortcuts")
                .ok()
                .flatten()
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();

            let state = Arc::new(AppState {
                audio: parking_lot::Mutex::new(audio_engine),
                db: parking_lot::Mutex::new(db),
                queue: parking_lot::Mutex::new(queue),
                watcher: parking_lot::Mutex::new(None),
                shortcut_map: parking_lot::Mutex::new(std::collections::HashMap::new()),
                mouse_bindings: parking_lot::Mutex::new(user_bindings.clone()),
            });

            app.manage(state.clone());

            // Initialize file watcher
            {
                let fw =
                    watcher::FolderWatcher::new(state.clone(), app.handle().clone());
                *state.watcher.lock() = Some(fw);

                let db = state.db.lock();
                if let Ok(folders) = db.get_monitored_folders() {
                    drop(db);
                    let mut watcher_lock = state.watcher.lock();
                    if let Some(ref mut w) = *watcher_lock {
                        for folder in folders {
                            if folder.watching_enabled {
                                if let Err(e) = w.start_watching(&folder.path) {
                                    log::error!(
                                        "Failed to start watching {}: {}",
                                        folder.path,
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Set up system tray icon
            //
            // Platform behaviour:
            //   Windows/macOS — left-click toggles window (via on_tray_icon_event),
            //                   right-click opens the context menu.
            //   Linux (KDE)  — Our patched tray-icon sets the first menu item as
            //                   the secondary_activate_target. KDE sends the SNI
            //                   `Activate` D-Bus method on left-click, which triggers
            //                   the "Show / Hide" item directly. Right-click opens
            //                   the full context menu.
            //   Linux (GNOME)— GNOME's AppIndicator extension always shows the menu
            //                   on any click. The "Show / Hide" item is at the top.
            let show_hide_item = MenuItemBuilder::with_id("show_hide", "Show / Hide").build(app)?;
            let play_pause_item = MenuItemBuilder::with_id("play_pause", "Play / Pause").build(app)?;
            let next_item = MenuItemBuilder::with_id("next_track", "Next").build(app)?;
            let prev_item = MenuItemBuilder::with_id("prev_track", "Previous").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&show_hide_item)
                .separator()
                .item(&play_pause_item)
                .item(&next_item)
                .item(&prev_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Read initial tray visibility from settings (default: visible)
            let show_in_tray = {
                let db = state.db.lock();
                db.get_setting("show_in_tray")
                    .ok()
                    .flatten()
                    .map(|v| v == "true")
                    .unwrap_or(false)
            };

            // Choose tray icon based on OS theme (white for dark, black for light)
            let tray_icon = {
                let is_light = app.get_webview_window("main")
                    .and_then(|w| w.theme().ok())
                    .map(|t| t == tauri::Theme::Light)
                    .unwrap_or(false);

                let png_bytes: &[u8] = if is_light {
                    include_bytes!("../icons/tray-icon-big-black-fixed.png")
                } else {
                    include_bytes!("../icons/tray-icon-big-fixed.png")
                };
                let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
                let mut reader = decoder.read_info().expect("Failed to decode tray icon PNG");
                let mut buf = vec![0u8; reader.output_buffer_size()];
                let info = reader.next_frame(&mut buf).expect("Failed to read tray icon frame");
                buf.truncate(info.buffer_size());
                tauri::image::Image::new_owned(buf, info.width, info.height)
            };

            let tray_builder = TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .tooltip("Tunante")
                .menu(&tray_menu);

            // On Windows/macOS: disable menu on left-click so left-click
            // fires the Click event (toggle window), right-click shows menu.
            // On Linux: do NOT call this — libayatana-appindicator needs the
            // menu activation path to even display the tray icon.
            #[cfg(not(target_os = "linux"))]
            let tray_builder = tray_builder.show_menu_on_left_click(false);

            let tray = tray_builder
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show_hide" => {
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.unminimize();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                        "play_pause" | "next_track" | "prev_track" => {
                            let state = app.state::<Arc<AppState>>();
                            shortcuts::handle_action(event.id().as_ref(), app, &state);
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event({
                    // Debounce tray clicks — macOS fires the Click event twice
                    // for a single physical click, causing the window to toggle
                    // back to its original state.
                    use std::sync::atomic::{AtomicU64, Ordering};
                    static LAST_CLICK_MS: AtomicU64 = AtomicU64::new(0);

                    move |tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { button, .. } = event {
                        let app = tray.app_handle();
                        match button {
                            tauri::tray::MouseButton::Left => {
                                // Debounce: ignore clicks within 500ms of each other
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64;
                                let prev = LAST_CLICK_MS.swap(now, Ordering::Relaxed);
                                if now.saturating_sub(prev) < 500 {
                                    return;
                                }

                                // Left click: toggle window visibility
                                if let Some(window) = app.get_webview_window("main") {
                                    if window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false) {
                                        let _ = window.hide();
                                    } else {
                                        let _ = window.show();
                                        let _ = window.unminimize();
                                        let _ = window.set_focus();
                                    }
                                }
                            }
                            tauri::tray::MouseButton::Middle => {
                                // Middle click: toggle pause/resume
                                let state = app.state::<Arc<AppState>>();
                                let mut audio = state.audio.lock();
                                if audio.is_playing() {
                                    audio.pause();
                                } else {
                                    audio.resume();
                                }
                            }
                            _ => {}
                        }
                    }
                }})
                .build(app)?;

            // Apply initial visibility
            let _ = tray.set_visible(show_in_tray);

            // Linux: install a D-Bus message filter to handle StatusNotifierItem
            // methods: Activate (left-click toggle) and Scroll (volume control).
            #[cfg(target_os = "linux")]
            {
                setup_dbus_tray_handler(app.handle().clone());
            }

            // Register tray scroll handler for volume control (macOS + Windows).
            // Linux handles scroll via D-Bus directly in setup_dbus_tray_handler.
            #[cfg(not(target_os = "linux"))]
            {
                let scroll_state = state.clone();
                let scroll_handle = app.handle().clone();
                tray_icon::set_scroll_handler(move |_id, delta| {
                    handle_tray_scroll(&scroll_handle, &scroll_state, delta);
                });
            }

            // Prevent App Nap on macOS so global shortcuts and CGEventTap
            // keep working when the window is minimized or hidden.
            #[cfg(target_os = "macos")]
            {
                disable_app_nap();
            }

            // Register global shortcuts: media keys + user-configured
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, Code, Shortcut};
                // Hardcoded media keys (always active)
                let media_keys = [
                    Shortcut::new(None, Code::MediaPlayPause),
                    Shortcut::new(None, Code::MediaTrackNext),
                    Shortcut::new(None, Code::MediaTrackPrevious),
                    Shortcut::new(None, Code::MediaStop),
                ];
                for shortcut in &media_keys {
                    if let Err(e) = app.global_shortcut().register(*shortcut) {
                        log::warn!("Failed to register media key {:?}: {}", shortcut.key, e);
                    }
                }
                // User-configured keyboard shortcuts (with modifiers → global)
                let shortcut_map =
                    shortcuts::register_user_shortcuts(app.handle(), &user_bindings);
                *state.shortcut_map.lock() = shortcut_map;
            }

            // Spawn evdev mouse listener for global mouse button shortcuts (Linux).
            // Reads /dev/input/event* in non-blocking mode. Requires 'input' group.
            // Falls back gracefully if input-remapper grabs the devices.
            // Gated behind the "global-mouse" feature to allow builds without evdev overhead.
            #[cfg(all(target_os = "linux", feature = "global-mouse"))]
            {
                let mouse_state = state.clone();
                let mouse_handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("evdev-mouse".into())
                    .spawn(move || {
                        use evdev::{Device, EventType, KeyCode};

                        let mut devices: Vec<Device> = evdev::enumerate()
                            .filter_map(|(_, mut dev)| {
                                let keys = dev.supported_keys()?;
                                if keys.contains(KeyCode::BTN_SIDE)
                                    || keys.contains(KeyCode::BTN_EXTRA)
                                    || keys.contains(KeyCode::BTN_MIDDLE)
                                {
                                    let _ = dev.set_nonblocking(true);
                                    Some(dev)
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if devices.is_empty() {
                            return;
                        }

                        loop {
                            for dev in &mut devices {
                                match dev.fetch_events() {
                                    Ok(events) => {
                                        for event in events {
                                            if event.event_type() != EventType::KEY
                                                || event.value() != 1
                                            {
                                                continue;
                                            }
                                            let btn_name = match event.code() {
                                                0x112 => Some("MouseMiddle"),
                                                0x113 => Some("MouseBack"),
                                                0x114 => Some("MouseForward"),
                                                0x115 => Some("Mouse6"),
                                                0x116 => Some("Mouse7"),
                                                0x117 => Some("Mouse8"),
                                                0x118 => Some("Mouse9"),
                                                0x119 => Some("Mouse10"),
                                                _ => None,
                                            };
                                            if let Some(name) = btn_name {
                                                let bindings =
                                                    mouse_state.mouse_bindings.lock();
                                                let action = bindings
                                                    .iter()
                                                    .find(|(_, v)| {
                                                        v.as_str() == name
                                                            || v.ends_with(
                                                                &format!("+{}", name),
                                                            )
                                                    })
                                                    .map(|(k, _)| k.clone());
                                                drop(bindings);
                                                if let Some(action_id) = action {
                                                    shortcuts::handle_action(
                                                        &action_id,
                                                        &mouse_handle,
                                                        &mouse_state,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    Err(e)
                                        if e.kind()
                                            == std::io::ErrorKind::WouldBlock => {}
                                    Err(_) => {}
                                }
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    })
                    .ok();
            }

            // Spawn CGEventTap mouse listener for global mouse button shortcuts (macOS).
            // Uses Input Monitoring permission to passively listen for extra mouse buttons.
            // Gated behind the "global-mouse" feature.
            #[cfg(all(target_os = "macos", feature = "global-mouse"))]
            {
                let mouse_state = state.clone();
                let mouse_handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("macos-mouse".into())
                    .spawn(move || {
                        use core_graphics::event::{
                            CGEventTap, CGEventTapLocation, CGEventTapPlacement,
                            CGEventTapOptions, CGEventType, EventField, CallbackResult,
                        };
                        use core_foundation::runloop::CFRunLoop;

                        let result = CGEventTap::with_enabled(
                            CGEventTapLocation::Session,
                            CGEventTapPlacement::TailAppendEventTap,
                            CGEventTapOptions::ListenOnly,
                            vec![CGEventType::OtherMouseDown],
                            |_proxy, _type, event| {
                                let button = event.get_integer_value_field(
                                    EventField::MOUSE_EVENT_BUTTON_NUMBER,
                                );
                                let btn_name = match button {
                                    2 => Some("MouseMiddle"),
                                    3 => Some("MouseBack"),
                                    4 => Some("MouseForward"),
                                    5 => Some("Mouse6"),
                                    6 => Some("Mouse7"),
                                    7 => Some("Mouse8"),
                                    8 => Some("Mouse9"),
                                    9 => Some("Mouse10"),
                                    _ => None,
                                };
                                if let Some(name) = btn_name {
                                    let bindings =
                                        mouse_state.mouse_bindings.lock();
                                    let action = bindings
                                        .iter()
                                        .find(|(_, v)| {
                                            v.as_str() == name
                                                || v.ends_with(
                                                    &format!("+{}", name),
                                                )
                                        })
                                        .map(|(k, _)| k.clone());
                                    drop(bindings);
                                    if let Some(action_id) = action {
                                        shortcuts::handle_action(
                                            &action_id,
                                            &mouse_handle,
                                            &mouse_state,
                                        );
                                    }
                                }
                                CallbackResult::Keep
                            },
                            || {
                                CFRunLoop::run_current();
                            },
                        );

                        if result.is_err() {
                            log::warn!(
                                "Failed to create macOS mouse event tap — \
                                 grant Input Monitoring permission in \
                                 System Settings > Privacy & Security > Input Monitoring"
                            );
                        }
                    })
                    .ok();
            }

            // Spawn state update thread
            //
            // Uses try_lock() to never block when the audio mutex is held by a
            // slow operation (PSF seek, loading, etc.). This keeps the event loop
            // alive so the frontend stays responsive even during long operations.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut last_tooltip = String::from("Tunante");
                loop {
                    std::thread::sleep(Duration::from_millis(250));

                    let state = handle.state::<Arc<AppState>>();

                    // try_lock: skip this cycle if audio is busy (seek/load in progress)
                    let Some(audio) = state.audio.try_lock() else {
                        continue;
                    };
                    let is_playing = audio.is_playing();
                    let position_ms = audio.position_ms();
                    let duration_ms = audio.duration_ms();
                    let volume = audio.volume();
                    let track_finished = audio.track_finished();
                    drop(audio);

                    // Update tray tooltip with current track info
                    let tooltip = {
                        let queue = state.queue.lock();
                        if let Some(track) = queue.current() {
                            let status = if is_playing { "▶" } else { "⏸" };
                            if track.artist.is_empty() || track.artist == "Unknown Artist" {
                                format!("{} {}", status, track.title)
                            } else {
                                format!("{} {} – {}", status, track.artist, track.title)
                            }
                        } else {
                            "Tunante".to_string()
                        }
                    };
                    if tooltip != last_tooltip {
                        // Don't overwrite the "Volume: XX%" tooltip while it's showing
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        if now_ms >= VOLUME_TOOLTIP_UNTIL.load(Ordering::Relaxed) {
                            if let Some(tray) = handle.tray_by_id("main-tray") {
                                let _ = tray.set_tooltip(Some(&tooltip));
                            }
                            last_tooltip = tooltip;
                        }
                    }

                    let _ = handle.emit(
                        "player-state-update",
                        PlayerStatePayload {
                            is_playing,
                            position_ms,
                            duration_ms,
                            volume,
                        },
                    );

                    // Auto-advance when track finishes
                    if track_finished {
                        let mut queue = state.queue.lock();
                        if let Some(next_track) = queue.next() {
                            let path = next_track.path.clone();
                            drop(queue);
                            let mut audio = state.audio.lock();
                            match audio.play_file(&std::path::PathBuf::from(&path)) {
                                Ok(()) => {
                                    let _ = handle.emit("track-changed", next_track);
                                }
                                Err(e) => {
                                    log::error!("Failed to play {}: {}", path, e);
                                    let _ = handle.emit(
                                        "playback-error",
                                        PlaybackErrorPayload {
                                            message: e.to_string(),
                                            path: path.clone(),
                                        },
                                    );
                                    // Reset state so we don't loop on the same error
                                    audio.stop();
                                    drop(audio);
                                    let _ = handle.emit("playback-stopped", ());
                                }
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // Check if close_to_tray is enabled
                    let app_state = window.state::<Arc<AppState>>();
                    let close_to_tray = {
                        let db = app_state.db.lock();
                        db.get_setting("close_to_tray")
                            .ok()
                            .flatten()
                            .map(|v| v == "true")
                            .unwrap_or(false)
                    };

                    if close_to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                tauri::WindowEvent::ThemeChanged(theme) => {
                    // Update tray icon when OS theme changes
                    let is_light = *theme == tauri::Theme::Light;
                    let png_bytes: &[u8] = if is_light {
                        include_bytes!("../icons/tray-icon-big-black-fixed.png")
                    } else {
                        include_bytes!("../icons/tray-icon-big-fixed.png")
                    };
                    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
                    if let Ok(mut reader) = decoder.read_info() {
                        let mut buf = vec![0u8; reader.output_buffer_size()];
                        if let Ok(info) = reader.next_frame(&mut buf) {
                            buf.truncate(info.buffer_size());
                            let icon = tauri::image::Image::new_owned(buf, info.width, info.height);
                            if let Some(tray) = window.app_handle().tray_by_id("main-tray") {
                                let _ = tray.set_icon(Some(icon));
                            }
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_debug_logs,
            clear_debug_logs,
            commands::player::play_file,
            commands::player::pause,
            commands::player::resume,
            commands::player::stop,
            commands::player::seek,
            commands::player::set_volume,
            commands::player::next_track,
            commands::player::prev_track,
            commands::player::get_player_state,
            commands::player::enqueue_tracks,
            commands::player::dequeue_tracks,
            commands::player::get_queue,
            commands::player::is_in_queue,
            commands::player::set_shuffle,
            commands::player::set_repeat,
            commands::library::get_all_tracks,
            commands::library::set_track_rating,
            commands::library::get_faved_tracks,
            commands::library::scan_folder,
            commands::library::add_files,
            commands::library::get_artwork,
            commands::library::fetch_cover_art,
            commands::library::fetch_vgm_cover_art,
            commands::playlists::get_playlists,
            commands::playlists::get_playlist_tracks,
            commands::playlists::create_playlist,
            commands::playlists::delete_playlist,
            commands::playlists::rename_playlist,
            commands::playlists::reorder_playlists,
            commands::playlists::add_tracks_to_playlist,
            commands::playlists::remove_track_from_playlist,
            commands::playlists::create_playlist_from_folder,
            commands::library::is_directory,
            commands::settings::get_settings,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_monitored_folders,
            commands::settings::add_monitored_folder,
            commands::settings::remove_monitored_folder,
            commands::settings::toggle_folder_watching,
            commands::settings::set_tray_visible,
            commands::settings::get_desktop_entry_path,
            commands::settings::create_desktop_entry,
            commands::library::open_containing_folder,
            commands::library::open_folder,
            commands::library::resync_library,
            commands::library::update_track_metadata,
            shortcuts::update_shortcuts,
            shortcuts::get_shortcuts,
            run_macos_update_script,
            updater::check_for_updates,
            updater::download_and_apply_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS: clicking the dock icon while the window is hidden sends Reopen
            // (the single-instance plugin doesn't fire because no second process is launched)
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows, .. } = event {
                if !has_visible_windows {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            }
        });
}
// Auto-updater test bump 1774267730
