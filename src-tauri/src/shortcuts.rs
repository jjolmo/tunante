use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use crate::AppState;

/// All configurable shortcut action IDs.
pub const ACTION_IDS: &[(&str, &str)] = &[
    ("play_pause", "Play / Pause"),
    ("stop", "Stop"),
    ("prev_track", "Previous Track"),
    ("next_track", "Next Track"),
    ("next_track_with_fade", "Next Track (with fade)"),
    ("volume_up", "Volume Up"),
    ("volume_down", "Volume Down"),
    ("mute", "Mute / Unmute"),
    ("toggle_shuffle", "Toggle Shuffle"),
    ("cycle_repeat", "Cycle Repeat"),
    ("focus_search", "Focus Search"),
    ("toggle_fav", "Toggle Favorite"),
];

/// Execute a shortcut action by ID.
pub fn handle_action(action_id: &str, app: &AppHandle, state: &Arc<AppState>) {
    match action_id {
        "play_pause" => {
            let mut audio = state.audio.lock();
            if audio.is_playing() {
                audio.pause();
            } else {
                audio.resume();
            }
        }
        "stop" => {
            let mut audio = state.audio.lock();
            audio.stop();
            let _ = app.emit("playback-stopped", ());
        }
        "next_track" => {
            let mut queue = state.queue.lock();
            let track = queue.next().or_else(|| queue.current().cloned());
            if let Some(track) = track {
                let path = track.path.clone();
                let duration_hint = track.duration_ms;
                drop(queue);
                let mut audio = state.audio.lock();
                if let Ok(()) = audio.play_file(&std::path::PathBuf::from(&path), duration_hint) {
                    let _ = app.emit("track-changed", track);
                }
            }
        }
        "next_track_with_fade" => {
            let mut queue = state.queue.lock();
            let track = queue.next().or_else(|| queue.current().cloned());
            if let Some(track) = track {
                let path = track.path.clone();
                let duration_hint = track.duration_ms;
                drop(queue);
                crate::commands::player::play_with_fade_opts(
                    state.clone(),
                    app.clone(),
                    path,
                    duration_hint,
                    Some(track),
                    true,
                );
            }
        }
        "prev_track" => {
            let mut queue = state.queue.lock();
            let track = queue.prev().or_else(|| queue.current().cloned());
            if let Some(track) = track {
                let path = track.path.clone();
                let duration_hint = track.duration_ms;
                drop(queue);
                let mut audio = state.audio.lock();
                if let Ok(()) = audio.play_file(&std::path::PathBuf::from(&path), duration_hint) {
                    let _ = app.emit("track-changed", track);
                }
            }
        }
        "volume_up" => {
            let mut audio = state.audio.lock();
            let vol = (audio.volume() + 0.05).min(1.0);
            audio.set_volume(vol);
        }
        "volume_down" => {
            let mut audio = state.audio.lock();
            let vol = (audio.volume() - 0.05).max(0.0);
            audio.set_volume(vol);
        }
        "mute" => {
            let mut audio = state.audio.lock();
            if audio.volume() > 0.0 {
                audio.set_volume(0.0);
            } else {
                audio.set_volume(0.8);
            }
        }
        "toggle_shuffle" | "cycle_repeat" | "focus_search" | "toggle_fav" => {
            // These are frontend-only actions — emit an event for the frontend to handle
            let _ = app.emit("shortcut-action", action_id);
        }
        _ => {}
    }
}

