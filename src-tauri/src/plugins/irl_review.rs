//! IRL Review submission inbox.
//!
//! Local-only instructor-review flow backing the `irl-review` builtin
//! plugin. A learner submits work (files + skills + comment) through the
//! plugin protocol; the host queues a `plugin_irl_submissions` row.
//! Instructor mode lists pending rows and posts a review back.
//!
//! No network — everything stays in this node's SQLite. Federation /
//! cross-device review routing is a later phase.

use rusqlite::{params, OptionalExtension};

use crate::crypto::hash::entity_id;
use crate::db::Database;
use crate::domain::plugin::IrlSubmission;

/// Insert a new IRL Review submission. Returns the row id.
pub fn submit(
    db: &Database,
    plugin_cid: &str,
    element_id: Option<&str>,
    enrollment_id: Option<&str>,
    learner_did: &str,
    submission_json: &str,
    skills_json: &str,
) -> Result<String, String> {
    let created_at = chrono::Utc::now().to_rfc3339();
    let id = entity_id(&[plugin_cid, learner_did, submission_json, &created_at]);
    db.conn()
        .execute(
            "INSERT INTO plugin_irl_submissions \
             (id, plugin_cid, element_id, enrollment_id, learner_did, submission_json, \
              skills_json, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8)",
            params![
                id,
                plugin_cid,
                element_id,
                enrollment_id,
                learner_did,
                submission_json,
                skills_json,
                created_at,
            ],
        )
        .map_err(|e| format!("failed to insert irl submission: {e}"))?;
    Ok(id)
}

/// List the caller's own submissions (any status). Optionally filtered to
/// a single plugin.
pub fn list_for_learner(
    db: &Database,
    learner_did: &str,
    plugin_cid: Option<&str>,
) -> Result<Vec<IrlSubmission>, String> {
    let mut sql = String::from(
        "SELECT id, plugin_cid, element_id, enrollment_id, learner_did, submission_json, \
         skills_json, status, reviewer_did, score, feedback, skill_ratings_json, \
         created_at, reviewed_at \
         FROM plugin_irl_submissions WHERE learner_did = ?1",
    );
    if plugin_cid.is_some() {
        sql.push_str(" AND plugin_cid = ?2");
    }
    sql.push_str(" ORDER BY created_at DESC");

    let conn = db.conn();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if let Some(cid) = plugin_cid {
        stmt.query_map(params![learner_did, cid], row_to_submission)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
    } else {
        stmt.query_map(params![learner_did], row_to_submission)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
    }
    .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// List submissions awaiting an instructor review. Optionally filtered to
/// a single plugin.
pub fn list_pending(db: &Database, plugin_cid: Option<&str>) -> Result<Vec<IrlSubmission>, String> {
    let mut sql = String::from(
        "SELECT id, plugin_cid, element_id, enrollment_id, learner_did, submission_json, \
         skills_json, status, reviewer_did, score, feedback, skill_ratings_json, \
         created_at, reviewed_at \
         FROM plugin_irl_submissions WHERE status = 'pending'",
    );
    if plugin_cid.is_some() {
        sql.push_str(" AND plugin_cid = ?1");
    }
    sql.push_str(" ORDER BY created_at ASC");

    let conn = db.conn();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if let Some(cid) = plugin_cid {
        stmt.query_map(params![cid], row_to_submission)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
    } else {
        stmt.query_map([], row_to_submission)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
    }
    .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Fetch one submission by id. Returns `None` if no row matches.
pub fn get(db: &Database, submission_id: &str) -> Result<Option<IrlSubmission>, String> {
    db.conn()
        .query_row(
            "SELECT id, plugin_cid, element_id, enrollment_id, learner_did, submission_json, \
             skills_json, status, reviewer_did, score, feedback, skill_ratings_json, \
             created_at, reviewed_at \
             FROM plugin_irl_submissions WHERE id = ?1",
            params![submission_id],
            row_to_submission,
        )
        .optional()
        .map_err(|e| e.to_string())
}

/// Post a review for a pending submission. `score` must be in 0..=1.
pub fn post_review(
    db: &Database,
    submission_id: &str,
    reviewer_did: &str,
    score: f64,
    feedback: &str,
    skill_ratings_json: &str,
) -> Result<(), String> {
    if !(0.0..=1.0).contains(&score) {
        return Err(format!("score {score} out of range [0,1]"));
    }
    let reviewed_at = chrono::Utc::now().to_rfc3339();
    let rows = db
        .conn()
        .execute(
            "UPDATE plugin_irl_submissions \
             SET status = 'reviewed', reviewer_did = ?1, score = ?2, feedback = ?3, \
                 skill_ratings_json = ?4, reviewed_at = ?5 \
             WHERE id = ?6 AND status = 'pending'",
            params![
                reviewer_did,
                score,
                feedback,
                skill_ratings_json,
                reviewed_at,
                submission_id,
            ],
        )
        .map_err(|e| e.to_string())?;
    if rows == 0 {
        return Err("submission not pending or not found".into());
    }
    Ok(())
}

fn row_to_submission(row: &rusqlite::Row<'_>) -> rusqlite::Result<IrlSubmission> {
    Ok(IrlSubmission {
        id: row.get(0)?,
        plugin_cid: row.get(1)?,
        element_id: row.get(2)?,
        enrollment_id: row.get(3)?,
        learner_did: row.get(4)?,
        submission_json: row.get(5)?,
        skills_json: row.get(6)?,
        status: row.get(7)?,
        reviewer_did: row.get(8)?,
        score: row.get(9)?,
        feedback: row.get(10)?,
        skill_ratings_json: row.get(11)?,
        created_at: row.get(12)?,
        reviewed_at: row.get(13)?,
    })
}
