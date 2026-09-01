//! Playback control: the queue, the fade orchestrator, output selection and
//! the DSP chain. Bodies extracted verbatim from `commands/player.rs`.

use crate::audio::RepeatMode;
use crate::db::models::Track;
use crate::events::{AppEvent, Events, PlaybackErrorPayload};
use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const FADE_TICK_MS: u64 = 25;

/// Whether a play request should cross-fade rather than start immediately.
///
/// A fade is a *transition*: the outgoing track ramps down and the incoming one ramps up
/// to meet it. That only makes sense when something is currently audible. Starting
/// playback from stopped, picking a track after the previous one ended, or choosing one
/// while paused have nothing to fade out of, and the fade-in half on its own is just an
/// unrequested ramp on a track the user asked to start.
fn should_fade(fade_enabled: bool, fade_seconds: f32, is_playing: bool) -> bool {
    fade_enabled && fade_seconds > 0.0 && is_playing
}

/// Play a file, optionally with a fade-out of the current track and a fade-in
/// of the new one. The fade is performed entirely in Rust without touching the
/// user-visible volume, so the UI volume slider stays at its current value.
///
/// When fade is disabled, this behaves the same as calling `audio.play_file`
/// directly. When enabled, work is done on a background thread; this function
/// returns immediately.
pub fn play_with_fade(
    state: Arc<AppState>,
    events: Events,
    path: String,
    duration_hint_ms: i64,
    track_for_event: Option<Track>,
) {
    play_with_fade_opts(state, events, path, duration_hint_ms, track_for_event, false);
}

