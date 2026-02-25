//! IPC commands for the reputation engine.
//!
//! Exposes the full whitepaper reputation computation to the frontend:
//!   - Query reputation assertions with filters
//!   - Compute reputation for a specific actor (on-demand recompute)
//!   - Get instructor rankings per skill scope
//!   - Verify a reputation assertion against its evidence chain

use tauri::State;

use crate::domain::reputation::{
    FullReputationAssertion, DistributionMetrics, InstructorRanking,
    RecomputeResult, ReputationQuery, VerificationResult,
};
use crate::evidence::reputation;
use crate::AppState;

/// Get reputation assertions with optional filters.
///
/// Supports filtering by actor_address, role, skill_id, proficiency_level.
/// Returns full assertions including distribution metrics.
#[tauri::command]
pub async fn get_reputation(
    state: State<'_, AppState>,
    query: ReputationQuery,
) -> Result<Vec<FullReputationAssertion>, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref addr) = query.actor_address {
        conditions.push(format!("actor_address = ?{idx}"));
        param_values.push(Box::new(addr.clone()));
        idx += 1;
    }
    if let Some(ref role) = query.role {
        conditions.push(format!("role = ?{idx}"));
        param_values.push(Box::new(role.clone()));
        idx += 1;
    }
    if let Some(ref skill) = query.skill_id {
        conditions.push(format!("skill_id = ?{idx}"));
        param_values.push(Box::new(skill.clone()));
        idx += 1;
    }
    if let Some(ref level) = query.proficiency_level {
        conditions.push(format!("proficiency_level = ?{idx}"));
        param_values.push(Box::new(level.clone()));
        idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let limit = query.limit.unwrap_or(100);
    let sql = format!(
        "SELECT id, actor_address, role, skill_id, proficiency_level, \
         score, evidence_count, median_impact, impact_p25, impact_p75, \
         learner_count, impact_variance, window_start, window_end, \
         computation_spec, updated_at \
         FROM reputation_assertions {where_clause} \
         ORDER BY updated_at DESC LIMIT ?{idx}"
    );

    param_values.push(Box::new(limit));

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let assertions = stmt
        .query_map(params_ref.as_slice(), |row| {
            let role: String = row.get(2)?;
            let median: Option<f64> = row.get(7)?;
            let p25: Option<f64> = row.get(8)?;
            let p75: Option<f64> = row.get(9)?;
            let lc: Option<i64> = row.get(10)?;
            let var: Option<f64> = row.get(11)?;

            let distribution = if role == "instructor" && median.is_some() {
                Some(DistributionMetrics {
                    median_impact: median.unwrap_or(0.0),
                    impact_p25: p25.unwrap_or(0.0),
                    impact_p75: p75.unwrap_or(0.0),
                    learner_count: lc.unwrap_or(0),
                    impact_variance: var.unwrap_or(0.0),
                })
            } else {
                None
            };

            // Compute confidence: for instructors use smoothing, for learners use score
            let evidence_count: i64 = row.get(6)?;
            let score: f64 = row.get(5)?;
            let confidence = if role == "instructor" {
                evidence_count as f64 / (evidence_count as f64 + 5.0)
            } else {
                score // learner confidence = score
            };

            Ok(FullReputationAssertion {
                id: row.get(0)?,
                actor_address: row.get(1)?,
                role,
                skill_id: row.get(3)?,
                proficiency_level: row.get(4)?,
                score,
                confidence,
                evidence_count,
                distribution,
                computation_spec: row.get(14)?,
                window_start: row.get(12)?,
                window_end: row.get(13)?,
                updated_at: row.get(15)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(assertions)
}

/// Trigger a full reputation recomputation from the evidence chain.
///
/// Clears all reputation state and replays every proof event
/// chronologically. This is the v2 "any node can reproduce" guarantee.
#[tauri::command]
pub async fn compute_reputation(
    state: State<'_, AppState>,
) -> Result<RecomputeResult, String> {
    let db = state.db.lock().await;

    let start = std::time::Instant::now();
    let (assertions_updated, deltas_recomputed) = reputation::full_recompute(db.conn())?;
    let duration_ms = start.elapsed().as_millis() as i64;

    Ok(RecomputeResult {
        assertions_updated,
        deltas_recomputed,
        duration_ms,
    })
}

/// Get instructor rankings for a skill, ordered by impact score.
///
/// Returns ranked list with impact score, learner count, and median impact.
#[tauri::command]
pub async fn get_instructor_ranking(
    state: State<'_, AppState>,
    skill_id: String,
    proficiency_level: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<InstructorRanking>, String> {
    let db = state.db.lock().await;

    let max = limit.unwrap_or(50);
    let rankings = reputation::get_instructor_rankings(
        db.conn(),
        &skill_id,
        proficiency_level.as_deref(),
        max,
    )?;

    let result: Vec<InstructorRanking> = rankings
        .into_iter()
        .enumerate()
        .map(|(i, (addr, score, ev_count, learner_count, median))| {
            InstructorRanking {
                actor_address: addr,
                skill_id: skill_id.clone(),
                proficiency_level: proficiency_level.clone().unwrap_or_default(),
                impact_score: score,
                confidence: ev_count as f64 / (ev_count as f64 + 5.0),
                learner_count,
                median_impact: median,
                rank: (i + 1) as i64,
            }
        })
        .collect();

    Ok(result)
}

/// Verify a reputation assertion against its evidence chain.
///
/// Checks if the claimed score can be independently reproduced
/// from the stored impact deltas. Used for P2P verification.
#[tauri::command]
pub async fn verify_reputation(
    state: State<'_, AppState>,
    assertion_id: String,
) -> Result<VerificationResult, String> {
    let db = state.db.lock().await;

    let (score_matches, confidence_matches, recomputed_score, recomputed_confidence, claimed_score, claimed_confidence) =
        reputation::verify_assertion(db.conn(), &assertion_id)?;

    let max_diff = (claimed_score - recomputed_score)
        .abs()
        .max((claimed_confidence - recomputed_confidence).abs());

    Ok(VerificationResult {
        score_matches,
        confidence_matches,
        recomputed_score,
        recomputed_confidence,
        claimed_score,
        claimed_confidence,
        max_diff,
    })
}
