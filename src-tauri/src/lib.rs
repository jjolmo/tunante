use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

pub mod audio;
pub mod commands;
pub mod db;
pub mod metadata;
pub mod shortcuts;
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

/// Installs a D-Bus message filter on the session bus that handles the
/// StatusNotifierItem `Activate` method call (sent by KDE on left-click).
///
/// libayatana-appindicator 0.5.x does not implement `Activate` in its D-Bus
/// interface, so KDE falls back to showing the context menu on left-click.
/// This filter intercepts `Activate`, toggles the main window, and sends a
/// success reply before the "no such method" error is synthesised.
#[cfg(target_os = "linux")]
fn setup_dbus_activate_handler(handle: tauri::AppHandle) {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        log::warn!("Could not get D-Bus session bus for Activate handler");
        return;
    };

    connection.add_filter(move |conn, msg, incoming| {
        if !incoming {
            return Some(msg.to_owned());
        }

        let is_activate = msg.message_type() == gio::DBusMessageType::MethodCall
            && msg.member().as_deref() == Some("Activate")
            && msg.interface().as_deref() == Some("org.kde.StatusNotifierItem");

        if !is_activate {
            return Some(msg.to_owned());
        }

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

        // Send a success reply so KDE knows the call was handled.
        // (Returning None causes GLib to auto-synthesise an error reply,
        // but D-Bus ignores duplicate replies for the same serial.)
        let reply = msg.to_owned();
        let method_reply = gio::DBusMessage::new_method_reply(&reply);
        let _ = conn.send_message(&method_reply, gio::DBusSendMessageFlags::NONE);

        // Consume the message so normal dispatch doesn't see it.
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

pub fn run() {
    setup_panic_hook();

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
                    .unwrap_or(true)
            };

            let tray_builder = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
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
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { button, .. } = event {
                        let app = tray.app_handle();
                        match button {
                            tauri::tray::MouseButton::Left => {
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
                })
                .build(app)?;

            // Apply initial visibility
            let _ = tray.set_visible(show_in_tray);

            // Linux: install a D-Bus message filter to handle the `Activate`
            // method on the StatusNotifierItem interface.
            //
            // libayatana-appindicator 0.5.x does NOT expose the `Activate`
            // D-Bus method (only `SecondaryActivate` and `Scroll`). When KDE
            // Plasma sends `Activate` on left-click, the call fails and KDE
            // falls back to showing the context menu.
            //
            // Our filter intercepts the incoming `Activate` method call,
            // toggles the main window, and sends a success reply so KDE
            // knows the action was handled.
            #[cfg(target_os = "linux")]
            {
                setup_dbus_activate_handler(app.handle().clone());
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

            // NOTE: Global mouse button shortcuts are handled via the keyboard
            // shortcut system. Users with extra mouse buttons should configure
            // their input remapper (input-remapper, xbindkeys, etc.) to emit
            // keyboard combos (e.g. Ctrl+Alt+1), then bind those combos in
            // Tunante's Shortcuts settings. This works globally even when
            // the app is minimized.

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
                        if let Some(tray) = handle.tray_by_id("main-tray") {
                            let _ = tray.set_tooltip(Some(&tooltip));
                        }
                        last_tooltip = tooltip;
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
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
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
        })
        .invoke_handler(tauri::generate_handler![
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
            commands::playlists::get_playlists,
            commands::playlists::get_playlist_tracks,
            commands::playlists::create_playlist,
            commands::playlists::delete_playlist,
            commands::playlists::rename_playlist,
            commands::playlists::add_tracks_to_playlist,
            commands::playlists::remove_track_from_playlist,
            commands::settings::get_settings,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_monitored_folders,
            commands::settings::add_monitored_folder,
            commands::settings::remove_monitored_folder,
            commands::settings::toggle_folder_watching,
            commands::settings::set_tray_visible,
            commands::library::open_containing_folder,
            commands::library::resync_library,
            commands::library::update_track_metadata,
            shortcuts::update_shortcuts,
            shortcuts::get_shortcuts,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
