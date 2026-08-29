//! Reading the console catalog, and correcting what the rules got wrong.
//!
//! The catalog used to live in `consoles.svelte.ts` as a second hand-maintained
//! table, and the cover-art code keyed a third table off *its display strings* —
//! so renaming "SNES" in a `.ts` file silently disabled box-art lookups for the
//! SNES. There is one table now, in `tunante_core::console`, and this is how the
//! frontend reads it.

use crate::AppState;
use std::sync::Arc;
use tauri::State;
use tunante_core::console::{Console, CONSOLES};
use tunante_core::db::{ClassificationOverride, UnclassifiedFolder};
use uuid::Uuid;

/// One console, as the frontend sees it.
///
/// The SVG icon is deliberately *not* here. It is presentation, it lives in
/// `src/lib/data/consoleIcons.ts`, and a console with no icon drawn yet falls
/// back to a generic one rather than being blocked from existing.
#[derive(serde::Serialize)]
pub struct ConsoleDto {
    pub id: String,
    pub name: String,
    pub name_es: String,
    pub codecs: Vec<String>,
    pub libretro: Option<String>,
}

impl From<&Console> for ConsoleDto {
    fn from(c: &Console) -> Self {
        Self {
            id: c.id.to_string(),
            name: c.name.to_string(),
            name_es: c.name_es.to_string(),
            // Both tiers: the frontend only wants to know which extensions
            // belong to this machine, not which of them are definitive.
            codecs: c
                .codecs
                .iter()
                .chain(c.weak_codecs.iter())
                .map(|s| s.to_uppercase())
                .collect(),
            libretro: c.libretro.map(str::to_string),
        }
    }
}

#[tauri::command]
pub fn get_console_catalog() -> Vec<ConsoleDto> {
    CONSOLES.iter().map(ConsoleDto::from).collect()
}

#[tauri::command]
pub fn get_classification_overrides(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ClassificationOverride>, String> {
    state.db.lock().get_overrides().map_err(|e| e.to_string())
}

/// Flag a whole folder — the case this exists for. A franchise folder like
/// `Megaten/` spans five machines, so the correction that is actually true is
/// one level down, on `Megaten/Persona 5`.
#[tauri::command]
pub fn set_folder_classification(
    folder: String,
    console_id: Option<String>,
    game_name: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    set(state, "folder", &folder, console_id, game_name)
}

#[tauri::command]
pub fn set_track_classification(
    track_path: String,
    console_id: Option<String>,
    game_name: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    set(state, "track", &track_path, console_id, game_name)
}

fn set(
    state: State<'_, Arc<AppState>>,
    scope: &str,
    target: &str,
    console_id: Option<String>,
    game_name: Option<String>,
) -> Result<(), String> {
    // Refuse a console the table does not know rather than storing a
    // correction that silently resolves to nothing.
    if let Some(id) = console_id.as_deref().filter(|s| !s.trim().is_empty()) {
        if tunante_core::console::by_id(id.trim()).is_none() {
            return Err(format!("unknown console id: {id}"));
        }
    }
    let db = state.db.lock();
    db.set_override(
        &Uuid::new_v4().to_string(),
        scope,
        target,
        console_id.as_deref(),
        game_name.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_classification(
    scope: String,
    target: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .db
        .lock()
        .clear_override(&scope, &target)
        .map_err(|e| e.to_string())
}

/// The worklist for flagging: folders whose tracks nothing could classify,
/// biggest first.
#[tauri::command]
pub fn get_unclassified_folders(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<UnclassifiedFolder>, String> {
    state.db.lock().unclassified_folders().map_err(|e| e.to_string())
}

/// Rebuild every derived console/game row. An escape hatch — the rules run
/// themselves on scan, on insert and after any correction.
#[tauri::command]
pub fn reclassify_library(state: State<'_, Arc<AppState>>) -> Result<usize, String> {
    state.db.lock().reclassify_all().map_err(|e| e.to_string())
}
