//! IPC commands for the evidence pipeline.
//!
//! Exposes skill proofs, evidence records, and reputation assertions
//! to the frontend.

use tauri::State;

use crate::domain::evidence::{EvidenceRecord, ReputationAssertion, SkillProof};
use crate::AppState;

/// List all skill proofs for the local user.
#[tauri::command]
pub async fn list_skill_proofs(state: State<'_, AppState>) -> Result<Vec<SkillProof>, String> {
    let db = state.db.lock().unwrap();

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, skill_id, proficiency_level, confidence, evidence_count, \
             computed_at, updated_at FROM skill_proofs ORDER BY updated_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let proofs = stmt
        .query_map([], |row| {
            Ok(SkillProof {
                id: row.get(0)?,
                skill_id: row.get(1)?,
                proficiency_level: row.get(2)?,
                confidence: row.get(3)?,
                evidence_count: row.get(4)?,
                computed_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(proofs)
}

/// List evidence records, optionally filtered by skill.
#[tauri::command]
pub async fn list_evidence(
    state: State<'_, AppState>,
    skill_id: Option<String>,
) -> Result<Vec<EvidenceRecord>, String> {
    let db = state.db.lock().unwrap();

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref sid) = skill_id {
            (
                "SELECT id, skill_assessment_id, skill_id, proficiency_level, score, \
                 difficulty, trust_factor, course_id, instructor_address, created_at \
                 FROM evidence_records WHERE skill_id = ?1 ORDER BY created_at DESC"
                    .to_string(),
                vec![Box::new(sid.clone())],
            )
        } else {
            (
                "SELECT id, skill_assessment_id, skill_id, proficiency_level, score, \
                 difficulty, trust_factor, course_id, instructor_address, created_at \
                 FROM evidence_records ORDER BY created_at DESC"
                    .to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let records = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(EvidenceRecord {
                id: row.get(0)?,
                skill_assessment_id: row.get(1)?,
                skill_id: row.get(2)?,
                proficiency_level: row.get(3)?,
                score: row.get(4)?,
                difficulty: row.get(5)?,
                trust_factor: row.get(6)?,
                course_id: row.get(7)?,
                instructor_address: row.get(8)?,
                created_at: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(records)
}

/// List reputation assertions, optionally filtered by role.
#[tauri::command]
pub async fn list_reputation(
    state: State<'_, AppState>,
    role: Option<String>,
) -> Result<Vec<ReputationAssertion>, String> {
    let db = state.db.lock().unwrap();

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
