//! Corrections to the console/game classification. See
//! `commands/classification.rs` for why the catalog is one table.

use crate::AppState;
use tunante_core::db::{ClassificationOverride, UnclassifiedFolder};
use uuid::Uuid;

pub fn get_overrides(state: &AppState) -> Result<Vec<ClassificationOverride>, String> {
    state.db.lock().get_overrides().map_err(|e| e.to_string())
}

pub fn set_classification(
    state: &AppState,
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

pub fn clear_classification(state: &AppState, scope: &str, target: &str) -> Result<(), String> {
    state
        .db
        .lock()
        .clear_override(scope, target)
        .map_err(|e| e.to_string())
}

pub fn unclassified_folders(state: &AppState) -> Result<Vec<UnclassifiedFolder>, String> {
    state.db.lock().unclassified_folders().map_err(|e| e.to_string())
}

pub fn reclassify_library(state: &AppState) -> Result<usize, String> {
    state.db.lock().reclassify_all().map_err(|e| e.to_string())
}
