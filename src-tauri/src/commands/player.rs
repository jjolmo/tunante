use crate::audio::RepeatMode;
use crate::db::models::Track;
use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, serde::Serialize)]
pub(crate) struct PlaybackErrorPayload {
    pub message: String,
    pub path: String,
}

const FADE_TICK_MS: u64 = 25;

/// Play a file, optionally with a fade-out of the current track and a fade-in
/// of the new one. The fade is performed entirely in Rust without touching the
/// user-visible volume, so the UI volume slider stays at its current value.
///
/// When fade is disabled, this behaves the same as calling `audio.play_file`
/// directly. When enabled, work is done on a background thread; this function
/// returns immediately.
pub fn play_with_fade(
    state: Arc<AppState>,
    app: AppHandle,
    path: String,
    duration_hint_ms: i64,
    track_for_event: Option<Track>,
) {
    play_with_fade_opts(state, app, path, duration_hint_ms, track_for_event, false);
}

/// Like [`play_with_fade`] but with a `force_fade` flag that requests a fade
/// transition regardless of the user's `fade_on_track_change` setting. Used by
/// on-demand actions (e.g. tray middle-click → "Next Song with fade").
pub fn play_with_fade_opts(
    state: Arc<AppState>,
    app: AppHandle,
    path: String,
    duration_hint_ms: i64,
    track_for_event: Option<Track>,
    force_fade: bool,
) {
    let (cfg_fade, fade_seconds, has_source) = {
        let audio = state.audio.lock();
        (
            audio.fade_on_track_change(),
            audio.fade_seconds(),
            audio.has_source(),
        )
    };
    let fade_enabled = force_fade || cfg_fade;

    if !fade_enabled || fade_seconds <= 0.0 {
        let mut audio = state.audio.lock();
        match audio.play_file(&PathBuf::from(&path), duration_hint_ms) {
            Ok(()) => {
                drop(audio);
                if let Some(t) = track_for_event {
                    let _ = app.emit("track-changed", &t);
                }
            }
            Err(e) => {
                drop(audio);
                let _ = app.emit(
                    "playback-error",
                    PlaybackErrorPayload {
                        message: e.to_string(),
                        path: path.clone(),
                    },
                );
            }
        }
        return;
    }

    std::thread::spawn(move || {
        let generation = state.audio.lock().bump_fade_generation();

        let half_secs = (fade_seconds / 2.0).max(0.0);
        let half_ms = (half_secs * 1000.0) as u64;
        let steps = (half_ms / FADE_TICK_MS).max(1);
        let tick = Duration::from_millis(FADE_TICK_MS);

        let is_current = |gen_id: u64| -> bool {
            state.audio.lock().fade_generation() == gen_id
        };

        if has_source {
            for i in 1..=steps {
                if !is_current(generation) {
                    return;
                }
                let factor = 1.0 - (i as f32 / steps as f32);
                let user_vol = state.audio.lock().volume();
                state.audio.lock().set_player_volume_raw(user_vol * factor);
                std::thread::sleep(tick);
            }
        }

        if !is_current(generation) {
            return;
        }

        {
            let mut audio = state.audio.lock();
            match audio.play_file_at_volume(&PathBuf::from(&path), duration_hint_ms, 0.0) {
                Ok(()) => {
                    drop(audio);
                    if let Some(t) = &track_for_event {
                        let _ = app.emit("track-changed", t);
                    }
                }
                Err(e) => {
                    drop(audio);
                    let _ = app.emit(
                        "playback-error",
                        PlaybackErrorPayload {
                            message: e.to_string(),
                            path: path.clone(),
                        },
                    );
                    return;
                }
            }
        }

        for i in 1..=steps {
            if !is_current(generation) {
                return;
            }
            let factor = i as f32 / steps as f32;
            let user_vol = state.audio.lock().volume();
            state.audio.lock().set_player_volume_raw(user_vol * factor);
            std::thread::sleep(tick);
        }

        if is_current(generation) {
            let user_vol = state.audio.lock().volume();
            state.audio.lock().set_player_volume_raw(user_vol);
        }
    });
}

/// Play a file. If `track_ids` is provided, those tracks become the queue context
/// (for context-aware auto-advance). Otherwise, all library tracks are used.
#[tauri::command]
pub fn play_file(
    path: String,
    track_ids: Option<Vec<String>>,
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    // Load context tracks into queue
    let db = state.db.lock();
    let context_tracks = if let Some(ids) = track_ids {
        db.get_tracks_by_ids(&ids).map_err(|e| e.to_string())?
    } else {
        db.get_all_tracks().map_err(|e| e.to_string())?
    };

    let db_track = db.get_track_by_path(&path).map_err(|e| e.to_string())?;
    let track_id = db_track.as_ref().map(|t| t.id.clone()).unwrap_or_default();
    let duration_hint_ms = db_track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
    drop(db);

    let mut queue = state.queue.lock();
    queue.set_tracks(context_tracks);
    queue.play_track_by_id(&track_id);
    drop(queue);

    play_with_fade(state.inner().clone(), app, path, duration_hint_ms, db_track);
    Ok(())
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
pub fn next_track(state: State<'_, Arc<AppState>>, app: tauri::AppHandle) -> Result<(), String> {
    let mut queue = state.queue.lock();
    if let Some(track) = queue.next() {
        let path = track.path.clone();
        let duration_hint = track.duration_ms;
        let track_clone = track.clone();
        drop(queue);
        play_with_fade(state.inner().clone(), app, path, duration_hint, Some(track_clone));
    }
    Ok(())
}

#[tauri::command]
pub fn prev_track(state: State<'_, Arc<AppState>>, app: tauri::AppHandle) -> Result<(), String> {
    let mut queue = state.queue.lock();
    if let Some(track) = queue.prev() {
        let path = track.path.clone();
        let duration_hint = track.duration_ms;
        let track_clone = track.clone();
        drop(queue);
        play_with_fade(state.inner().clone(), app, path, duration_hint, Some(track_clone));
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

#[tauri::command]
pub fn set_shuffle(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.queue.lock().set_shuffle(enabled);
    Ok(())
}

#[tauri::command]
pub fn set_continue_from_queue(
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.queue.lock().set_continue_from_queue(enabled);
    Ok(())
}

#[tauri::command]
pub fn set_short_filter(
    enabled: bool,
    threshold_sec: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let threshold_ms = if enabled && threshold_sec > 0 {
        threshold_sec * 1000
    } else {
        0
    };
    state.queue.lock().set_short_filter(threshold_ms);
    Ok(())
}

#[tauri::command]
pub fn set_repeat(mode: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let repeat = match mode.as_str() {
        "all" => RepeatMode::All,
        "one" => RepeatMode::One,
        _ => RepeatMode::Off,
    };
    state.queue.lock().set_repeat(repeat);
    Ok(())
}

#[tauri::command]
pub fn set_fade_on_track_change(
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.audio.lock().set_fade_on_track_change(enabled);
    Ok(())
}

#[tauri::command]
pub fn set_fade_seconds(seconds: f32, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.audio.lock().set_fade_seconds(seconds);
    Ok(())
}
