use std::sync::Arc;
use std::time::Duration;
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

        // Write crash log next to the executable or in /tmp
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

            // Spawn state update thread
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(250));

                    let state = handle.state::<Arc<AppState>>();
                    let audio = state.audio.lock();
                    let is_playing = audio.is_playing();
                    let position_ms = audio.position_ms();
                    let duration_ms = audio.duration_ms();
                    let volume = audio.volume();
                    let track_finished = audio.track_finished();
                    drop(audio);

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
