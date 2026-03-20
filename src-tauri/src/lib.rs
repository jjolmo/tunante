use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};

pub mod audio;
pub mod commands;
pub mod db;
pub mod metadata;

pub struct AppState {
    pub audio: parking_lot::Mutex<audio::AudioEngine>,
    pub db: parking_lot::Mutex<db::Database>,
    pub queue: parking_lot::Mutex<audio::PlayQueue>,
}

#[derive(Clone, serde::Serialize)]
struct PlayerStatePayload {
    is_playing: bool,
    position_ms: u64,
    duration_ms: u64,
    volume: f32,
}

pub fn run() {
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
            });

            app.manage(state.clone());

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
                            if let Err(e) = audio.play_file(&std::path::PathBuf::from(&path)) {
                                log::error!("Failed to auto-advance: {}", e);
                            }
                            let _ = handle.emit("track-changed", next_track);
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
