use crate::db::models::Track;
use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, State};

#[derive(Clone, serde::Serialize)]
struct PlaybackErrorPayload {
    message: String,
    path: String,
}

#[tauri::command]
pub fn play_file(path: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let file_path = PathBuf::from(&path);

    // Load tracks into queue from library
    let db = state.db.lock();
    let all_tracks = db.get_all_tracks().map_err(|e| e.to_string())?;
    drop(db);

    let mut queue = state.queue.lock();
    queue.set_tracks(all_tracks);
    queue.play_track_by_id(
        &state
            .db
            .lock()
            .get_track_by_path(&path)
            .map_err(|e| e.to_string())?
            .map(|t| t.id)
            .unwrap_or_default(),
    );
    drop(queue);

    let mut audio = state.audio.lock();
    audio.play_file(&file_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pause(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.audio.lock().pause();
    Ok(())
}

#[tauri::command]
pub fn resume(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.audio.lock().resume();
    Ok(())
}

#[tauri::command]
pub fn stop(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.audio.lock().stop();
    Ok(())
}

/// Seek command — runs the seek on a background thread so the UI stays responsive.
///
/// PSF seek involves fast-forwarding the PS1 CPU emulator to the target position,
/// which can take seconds for far seeks. By spawning a thread, the command returns
/// immediately and the frontend gets an optimistic update. If the seek fails, a
/// `playback-error` event is emitted to show a toast.
#[tauri::command]
pub fn seek(
    position_ms: u64,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let state = state.inner().clone();

    std::thread::spawn(move || {
        let mut audio = state.audio.lock();
        if let Err(e) = audio.seek(position_ms) {
            log::error!("Seek failed: {}", e);
            let _ = app.emit(
                "playback-error",
                PlaybackErrorPayload {
                    message: format!("Seek failed: {}", e),
                    path: String::new(),
                },
            );
        }
    });

    Ok(())
}

#[tauri::command]
pub fn set_volume(volume: f32, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.audio.lock().set_volume(volume);
    Ok(())
}

#[tauri::command]
pub fn next_track(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut queue = state.queue.lock();
    if let Some(track) = queue.next() {
        let path = track.path.clone();
        let _ = app.emit("track-changed", &track);
        drop(queue);
        let mut audio = state.audio.lock();
        audio.play_file(&PathBuf::from(&path)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn prev_track(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut queue = state.queue.lock();
    if let Some(track) = queue.prev() {
        let path = track.path.clone();
        let _ = app.emit("track-changed", &track);
        drop(queue);
        let mut audio = state.audio.lock();
        audio.play_file(&PathBuf::from(&path)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_player_state(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let audio = state.audio.lock();
    let queue = state.queue.lock();

    Ok(serde_json::json!({
        "is_playing": audio.is_playing(),
        "position_ms": audio.position_ms(),
        "duration_ms": audio.duration_ms(),
        "volume": audio.volume(),
        "current_track": queue.current(),
    }))
}

#[tauri::command]
pub fn enqueue_tracks(
    track_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let db = state.db.lock();
    let mut queue = state.queue.lock();
    for id in track_ids {
        if let Ok(Some(track)) = db.get_track_by_id(&id) {
            queue.enqueue_track(track);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn dequeue_tracks(
    track_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut queue = state.queue.lock();
    for id in track_ids {
        queue.dequeue_track(&id);
    }
    Ok(())
}

#[tauri::command]
pub fn get_queue(state: State<'_, Arc<AppState>>) -> Result<Vec<Track>, String> {
    let queue = state.queue.lock();
    Ok(queue.get_user_queue().to_vec())
}

#[tauri::command]
pub fn is_in_queue(track_id: String, state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    let queue = state.queue.lock();
    Ok(queue.is_in_user_queue(&track_id))
}
