//! IPC commands for the reputation read surface.
//!
//! Post-migration 040 the SkillProof and EvidenceRecord listings are
//! gone; callers should reach for the credentials IPC instead
//! (`list_credentials`, `get_credential`, etc.). Only `list_reputation`
//! survives here because `reputation_assertions` is still the canonical
//! actor-level reputation store and is being repointed at credentials
//! (not rebuilt) in a follow-up session.

use tauri::State;

use crate::domain::evidence::ReputationAssertion;
use crate::AppState;

/// List reputation assertions, optionally filtered by role.
#[tauri::command]
pub async fn list_reputation(
    state: State<'_, AppState>,
    role: Option<String>,
) -> Result<Vec<ReputationAssertion>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref r) = role {
            (
                "SELECT id, actor_address, role, skill_id, proficiency_level, score, \
                 evidence_count, computation_spec, updated_at \
                 FROM reputation_assertions WHERE role = ?1 ORDER BY updated_at DESC"
                    .to_string(),
                vec![Box::new(r.clone())],
            )
        } else {
            (
                "SELECT id, actor_address, role, skill_id, proficiency_level, score, \
                 evidence_count, computation_spec, updated_at \
                 FROM reputation_assertions ORDER BY updated_at DESC"
                    .to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let assertions = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(ReputationAssertion {
                id: row.get(0)?,
                actor_address: row.get(1)?,
                role: row.get(2)?,
                skill_id: row.get(3)?,
                proficiency_level: row.get(4)?,
                score: row.get(5)?,
                evidence_count: row.get(6)?,
                computation_spec: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(assertions)
}
