use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

pub mod audio;
pub mod commands;
pub mod db;
pub mod metadata;
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

pub fn run() {
    setup_panic_hook();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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

            let state = Arc::new(AppState {
                audio: parking_lot::Mutex::new(audio_engine),
                db: parking_lot::Mutex::new(db),
                queue: parking_lot::Mutex::new(queue),
                watcher: parking_lot::Mutex::new(None),
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
            let show_item = MenuItemBuilder::with_id("show", "Show Tunante").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&quit_item)
                .build()?;

            // Read initial tray visibility from settings
            let show_in_tray = {
                let db = state.db.lock();
                db.get_setting("show_in_tray")
                    .ok()
                    .flatten()
                    .map(|v| v == "true")
                    .unwrap_or(false)
            };

            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Tunante")
                .menu(&tray_menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
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
                                // Left click: show and focus window
                                if let Some(window) = app.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.unminimize();
                                    let _ = window.set_focus();
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
