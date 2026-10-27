//! IPC commands for derived skill state (§14, §16, §17).
//!
//! Aggregation math lives in `crate::aggregation::aggregate_skill_state`
//! and is pure; loading inputs and caching the result live in
//! [`crate::aggregation::store`]. These commands read and recompute it, and
//! read a skill's dated history.

use crate::clock::now_rfc3339;
use crate::profile::scope::ProfileState as State;
use rusqlite::{params, Connection};

use crate::aggregation::store::{
    get_derived_skill_state_impl, list_derived_states_impl, recompute_all_impl,
};
use crate::aggregation::DerivedSkillState;
use crate::crypto::did::Did;
use crate::db::executor::DatabaseWorkload;
use crate::AppState;

// ---- tauri command handlers ---------------------------------------------

#[tauri::command]
pub async fn get_derived_skill_state(
    state: State<'_, AppState>,
    subject_did: String,
    skill_id: String,
) -> Result<Option<DerivedSkillState>, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "aggregation.get_derived_skill_state",
            move |db| get_derived_skill_state_impl(db.conn(), &Did(subject_did), &skill_id, &now),
        )
        .await
}

#[tauri::command]
pub async fn list_derived_states(
    state: State<'_, AppState>,
    subject_did: Option<String>,
) -> Result<Vec<DerivedSkillState>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "aggregation.list_derived_states",
            move |db| list_derived_states_impl(db.conn(), subject_did.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn recompute_all(state: State<'_, AppState>) -> Result<u32, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            state.profile_lease(),
            "aggregation.recompute_all",
            move |db| recompute_all_impl(db.conn(), &now),
        )
        .await
}

/// One dated point on a skill's confidence/trust history.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillHistoryPoint {
    pub snapshot_date: String,
    pub raw_score: f64,
    pub confidence: f64,
    pub trust_score: f64,
    pub level: u8,
    pub evidence_mass: f64,
}

/// The dated trust/confidence history for one skill, oldest first — the data
/// behind a decay curve and a learning-velocity readout.
#[tauri::command]
pub async fn get_skill_state_history(
    state: State<'_, AppState>,
    subject_did: String,
    skill_id: String,
) -> Result<Vec<SkillHistoryPoint>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "aggregation.get_skill_state_history",
            move |db| get_skill_state_history_impl(db.conn(), &subject_did, &skill_id),
        )
        .await
}

fn get_skill_state_history_impl(
    conn: &Connection,
    subject_did: &str,
    skill_id: &str,
) -> Result<Vec<SkillHistoryPoint>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT snapshot_date, raw_score, confidence, trust_score, level, evidence_mass \
               FROM derived_skill_state_history \
              WHERE subject_did = ?1 AND skill_id = ?2 \
              ORDER BY snapshot_date ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![subject_did, skill_id], |r| {
            Ok(SkillHistoryPoint {
                snapshot_date: r.get(0)?,
                raw_score: r.get(1)?,
                confidence: r.get(2)?,
                trust_score: r.get(3)?,
                level: r.get::<_, i64>(4)? as u8,
                evidence_mass: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();
    Ok(rows)
}
