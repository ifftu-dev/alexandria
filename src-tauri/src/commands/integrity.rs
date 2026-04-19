use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::AppState;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegritySession {
    pub id: String,
    pub enrollment_id: String,
    pub status: String,
    pub integrity_score: Option<f64>,
    pub critical_count: i64,
    pub warning_count: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegritySnapshot {
    pub id: String,
    pub session_id: String,
    pub typing_score: Option<f64>,
    pub mouse_score: Option<f64>,
    pub human_score: Option<f64>,
    pub tab_score: Option<f64>,
    pub paste_score: Option<f64>,
    pub devtools_score: Option<f64>,
    pub camera_score: Option<f64>,
    pub composite_score: Option<f64>,
    pub anomaly_flags: Vec<String>,
    pub captured_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitSnapshotRequest {
    pub session_id: String,
    pub element_id: String,
    pub integrity_score: f64,
    pub consistency_score: f64,
    pub typing_score: Option<f64>,
    pub mouse_score: Option<f64>,
    pub human_score: Option<f64>,
    pub tab_score: Option<f64>,
    pub paste_score: Option<f64>,
    pub devtools_score: Option<f64>,
    pub camera_score: Option<f64>,
    pub anomaly_flags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct EndSessionRequest {
    pub overall_integrity_score: f64,
    pub overall_consistency_score: f64,
}

#[derive(Debug, Serialize)]
pub struct StartSessionResponse {
    pub session_id: String,
}

// ============================================================================
// Severity + outcome evaluation
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Severity {
    Info,
    Warning,
    Critical,
}

/// Maps anomaly flag names (emitted by `useSentinel.computeScores()`) to
/// severity per docs/sentinel.md §Flagging Logic. Unknown flags default to
/// Info so a client/server version skew never auto-suspends a session.
fn flag_severity(flag: &str) -> Severity {
    match flag {
        "devtools_detected" | "bot_suspected" | "face_mismatch" => Severity::Critical,
        "behavior_shift" | "paste_detected" | "multiple_faces" | "prolonged_absence"
        | "low_integrity" => Severity::Warning,
        "tab_switching" | "no_face" | "frequent_absence" => Severity::Info,
        _ => Severity::Info,
    }
}

/// Computes session outcome from cumulative counters + running integrity score.
/// Ordered strong→weak: suspended takes precedence over flagged.
fn compute_outcome(
    critical_count: i64,
    warning_count: i64,
    integrity_score: f64,
    ending: bool,
) -> &'static str {
    if critical_count >= 2 || (critical_count >= 1 && warning_count >= 2) {
        "suspended"
    } else if critical_count >= 1 || warning_count >= 3 || integrity_score < 0.40 {
        "flagged"
    } else if ending {
        "completed"
    } else {
        "active"
    }
}

/// Weighted severity penalty per spec §Trust Factor Integration — each
/// critical subtracts 0.20, each warning 0.10, info contributes nothing.
fn trust_penalty(critical_count: i64, warning_count: i64) -> f64 {
    (critical_count as f64) * 0.20 + (warning_count as f64) * 0.10
}

// ============================================================================
// Commands
// ============================================================================

/// Start a new integrity monitoring session for an enrollment.
#[tauri::command]
pub async fn integrity_start_session(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<StartSessionResponse, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let session_id = entity_id(&[&enrollment_id, &chrono::Utc::now().to_rfc3339()]);

    db.conn()
        .execute(
            "INSERT INTO integrity_sessions (id, enrollment_id, status)
             VALUES (?1, ?2, 'active')",
            params![session_id, enrollment_id],
        )
        .map_err(|e| e.to_string())?;

    Ok(StartSessionResponse { session_id })
}

/// Submit an integrity snapshot with signal scores.
#[tauri::command]
pub async fn integrity_submit_snapshot(
    state: State<'_, AppState>,
    req: SubmitSnapshotRequest,
) -> Result<IntegritySnapshot, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    // Accept snapshots for any non-terminal session — once a session is
    // 'suspended' or 'completed' it stops accepting new data, but 'flagged'
    // sessions continue (a learner may recover or a single critical flag
    // may not be the final verdict).
    let current_status: String = db
        .conn()
        .query_row(
            "SELECT status FROM integrity_sessions WHERE id = ?1",
            params![req.session_id],
            |row| row.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;

    if current_status == "suspended" || current_status == "completed" {
        return Err(format!(
            "session not accepting snapshots (status: {current_status})"
        ));
    }

    let snapshot_id = entity_id(&[
        &req.session_id,
        &req.element_id,
        &chrono::Utc::now().to_rfc3339(),
    ]);

    let composite = req.integrity_score;
    let anomaly_flags_json =
        serde_json::to_string(&req.anomaly_flags).map_err(|e| e.to_string())?;

    // Tally severities contributed by this snapshot.
    let mut snap_critical: i64 = 0;
    let mut snap_warning: i64 = 0;
    for flag in &req.anomaly_flags {
        match flag_severity(flag) {
            Severity::Critical => snap_critical += 1,
            Severity::Warning => snap_warning += 1,
            Severity::Info => {}
        }
    }

    db.conn()
        .execute(
            "INSERT INTO integrity_snapshots (id, session_id, typing_score, mouse_score, human_score,
             tab_score, paste_score, devtools_score, camera_score, composite_score, anomaly_flags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                snapshot_id,
                req.session_id,
                req.typing_score,
                req.mouse_score,
                req.human_score,
                req.tab_score,
                req.paste_score,
                req.devtools_score,
                req.camera_score,
                composite,
                anomaly_flags_json,
            ],
        )
        .map_err(|e| e.to_string())?;

    // Update the session's running integrity score (average of snapshots)
    // and cumulative severity counters in a single statement to keep them
    // consistent with each other.
    db.conn()
        .execute(
            "UPDATE integrity_sessions SET
                integrity_score = (
                    SELECT AVG(composite_score) FROM integrity_snapshots WHERE session_id = ?1
                ),
                critical_count  = critical_count + ?2,
                warning_count   = warning_count  + ?3
             WHERE id = ?1",
            params![req.session_id, snap_critical, snap_warning],
        )
        .map_err(|e| e.to_string())?;

    // Re-evaluate outcome from the authoritative counters. Status only moves
    // strong→weak (active → flagged → suspended); never downgrade.
    let (cumulative_critical, cumulative_warning, running_score): (i64, i64, Option<f64>) = db
        .conn()
        .query_row(
            "SELECT critical_count, warning_count, integrity_score
             FROM integrity_sessions WHERE id = ?1",
            params![req.session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| e.to_string())?;

    let new_status = compute_outcome(
        cumulative_critical,
        cumulative_warning,
        running_score.unwrap_or(1.0),
        false,
    );

    // Only promote severity. Never demote (e.g. a recovering session stays
    // flagged until end_session). Terminal transitions happen in end_session.
    let should_update = matches!(
        (current_status.as_str(), new_status),
        ("active", "flagged") | ("active", "suspended") | ("flagged", "suspended")
    );
    if should_update {
        db.conn()
            .execute(
                "UPDATE integrity_sessions SET status = ?2 WHERE id = ?1",
                params![req.session_id, new_status],
            )
            .map_err(|e| e.to_string())?;
    }

    read_snapshot(db.conn(), &snapshot_id)
}

/// End an integrity session and record final scores.
#[tauri::command]
pub async fn integrity_end_session(
    state: State<'_, AppState>,
    session_id: String,
    req: EndSessionRequest,
) -> Result<IntegritySession, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    // Reject double-end.
    let (current_status, cumulative_critical, cumulative_warning): (String, i64, i64) = db
        .conn()
        .query_row(
            "SELECT status, critical_count, warning_count
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;

    if current_status == "completed" {
        return Err("session already ended".into());
    }

    let final_status = compute_outcome(
        cumulative_critical,
        cumulative_warning,
        req.overall_integrity_score,
        // `ending=true` maps clean sessions to "completed" rather than leaving
        // them at "active". Flagged/suspended states stick regardless.
        true,
    );

    db.conn()
        .execute(
            "UPDATE integrity_sessions SET
                status = ?2,
                integrity_score = ?3,
                ended_at = datetime('now')
             WHERE id = ?1",
            params![session_id, final_status, req.overall_integrity_score],
        )
        .map_err(|e| e.to_string())?;

    // Apply trust_factor decay only when the session ends in a non-clean state.
    // The spec pins the per-violation weight at 0.20 for criticals and 0.10 for
    // warnings, with a trust floor of 0.10 — collateral damage is bounded.
    if final_status == "flagged" || final_status == "suspended" {
        let penalty = trust_penalty(cumulative_critical, cumulative_warning);
        if penalty > 0.0 {
            db.conn()
                .execute(
                    "UPDATE evidence_records
                        SET trust_factor = MAX(0.10, trust_factor - ?2)
                      WHERE integrity_session_id = ?1",
                    params![session_id, penalty],
                )
                .map_err(|e| e.to_string())?;
        }
    }

    read_session(db.conn(), &session_id)
}

/// Get the current integrity session for an enrollment.
#[tauri::command]
pub async fn integrity_get_session(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<Option<IntegritySession>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let result = db.conn().query_row(
        "SELECT id, enrollment_id, status, integrity_score, critical_count, warning_count,
                started_at, ended_at
         FROM integrity_sessions
         WHERE enrollment_id = ?1
         ORDER BY started_at DESC LIMIT 1",
        params![enrollment_id],
        map_session,
    );

    match result {
        Ok(session) => Ok(Some(session)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// List all integrity sessions, optionally filtered by status.
#[tauri::command]
pub async fn integrity_list_sessions(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<IntegritySession>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref s) = status {
            (
                "SELECT id, enrollment_id, status, integrity_score, critical_count, warning_count,
                        started_at, ended_at
                 FROM integrity_sessions WHERE status = ?1
                 ORDER BY started_at DESC"
                    .to_string(),
                vec![Box::new(s.clone())],
            )
        } else {
            (
                "SELECT id, enrollment_id, status, integrity_score, critical_count, warning_count,
                        started_at, ended_at
                 FROM integrity_sessions
                 ORDER BY started_at DESC"
                    .to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let sessions = stmt
        .query_map(params_ref.as_slice(), map_session)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(sessions)
}

/// Get snapshots for a session.
#[tauri::command]
pub async fn integrity_list_snapshots(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<IntegritySnapshot>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, session_id, typing_score, mouse_score, human_score,
                    tab_score, paste_score, devtools_score, camera_score,
                    composite_score, anomaly_flags, captured_at
             FROM integrity_snapshots
             WHERE session_id = ?1
             ORDER BY captured_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let snapshots = stmt
        .query_map(params![session_id], map_snapshot)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(snapshots)
}

// ============================================================================
// Row mapping helpers
// ============================================================================

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<IntegritySession> {
    Ok(IntegritySession {
        id: row.get(0)?,
        enrollment_id: row.get(1)?,
        status: row.get(2)?,
        integrity_score: row.get(3)?,
        critical_count: row.get(4)?,
        warning_count: row.get(5)?,
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
    })
}

fn map_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<IntegritySnapshot> {
    let flags_json: Option<String> = row.get(10)?;
    let anomaly_flags: Vec<String> = flags_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(IntegritySnapshot {
        id: row.get(0)?,
        session_id: row.get(1)?,
        typing_score: row.get(2)?,
        mouse_score: row.get(3)?,
        human_score: row.get(4)?,
        tab_score: row.get(5)?,
        paste_score: row.get(6)?,
        devtools_score: row.get(7)?,
        camera_score: row.get(8)?,
        composite_score: row.get(9)?,
        anomaly_flags,
        captured_at: row.get(11)?,
    })
}

fn read_session(conn: &rusqlite::Connection, id: &str) -> Result<IntegritySession, String> {
    conn.query_row(
        "SELECT id, enrollment_id, status, integrity_score, critical_count, warning_count,
                started_at, ended_at
         FROM integrity_sessions WHERE id = ?1",
        params![id],
        map_session,
    )
    .map_err(|e| e.to_string())
}

fn read_snapshot(conn: &rusqlite::Connection, id: &str) -> Result<IntegritySnapshot, String> {
    conn.query_row(
        "SELECT id, session_id, typing_score, mouse_score, human_score,
                tab_score, paste_score, devtools_score, camera_score,
                composite_score, anomaly_flags, captured_at
         FROM integrity_snapshots WHERE id = ?1",
        params![id],
        map_snapshot,
    )
    .map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_mapping_matches_spec() {
        assert_eq!(flag_severity("devtools_detected"), Severity::Critical);
        assert_eq!(flag_severity("bot_suspected"), Severity::Critical);
        assert_eq!(flag_severity("face_mismatch"), Severity::Critical);

        assert_eq!(flag_severity("behavior_shift"), Severity::Warning);
        assert_eq!(flag_severity("paste_detected"), Severity::Warning);
        assert_eq!(flag_severity("multiple_faces"), Severity::Warning);
        assert_eq!(flag_severity("prolonged_absence"), Severity::Warning);
        assert_eq!(flag_severity("low_integrity"), Severity::Warning);

        assert_eq!(flag_severity("tab_switching"), Severity::Info);
        assert_eq!(flag_severity("no_face"), Severity::Info);
        assert_eq!(flag_severity("frequent_absence"), Severity::Info);

        // Unknown flags default to Info rather than auto-escalating.
        assert_eq!(flag_severity("totally_made_up"), Severity::Info);
    }

    #[test]
    fn outcome_respects_spec_thresholds() {
        // Clean baseline
        assert_eq!(compute_outcome(0, 0, 1.0, false), "active");
        assert_eq!(compute_outcome(0, 0, 1.0, true), "completed");

        // Flagged: 1 critical
        assert_eq!(compute_outcome(1, 0, 1.0, false), "flagged");
        // Flagged: 3+ warnings
        assert_eq!(compute_outcome(0, 3, 1.0, false), "flagged");
        // Flagged: low integrity alone
        assert_eq!(compute_outcome(0, 0, 0.39, false), "flagged");

        // Suspended: 2+ critical
        assert_eq!(compute_outcome(2, 0, 1.0, false), "suspended");
        // Suspended: 1 critical + 2 warnings
        assert_eq!(compute_outcome(1, 2, 1.0, false), "suspended");
        // Suspended outranks low-integrity flagged
        assert_eq!(compute_outcome(2, 0, 0.20, false), "suspended");

        // Ending doesn't downgrade flagged/suspended
        assert_eq!(compute_outcome(1, 0, 1.0, true), "flagged");
        assert_eq!(compute_outcome(2, 0, 1.0, true), "suspended");
    }

    #[test]
    fn trust_penalty_matches_spec_weights() {
        assert!((trust_penalty(0, 0) - 0.0).abs() < 1e-9);
        assert!((trust_penalty(1, 0) - 0.20).abs() < 1e-9);
        assert!((trust_penalty(0, 1) - 0.10).abs() < 1e-9);
        assert!((trust_penalty(2, 3) - (0.40 + 0.30)).abs() < 1e-9);
    }
}
