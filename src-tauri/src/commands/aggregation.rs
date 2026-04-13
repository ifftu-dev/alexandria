//! IPC commands for derived skill state (§14). Stubs — PR 6.

use tauri::State;

use crate::aggregation::DerivedSkillState;
use crate::AppState;

#[tauri::command]
pub async fn get_derived_skill_state(
    _state: State<'_, AppState>,
    _subject_did: String,
    _skill_id: String,
) -> Result<Option<DerivedSkillState>, String> {
    Err("PR 6 — get_derived_skill_state not yet implemented".into())
}

#[tauri::command]
pub async fn list_derived_states(
    _state: State<'_, AppState>,
    _subject_did: Option<String>,
) -> Result<Vec<DerivedSkillState>, String> {
    Err("PR 6 — list_derived_states not yet implemented".into())
}

#[tauri::command]
pub async fn recompute_all(_state: State<'_, AppState>) -> Result<u32, String> {
    Err("PR 6 — recompute_all not yet implemented".into())
}