/// Parse a key string like "Ctrl+Shift+K" into (Modifiers, Code).
/// Returns None if the key string is empty or can't be parsed.
fn parse_key_string(keys: &str) -> Option<(Option<Modifiers>, Code)> {
    if keys.is_empty() {
        return None;
    }

    let parts: Vec<&str> = keys.split('+').collect();
    let key_name = parts.last()?;

    // Check if key_name is a mouse button — can't register as global shortcut
    if key_name.starts_with("Mouse") {
        return None;
    }

    let mut mods = Modifiers::empty();
    for part in &parts[..parts.len() - 1] {
        match *part {
            "Ctrl" => mods |= Modifiers::CONTROL,
            "Shift" => mods |= Modifiers::SHIFT,
            "Alt" => mods |= Modifiers::ALT,
            "Meta" | "Super" => mods |= Modifiers::META,
            _ => {}
        }
    }

    let code = match *key_name {
        "Space" => Code::Space,
        "Enter" => Code::Enter,
        "Escape" => Code::Escape,
        "Tab" => Code::Tab,
        "Backspace" => Code::Backspace,
        "Delete" => Code::Delete,
        "Insert" => Code::Insert,
        "Home" => Code::Home,
        "End" => Code::End,
        "PageUp" => Code::PageUp,
        "PageDown" => Code::PageDown,
        "ArrowUp" | "Up" => Code::ArrowUp,
        "ArrowDown" | "Down" => Code::ArrowDown,
        "ArrowLeft" | "Left" => Code::ArrowLeft,
        "ArrowRight" | "Right" => Code::ArrowRight,
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        "+" | "Equal" => Code::Equal,
        "-" | "Minus" => Code::Minus,
        "," | "Comma" => Code::Comma,
        "." | "Period" => Code::Period,
        "/" | "Slash" => Code::Slash,
        ";" | "Semicolon" => Code::Semicolon,
        "`" | "Backquote" => Code::Backquote,
        "[" | "BracketLeft" => Code::BracketLeft,
        "]" | "BracketRight" => Code::BracketRight,
        "\\" | "Backslash" => Code::Backslash,
        "'" | "Quote" => Code::Quote,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        s if s.len() == 1 && s.chars().next().unwrap().is_ascii_alphabetic() => {
            match s.to_ascii_uppercase().chars().next().unwrap() {
                'A' => Code::KeyA,
                'B' => Code::KeyB,
                'C' => Code::KeyC,
                'D' => Code::KeyD,
                'E' => Code::KeyE,
                'F' => Code::KeyF,
                'G' => Code::KeyG,
                'H' => Code::KeyH,
                'I' => Code::KeyI,
                'J' => Code::KeyJ,
                'K' => Code::KeyK,
                'L' => Code::KeyL,
                'M' => Code::KeyM,
                'N' => Code::KeyN,
                'O' => Code::KeyO,
                'P' => Code::KeyP,
                'Q' => Code::KeyQ,
                'R' => Code::KeyR,
                'S' => Code::KeyS,
                'T' => Code::KeyT,
                'U' => Code::KeyU,
                'V' => Code::KeyV,
                'W' => Code::KeyW,
                'X' => Code::KeyX,
                'Y' => Code::KeyY,
                'Z' => Code::KeyZ,
                _ => return None,
            }
        }
        _ => return None,
    };

    let mods_opt = if mods.is_empty() { None } else { Some(mods) };
    Some((mods_opt, code))
}

/// Register user-configured keyboard shortcuts via the global shortcut plugin.
/// Returns a new map of registered shortcut ID → action_id for the handler.
pub fn register_user_shortcuts(
    app: &AppHandle,
    bindings: &HashMap<String, String>,
) -> HashMap<u32, String> {
    let mut shortcut_map = HashMap::new();

    for (action_id, keys) in bindings {
        if keys.is_empty() {
            continue;
        }

        // Only register keyboard shortcuts with modifiers as global
        // (bare keys like "Space" can't be global — they'd break system-wide typing)
        if let Some((mods, code)) = parse_key_string(keys) {
            if mods.is_some() {
                // Has modifiers → register as global shortcut
                let shortcut = Shortcut::new(mods, code);
                match app.global_shortcut().register(shortcut) {
                    Ok(()) => {
                        shortcut_map.insert(shortcut.id(), action_id.clone());
                        log::info!("Registered global shortcut '{}' for {}", keys, action_id);
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to register shortcut '{}' for {}: {}",
                            keys,
                            action_id,
                            e
                        );
                    }
                }
            }
            // Bare keys without modifiers are handled by the frontend via web events
        }
        // Mouse button shortcuts: configure your input remapper to send
        // keyboard combos, then bind those here. The keyboard combos will
        // be registered as global shortcuts above.
    }

    shortcut_map
}

/// Tauri command: update shortcut bindings from frontend.
#[tauri::command]
pub fn update_shortcuts(
    bindings: HashMap<String, String>,
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    // Save to DB
    {
        let db = state.db.lock();
        let json = serde_json::to_string(&bindings).map_err(|e| e.to_string())?;
        db.set_setting("shortcuts", &json)
            .map_err(|e| e.to_string())?;
    }

    // Unregister old user shortcuts (keep media keys)
    {
        let old_map = state.shortcut_map.lock();
        for shortcut_id in old_map.keys() {
            // We stored the shortcut ID, but unregister needs the Shortcut itself.
            // Since we can't reconstruct it easily, we'll unregister all and re-register media keys.
        }
    }
    // Simpler: unregister all, then re-register media keys + new user shortcuts
    let _ = app.global_shortcut().unregister_all();
    {
        let media_keys = [
            Shortcut::new(None, Code::MediaPlayPause),
            Shortcut::new(None, Code::MediaTrackNext),
            Shortcut::new(None, Code::MediaTrackPrevious),
            Shortcut::new(None, Code::MediaStop),
        ];
        for mk in &media_keys {
            let _ = app.global_shortcut().register(*mk);
        }
    }

    // Register new user shortcuts
    let shortcut_map = register_user_shortcuts(&app, &bindings);

    // Update the shortcut map in app state
    *state.shortcut_map.lock() = shortcut_map;
    *state.mouse_bindings.lock() = bindings;

    Ok(())
}

/// Tauri command: get current shortcut bindings.
#[tauri::command]
pub fn get_shortcuts(state: State<'_, Arc<AppState>>) -> Result<HashMap<String, String>, String> {
    let db = state.db.lock();
    match db.get_setting("shortcuts").map_err(|e| e.to_string())? {
        Some(json) => {
            serde_json::from_str(&json).map_err(|e| e.to_string())
        }
        None => Ok(HashMap::new()),
    }
}
