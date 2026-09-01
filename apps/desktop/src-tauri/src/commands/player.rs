//! Tauri shells over `services::player` — the bodies live there.

use crate::db::models::Track;
use crate::events::tauri_events;
use crate::services::player as svc;
use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, State};

pub use crate::services::player::DspConfig;
// lib.rs builds this payload for the polling thread's error path.
pub(crate) use crate::events::PlaybackErrorPayload;

/// Compat wrappers for callers that still hold an `AppHandle` (lib.rs, the
/// shortcut handler); removed when those move onto `Events` themselves.
pub fn play_with_fade(
    state: Arc<AppState>,
    app: AppHandle,
    path: String,
    duration_hint_ms: i64,
    track_for_event: Option<Track>,
) {
    svc::play_with_fade(state, tauri_events(app), path, duration_hint_ms, track_for_event);
}

pub fn play_with_fade_opts(
    state: Arc<AppState>,
    app: AppHandle,
    path: String,
    duration_hint_ms: i64,
    track_for_event: Option<Track>,
    force_fade: bool,
) {
    svc::play_with_fade_opts(
        state,
        tauri_events(app),
        path,
        duration_hint_ms,
        track_for_event,
        force_fade,
    );
}

#[tauri::command]
pub fn play_file(
    path: String,
    track_ids: Option<Vec<String>>,
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    svc::play_file(&state, tauri_events(app), path, track_ids)
}

#[tauri::command]
pub fn pause(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::pause(&state);
    Ok(())
}

#[tauri::command]
pub fn resume(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::resume(&state);
    Ok(())
}

#[tauri::command]
pub fn stop(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::stop(&state);
    Ok(())
}

#[tauri::command]
pub fn seek(
    position_ms: u64,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    svc::seek(&state, tauri_events(app), position_ms);
    Ok(())
}

#[tauri::command]
pub fn set_volume(volume: f32, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::set_volume(&state, volume);
    Ok(())
}

#[tauri::command]
pub fn next_track(state: State<'_, Arc<AppState>>, app: tauri::AppHandle) -> Result<(), String> {
    svc::next_track(&state, tauri_events(app));
    Ok(())
}

#[tauri::command]
pub fn prev_track(state: State<'_, Arc<AppState>>, app: tauri::AppHandle) -> Result<(), String> {
    svc::prev_track(&state, tauri_events(app));
    Ok(())
}

#[tauri::command]
pub fn get_player_state(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    Ok(svc::get_player_state(&state))
}

#[tauri::command]
pub fn enqueue_tracks(
    track_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::enqueue_tracks(&state, track_ids);
    Ok(())
}

#[tauri::command]
pub fn dequeue_tracks(
    track_ids: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::dequeue_tracks(&state, track_ids);
    Ok(())
}

#[tauri::command]
pub fn get_queue(state: State<'_, Arc<AppState>>) -> Result<Vec<Track>, String> {
    Ok(svc::get_queue(&state))
}

#[tauri::command]
pub fn is_in_queue(track_id: String, state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(svc::is_in_queue(&state, &track_id))
}

#[tauri::command]
pub fn set_shuffle(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::set_shuffle(&state, enabled);
    Ok(())
}

#[tauri::command]
pub fn set_continue_from_queue(
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::set_continue_from_queue(&state, enabled);
    Ok(())
}

#[tauri::command]
pub fn set_short_filter(
    enabled: bool,
    threshold_sec: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::set_short_filter(&state, enabled, threshold_sec);
    Ok(())
}

#[tauri::command]
pub fn set_repeat(mode: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::set_repeat(&state, &mode);
    Ok(())
}

#[tauri::command]
pub fn set_fade_on_track_change(
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    svc::set_fade_on_track_change(&state, enabled);
    Ok(())
}

#[tauri::command]
pub fn set_vgm_loop_count(count: f64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::set_vgm_loop_count(&state, count);
    Ok(())
}

#[tauri::command]
pub fn set_fade_seconds(seconds: f32, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::set_fade_seconds(&state, seconds);
    Ok(())
}

#[tauri::command]
pub fn list_audio_outputs() -> Result<Vec<String>, String> {
    Ok(crate::audio::list_output_devices())
}

#[tauri::command]
pub fn get_audio_output(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    svc::get_audio_output(&state)
}

#[tauri::command]
pub fn set_audio_output(selection: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::set_audio_output(&state, &selection)
}

#[tauri::command]
pub fn set_dsp_config(config: DspConfig, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    svc::set_dsp_config(&state, &config);
    Ok(())
}

#[tauri::command]
pub fn get_dsp_config(state: State<'_, Arc<AppState>>) -> Result<DspConfig, String> {
    Ok(svc::get_dsp_config(&state))
}

#[tauri::command]
pub fn list_dsp_processors() -> Vec<String> {
    crate::audio::DspSettings::processor_ids()
        .iter()
        .map(|s| s.to_string())
        .collect()
}