/// Like [`play_with_fade`] but with a `force_fade` flag that requests a fade
/// transition regardless of the user's `fade_on_track_change` setting. Used by
/// on-demand actions (e.g. tray middle-click → "Next Song with fade").
pub fn play_with_fade_opts(
    state: Arc<AppState>,
    events: Events,
    path: String,
    duration_hint_ms: i64,
    track_for_event: Option<Track>,
    force_fade: bool,
) {
    let (cfg_fade, fade_seconds, is_playing) = {
        let audio = state.audio.lock();
        (
            audio.fade_on_track_change(),
            audio.fade_seconds(),
            audio.is_playing(),
        )
    };
    if !should_fade(force_fade || cfg_fade, fade_seconds, is_playing) {
        let mut audio = state.audio.lock();
        match audio.play_file(&PathBuf::from(&path), duration_hint_ms) {
            Ok(()) => {
                drop(audio);
                if let Some(t) = track_for_event {
                    events.emit(AppEvent::TrackChanged(t));
                }
            }
            Err(e) => {
                drop(audio);
                events.emit(AppEvent::PlaybackError(PlaybackErrorPayload {
                    message: e.to_string(),
                    path: path.clone(),
                }));
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

        let is_current = |gen_id: u64| -> bool { state.audio.lock().fade_generation() == gen_id };

        // Fade the outgoing track out. Reaching here means something was audible.
        for i in 1..=steps {
            if !is_current(generation) {
                return;
            }
            let factor = 1.0 - (i as f32 / steps as f32);
            let user_vol = state.audio.lock().volume();
            state.audio.lock().set_player_volume_raw(user_vol * factor);
            std::thread::sleep(tick);
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
                        events.emit(AppEvent::TrackChanged(t.clone()));
                    }
                }
                Err(e) => {
                    drop(audio);
                    events.emit(AppEvent::PlaybackError(PlaybackErrorPayload {
                        message: e.to_string(),
                        path: path.clone(),
                    }));
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
pub fn play_file(
    state: &Arc<AppState>,
    events: Events,
    path: String,
    track_ids: Option<Vec<String>>,
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

    play_with_fade(state.clone(), events, path, duration_hint_ms, db_track);
    Ok(())
}

pub fn pause(state: &AppState) {
    state.audio.lock().pause();
}

pub fn resume(state: &AppState) {
    state.audio.lock().resume();
}

pub fn stop(state: &AppState) {
    state.audio.lock().stop();
}

/// Seek — runs on a background thread so the UI stays responsive.
///
/// PSF seek involves fast-forwarding the PS1 CPU emulator to the target position,
/// which can take seconds for far seeks. By spawning a thread, this returns
/// immediately and the frontend gets an optimistic update. If the seek fails, a
/// `playback-error` event is emitted to show a toast.
pub fn seek(state: &Arc<AppState>, events: Events, position_ms: u64) {
    let state = state.clone();

    std::thread::spawn(move || {
        let mut audio = state.audio.lock();
        if let Err(e) = audio.seek(position_ms) {
            log::error!("Seek failed: {}", e);
            events.emit(AppEvent::PlaybackError(PlaybackErrorPayload {
                message: format!("Seek failed: {}", e),
                path: String::new(),
            }));
        }
    });
}

pub fn set_volume(state: &AppState, volume: f32) {
    state.audio.lock().set_volume(volume);
}

pub fn next_track(state: &Arc<AppState>, events: Events) {
    let mut queue = state.queue.lock();
    if let Some(track) = queue.next() {
        let path = track.path.clone();
        let duration_hint = track.duration_ms;
        let track_clone = track.clone();
        drop(queue);
        play_with_fade(state.clone(), events, path, duration_hint, Some(track_clone));
    }
}

pub fn prev_track(state: &Arc<AppState>, events: Events) {
    let mut queue = state.queue.lock();
    if let Some(track) = queue.prev() {
        let path = track.path.clone();
        let duration_hint = track.duration_ms;
        let track_clone = track.clone();
        drop(queue);
        play_with_fade(state.clone(), events, path, duration_hint, Some(track_clone));
    }
}

pub fn get_player_state(state: &AppState) -> serde_json::Value {
    let audio = state.audio.lock();
    let queue = state.queue.lock();

    serde_json::json!({
        "is_playing": audio.is_playing(),
        "position_ms": audio.position_ms(),
        "duration_ms": audio.duration_ms(),
        "volume": audio.volume(),
        "current_track": queue.current(),
    })
}

pub fn enqueue_tracks(state: &AppState, track_ids: Vec<String>) {
    let db = state.db.lock();
    let mut queue = state.queue.lock();
    for id in track_ids {
        if let Ok(Some(track)) = db.get_track_by_id(&id) {
            queue.enqueue_track(track);
        }
    }
}

pub fn dequeue_tracks(state: &AppState, track_ids: Vec<String>) {
    let mut queue = state.queue.lock();
    for id in track_ids {
        queue.dequeue_track(&id);
    }
}

pub fn get_queue(state: &AppState) -> Vec<Track> {
    state.queue.lock().get_user_queue().to_vec()
}

pub fn is_in_queue(state: &AppState, track_id: &str) -> bool {
    state.queue.lock().is_in_user_queue(track_id)
}

pub fn set_shuffle(state: &AppState, enabled: bool) {
    state.queue.lock().set_shuffle(enabled);
}

pub fn set_continue_from_queue(state: &AppState, enabled: bool) {
    state.queue.lock().set_continue_from_queue(enabled);
}

pub fn set_short_filter(state: &AppState, enabled: bool, threshold_sec: i64) {
    let threshold_ms = if enabled && threshold_sec > 0 {
        threshold_sec * 1000
    } else {
        0
    };
    state.queue.lock().set_short_filter(threshold_ms);
}

pub fn set_repeat(state: &AppState, mode: &str) {
    let repeat = match mode {
        "all" => RepeatMode::All,
        "one" => RepeatMode::One,
        _ => RepeatMode::Off,
    };
    state.queue.lock().set_repeat(repeat);
}

pub fn set_fade_on_track_change(state: &AppState, enabled: bool) {
    state.audio.lock().set_fade_on_track_change(enabled);
}

pub fn set_vgm_loop_count(state: &AppState, count: f64) {
    state.audio.lock().set_vgm_loop_count(count);
}

pub fn set_fade_seconds(state: &AppState, seconds: f32) {
    state.audio.lock().set_fade_seconds(seconds);
}

// ---- Audio output device ----

/// The persisted output selection: "system" or a specific device name.
pub fn get_audio_output(state: &AppState) -> Result<String, String> {
    let stored = state
        .db
        .lock()
        .get_setting("audio_output_device")
        .map_err(|e| e.to_string())?;
    Ok(stored.unwrap_or_else(|| "system".to_string()))
}

/// Persist and apply a new output selection. "system" (or empty) follows the OS
/// default; any other value selects that device by name. Applies immediately,
/// preserving the current track and position.
pub fn set_audio_output(state: &AppState, selection: &str) -> Result<(), String> {
    let sel = crate::audio::OutputSelection::from_setting(selection);
    state
        .db
        .lock()
        .set_setting("audio_output_device", &sel.to_setting())
        .map_err(|e| e.to_string())?;
    state
        .audio
        .lock()
        .set_output_selection(sel)
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// DSP chain
// ---------------------------------------------------------------------------

/// The whole DSP chain state, as sent to and from the UI.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct DspConfig {
    pub mono: bool,
    pub mono_compensate: bool,
    pub mono_phase_safe: bool,
    pub balance: f32,
    pub width_enabled: bool,
    pub width: f32,
    pub preamp_enabled: bool,
    pub preamp_db: f32,
    pub eq_enabled: bool,
    pub eq_low_db: f32,
    pub eq_mid_db: f32,
    pub eq_high_db: f32,
    pub limiter: bool,
}

/// Apply a whole DSP configuration at once.
///
/// Every value goes into an atomic the audio thread reads per frame, so this
/// takes effect on the track that is already playing — no restart, no gap.
pub fn set_dsp_config(state: &AppState, config: &DspConfig) {
    let audio = state.audio.lock();
    let dsp = audio.dsp();
    dsp.mono.set(config.mono);
    dsp.mono_compensate.set(config.mono_compensate);
    dsp.mono_phase_safe.set(config.mono_phase_safe);
    dsp.balance.set(config.balance.clamp(-1.0, 1.0));
    dsp.width_enabled.set(config.width_enabled);
    dsp.width.set(config.width.clamp(0.0, 2.0));
    dsp.preamp_enabled.set(config.preamp_enabled);
    dsp.preamp_db.set(config.preamp_db.clamp(-20.0, 20.0));
    dsp.eq_enabled.set(config.eq_enabled);
    dsp.eq_low_db.set(config.eq_low_db.clamp(-20.0, 20.0));
    dsp.eq_mid_db.set(config.eq_mid_db.clamp(-20.0, 20.0));
    dsp.eq_high_db.set(config.eq_high_db.clamp(-20.0, 20.0));
    dsp.limiter.set(config.limiter);
}

/// Read back the chain state, so the UI can't drift from the engine.
pub fn get_dsp_config(state: &AppState) -> DspConfig {
    let audio = state.audio.lock();
    let dsp = audio.dsp();
    DspConfig {
        mono: dsp.mono.get(),
        mono_compensate: dsp.mono_compensate.get(),
        mono_phase_safe: dsp.mono_phase_safe.get(),
        balance: dsp.balance.get(),
        width_enabled: dsp.width_enabled.get(),
        width: dsp.width.get(),
        preamp_enabled: dsp.preamp_enabled.get(),
        preamp_db: dsp.preamp_db.get(),
        eq_enabled: dsp.eq_enabled.get(),
        eq_low_db: dsp.eq_low_db.get(),
        eq_mid_db: dsp.eq_mid_db.get(),
        eq_high_db: dsp.eq_high_db.get(),
        limiter: dsp.limiter.get(),
    }
}

#[cfg(test)]
mod tests {
    use super::should_fade;

    /// The fade setting is "fade out / fade in **on track change**". Pressing play from a
    /// stopped player is not a track change, so it must start at full volume. This used to
    /// fade in regardless: the gate was `has_source`, which skipped only the fade-*out*
    /// half, leaving every cold start with an unrequested ramp-up.
    #[test]
    fn cold_start_does_not_fade() {
        assert!(
            !should_fade(true, 2.0, false),
            "starting playback with nothing audible must not fade in"
        );
        // Same story once a track has ended, or while paused — `is_playing` covers both,
        // and neither has anything to fade out of.
    }

    /// Swapping tracks mid-playback is the case the option exists for.
    #[test]
    fn track_change_while_playing_fades() {
        assert!(should_fade(true, 2.0, true));
    }

    /// Turning the option off, or setting the duration to zero, disables it either way.
    #[test]
    fn disabled_or_zero_duration_never_fades() {
        assert!(!should_fade(false, 2.0, true));
        assert!(!should_fade(true, 0.0, true));
        assert!(!should_fade(true, -1.0, true));
    }
}
