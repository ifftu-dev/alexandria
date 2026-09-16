use std::collections::HashSet;

use crate::profile::scope::ProfileState as State;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::AppState;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegritySession {
    pub id: String,
    pub enrollment_id: Option<String>,
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
    pub ai_paste_anomaly: Option<f64>,
    pub gaze_offscreen_ratio: Option<f64>,
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
    pub ai_paste_anomaly: Option<f64>,
    #[serde(default)]
    pub gaze_offscreen_ratio: Option<f64>,
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
        "bot_suspected" | "face_mismatch" | "paste_classifier_critical" | "device_glance" => {
            Severity::Critical
        }
        "behavior_shift"
        | "paste_detected"
        | "multiple_faces"
        | "prolonged_absence"
        | "low_integrity"
        | "paste_classifier_anomaly"
        | "gaze_wander"
        | "gaze_occluded"
        | "app_switch" => Severity::Warning,
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

/// The only assurance level a session can currently achieve. Commitment
/// anchoring and committee co-signing have no verified path, so a stored
/// `assurance_level` or `anchor_ref` never raises what a credential claims.
pub(crate) const ACHIEVED_ASSURANCE_LEVEL: &str = "local";

// ============================================================================
// Commands
// ============================================================================

/// Start a new integrity monitoring session for an enrollment.
#[tauri::command]
pub async fn integrity_start_session(
    state: State<'_, AppState>,
    // Optional: course players pass their enrollment id; standalone flows
    // (e.g. a skill assessment not tied to a course) pass null and the session
    // is recorded with a NULL enrollment (the column is nullable).
    enrollment_id: Option<String>,
    purpose: Option<String>,
) -> Result<StartSessionResponse, String> {
    let seed = enrollment_id.as_deref().unwrap_or("standalone");
    let purpose = purpose.unwrap_or_else(|| "assessment".to_owned());
    if !matches!(purpose.as_str(), "assessment" | "interview") {
        return Err(format!("invalid integrity session purpose: {purpose}"));
    }
    let session_id = entity_id(&[seed, &chrono::Utc::now().to_rfc3339()]);
    let persisted_session_id = session_id.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.start-session",
            move |db| {
                db.conn()
                    .execute(
                        "INSERT INTO integrity_sessions (id, enrollment_id, status, purpose)
                         VALUES (?1, ?2, 'active', ?3)",
                        params![persisted_session_id, enrollment_id, purpose],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        )
        .await?;

    Ok(StartSessionResponse { session_id })
}

/// Submit an integrity snapshot with signal scores.
#[tauri::command]
pub async fn integrity_submit_snapshot(
    state: State<'_, AppState>,
    req: SubmitSnapshotRequest,
) -> Result<IntegritySnapshot, String> {
    let staging_session_id = req.session_id.clone();
    let (snapshot, purpose) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.submit-snapshot",
            move |db| {
                let snapshot = persist_snapshot(db.conn(), &req)?;
                let purpose: String = db
                    .conn()
                    .query_row(
                        "SELECT purpose FROM integrity_sessions WHERE id = ?1",
                        params![req.session_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                Ok((snapshot, purpose))
            },
        )
        .await?;
    // Stage appeal evidence only after every snapshot/session write commits.
    // The learner's consent remains required before evidence is persisted.
    // Interview monitoring never stages camera frames as appeal evidence.
    if purpose == "assessment"
        && snapshot
            .anomaly_flags
            .iter()
            .any(|flag| flag_severity(flag) != Severity::Info)
    {
        state
            .evidence_staging
            .stage_last_frame(&staging_session_id, &snapshot.id);
    }
    Ok(snapshot)
}

fn persist_snapshot(
    connection: &rusqlite::Connection,
    req: &SubmitSnapshotRequest,
) -> Result<IntegritySnapshot, String> {
    // Do not reinterpret composites from the retired resize/devtools scorer:
    // their contribution cannot be reconstructed from this request. Reject
    // them before writing either a score or a misconduct record.
    if req.devtools_score.is_some()
        || req
            .anomaly_flags
            .iter()
            .any(|flag| flag == "devtools_detected")
    {
        return Err("developer-tools scoring is no longer supported; refresh the client".into());
    }
    for score in [
        Some(req.integrity_score),
        Some(req.consistency_score),
        req.typing_score,
        req.mouse_score,
        req.human_score,
        req.tab_score,
        req.paste_score,
        req.camera_score,
        req.ai_paste_anomaly,
        req.gaze_offscreen_ratio,
    ]
    .into_iter()
    .flatten()
    {
        if !score.is_finite() || !(0.0..=1.0).contains(&score) {
            return Err("integrity scores must be finite values between zero and one".into());
        }
    }
    let unique: HashSet<_> = req.anomaly_flags.iter().collect();
    if unique.len() != req.anomaly_flags.len() {
        return Err("snapshot anomaly flags must be distinct".into());
    }
    let tx =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let conn = &tx;

    // Accept snapshots for any non-terminal session — once a session is
    // 'suspended' or 'completed' it stops accepting new data, but 'flagged'
    // sessions continue (a learner may recover or a single critical flag
    // may not be the final verdict).
    let (current_status, ended_at, purpose): (String, Option<String>, String) = conn
        .query_row(
            "SELECT status, ended_at, purpose FROM integrity_sessions WHERE id = ?1",
            params![req.session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;

    if ended_at.is_some() || current_status == "suspended" || current_status == "completed" {
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

    // Fold this snapshot into the session's running commitment chain:
    // the chained root fixes the order + contents of the flag stream so
    // it is tamper-evident. Canonical bytes cover the immutable persisted
    // fields.
    let prev_root: Option<String> = conn
        .query_row(
            "SELECT commitment_root FROM integrity_sessions WHERE id = ?1",
            params![req.session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let snapshot_canonical = format!("{snapshot_id}|{composite}|{anomaly_flags_json}");
    let commitment_hash = crate::domain::integrity_commitment::fold_commitment(
        prev_root.as_deref().unwrap_or(""),
        snapshot_canonical.as_bytes(),
    );

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

    conn.execute(
        "INSERT INTO integrity_snapshots (id, session_id, typing_score, mouse_score, human_score,
             tab_score, paste_score, devtools_score, camera_score, composite_score,
             ai_paste_anomaly, gaze_offscreen_ratio, commitment_hash, anomaly_flags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            req.ai_paste_anomaly,
            req.gaze_offscreen_ratio,
            commitment_hash,
            anomaly_flags_json,
        ],
    )
    .map_err(|e| e.to_string())?;

    // Update the session's running integrity score (average of snapshots)
    // and cumulative severity counters in a single statement to keep them
    // consistent with each other.
    conn.execute(
        "UPDATE integrity_sessions SET
                integrity_score = (
                    SELECT AVG(composite_score) FROM integrity_snapshots WHERE session_id = ?1
                ),
                critical_count  = critical_count + ?2,
                warning_count   = warning_count  + ?3,
                commitment_root = ?4
             WHERE id = ?1",
        params![req.session_id, snap_critical, snap_warning, commitment_hash],
    )
    .map_err(|e| e.to_string())?;

    // Re-evaluate outcome from the authoritative counters. Status only moves
    // strong→weak (active → flagged → suspended); never downgrade.
    let (cumulative_critical, cumulative_warning, running_score): (i64, i64, Option<f64>) = conn
        .query_row(
            "SELECT critical_count, warning_count, integrity_score
             FROM integrity_sessions WHERE id = ?1",
            params![req.session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| e.to_string())?;

    let new_status = if purpose == "interview" {
        if cumulative_critical > 0 || cumulative_warning > 0 || running_score.unwrap_or(1.0) < 0.40
        {
            "flagged"
        } else {
            "active"
        }
    } else {
        compute_outcome(
            cumulative_critical,
            cumulative_warning,
            running_score.unwrap_or(1.0),
            false,
        )
    };

    // Only promote severity. Never demote (e.g. a recovering session stays
    // flagged until end_session). Terminal transitions happen in end_session.
    let should_update = matches!(
        (current_status.as_str(), new_status),
        ("active", "flagged") | ("active", "suspended") | ("flagged", "suspended")
    );
    if should_update {
        conn.execute(
            "UPDATE integrity_sessions SET status = ?2 WHERE id = ?1",
            params![req.session_id, new_status],
        )
        .map_err(|e| e.to_string())?;
    }

    let snapshot = read_snapshot(conn, &snapshot_id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(snapshot)
}

/// End an integrity session and record final scores.
#[tauri::command]
pub async fn integrity_end_session(
    state: State<'_, AppState>,
    session_id: String,
    req: EndSessionRequest,
) -> Result<IntegritySession, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.end-session",
            move |db| finish_session(db.conn(), &session_id, &req),
        )
        .await
}

fn finish_session(
    connection: &rusqlite::Connection,
    session_id: &str,
    req: &EndSessionRequest,
) -> Result<IntegritySession, String> {
    for score in [req.overall_integrity_score, req.overall_consistency_score] {
        if !score.is_finite() || !(0.0..=1.0).contains(&score) {
            return Err("integrity scores must be finite values between zero and one".into());
        }
    }
    let tx =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let conn = &tx;
    let (current_status, cumulative_critical, cumulative_warning, ended_at): (
        String,
        i64,
        i64,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT status, critical_count, warning_count, ended_at
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;

    // A lost IPC response must not strand cleanup or permit a retry to
    // overwrite already finalized terminal evidence.
    if ended_at.is_some() || current_status == "completed" {
        return read_session(conn, session_id);
    }

    let final_status = compute_outcome(
        cumulative_critical,
        cumulative_warning,
        req.overall_integrity_score,
        // `ending=true` maps clean sessions to "completed" rather than leaving
        // them at "active". Flagged/suspended states stick regardless.
        true,
    );

    conn.execute(
        "UPDATE integrity_sessions SET
                status = ?2,
                integrity_score = ?3,
                ended_at = datetime('now')
             WHERE id = ?1",
        params![session_id, final_status, req.overall_integrity_score],
    )
    .map_err(|e| e.to_string())?;

    let session = read_session(conn, session_id)?;
    tx.commit().map_err(|e| e.to_string())?;

    // Surface the trust impact when a session ends in a non-clean state.
    // The legacy per-evidence `trust_factor` decay was retired with the
    // `evidence_records` table (migration 040, VC-first cutover). The
    // trust signal now lives on this `integrity_sessions` row — its
    // terminal `status` + `integrity_score` — which downstream credential
    // issuance reads. The spec-pinned penalty (0.20/critical, 0.10/warning)
    // is computed here for observability.
    if final_status == "flagged" || final_status == "suspended" {
        let penalty = trust_penalty(cumulative_critical, cumulative_warning);
        if penalty > 0.0 {
            log::warn!(
                "integrity session {session_id} ended '{final_status}': trust penalty {penalty:.2} \
                 (criticals={cumulative_critical}, warnings={cumulative_warning})"
            );
        }
    }

    Ok(session)
}

/// Get the current integrity session for an enrollment.
#[tauri::command]
pub async fn integrity_get_session(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<Option<IntegritySession>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.get-session",
            move |db| {
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
            },
        )
        .await
}

/// List all integrity sessions, optionally filtered by status.
#[tauri::command]
pub async fn integrity_list_sessions(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<IntegritySession>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.list-sessions",
            move |db| {
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
            },
        )
        .await
}

/// Get snapshots for a session.
#[tauri::command]
pub async fn integrity_list_snapshots(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<IntegritySnapshot>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.list-snapshots",
            move |db| {
                let mut stmt = db
                    .conn()
                    .prepare(
                        "SELECT id, session_id, typing_score, mouse_score, human_score,
                                tab_score, paste_score, devtools_score, camera_score,
                                composite_score, ai_paste_anomaly, gaze_offscreen_ratio,
                                anomaly_flags, captured_at
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
            },
        )
        .await
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
    let flags_json: Option<String> = row.get(12)?;
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
        ai_paste_anomaly: row.get(10)?,
        gaze_offscreen_ratio: row.get(11)?,
        anomaly_flags,
        captured_at: row.get(13)?,
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
                composite_score, ai_paste_anomaly, gaze_offscreen_ratio,
                anomaly_flags, captured_at
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
        assert_eq!(flag_severity("devtools_detected"), Severity::Info);
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

        assert_eq!(flag_severity("paste_classifier_anomaly"), Severity::Warning);
        assert_eq!(
            flag_severity("paste_classifier_critical"),
            Severity::Critical
        );

        // Unknown flags default to Info rather than auto-escalating.
        assert_eq!(flag_severity("totally_made_up"), Severity::Info);
    }

    use crate::db::Database;

    fn snapshot_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, status) VALUES ('snapshot-session', 'active')",
                [],
            )
            .unwrap();
        db
    }

    fn snapshot_request() -> SubmitSnapshotRequest {
        SubmitSnapshotRequest {
            session_id: "snapshot-session".into(),
            element_id: "element".into(),
            integrity_score: 0.9,
            consistency_score: 0.8,
            typing_score: Some(0.9),
            mouse_score: Some(0.9),
            human_score: Some(1.0),
            tab_score: Some(1.0),
            paste_score: Some(1.0),
            devtools_score: None,
            camera_score: None,
            ai_paste_anomaly: None,
            gaze_offscreen_ratio: Some(0.0),
            anomaly_flags: Vec::new(),
        }
    }

    #[test]
    fn standalone_session_finish_is_retryable_without_rewriting_terminal_evidence() {
        for flags in [Vec::new(), vec!["bot_suspected".to_owned()]] {
            let db = snapshot_db();
            let mut request = snapshot_request();
            request.anomaly_flags = flags;
            persist_snapshot(db.conn(), &request).unwrap();
            let first = finish_session(
                db.conn(),
                &request.session_id,
                &EndSessionRequest {
                    overall_integrity_score: 0.9,
                    overall_consistency_score: 0.8,
                },
            )
            .unwrap();
            assert_eq!(first.enrollment_id, None);
            assert!(first.ended_at.is_some());
            assert_eq!(first.integrity_score, Some(0.9));
            assert_eq!(
                first.status,
                if request.anomaly_flags.is_empty() {
                    "completed"
                } else {
                    "flagged"
                }
            );
            let retry = finish_session(
                db.conn(),
                &request.session_id,
                &EndSessionRequest {
                    overall_integrity_score: 0.1,
                    overall_consistency_score: 0.1,
                },
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(first).unwrap(),
                serde_json::to_value(retry).unwrap()
            );
            assert!(persist_snapshot(db.conn(), &request).is_err());
        }
    }

    #[test]
    fn invalid_final_scores_cannot_end_a_session() {
        let db = snapshot_db();
        for score in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            let ending = EndSessionRequest {
                overall_integrity_score: score,
                overall_consistency_score: 0.8,
            };
            assert!(finish_session(db.conn(), "snapshot-session", &ending).is_err());
            assert_empty_snapshot_session(db.conn());
        }
    }

    fn assert_empty_snapshot_session(conn: &rusqlite::Connection) {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM integrity_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        let unchanged: bool = conn
            .query_row(
                "SELECT status = 'active' AND integrity_score IS NULL AND critical_count = 0
             AND warning_count = 0 AND commitment_root IS NULL AND ended_at IS NULL
             FROM integrity_sessions WHERE id = 'snapshot-session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(unchanged);
    }

    #[test]
    fn retired_devtools_inputs_cannot_create_scores_flags_or_penalties() {
        let db = snapshot_db();
        for score in [0.0, 1.0] {
            let mut request = snapshot_request();
            request.devtools_score = Some(score);
            assert!(persist_snapshot(db.conn(), &request)
                .unwrap_err()
                .contains("no longer supported"));
            assert_empty_snapshot_session(db.conn());
        }
        let mut request = snapshot_request();
        request.anomaly_flags.push("devtools_detected".into());
        assert!(persist_snapshot(db.conn(), &request).is_err());
        assert_empty_snapshot_session(db.conn());
        assert_eq!(trust_penalty(0, 0), 0.0);
    }

    #[test]
    fn snapshot_rejects_invalid_scores_and_duplicate_anomalies_without_writes() {
        let db = snapshot_db();
        for score in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            let mut request = snapshot_request();
            request.integrity_score = score;
            assert!(persist_snapshot(db.conn(), &request).is_err());
            request = snapshot_request();
            request.ai_paste_anomaly = Some(score);
            assert!(persist_snapshot(db.conn(), &request).is_err());
            assert_empty_snapshot_session(db.conn());
        }
        let mut request = snapshot_request();
        request.anomaly_flags = vec!["bot_suspected".into(), "bot_suspected".into()];
        assert!(persist_snapshot(db.conn(), &request).is_err());
        assert_empty_snapshot_session(db.conn());
    }

    #[test]
    fn snapshot_and_counters_rollback_if_final_outcome_update_fails() {
        let db = snapshot_db();
        db.conn().execute_batch(
            "CREATE TEMP TRIGGER fail_snapshot_outcome BEFORE UPDATE OF status ON integrity_sessions
             BEGIN SELECT RAISE(ABORT, 'injected outcome failure'); END;",
        ).unwrap();
        let mut request = snapshot_request();
        request.anomaly_flags = vec!["bot_suspected".into()];
        assert!(persist_snapshot(db.conn(), &request)
            .unwrap_err()
            .contains("injected outcome failure"));
        assert_empty_snapshot_session(db.conn());
        db.conn()
            .execute_batch("DROP TRIGGER fail_snapshot_outcome;")
            .unwrap();
        let snapshot = persist_snapshot(db.conn(), &request).unwrap();
        assert_eq!(snapshot.anomaly_flags, request.anomaly_flags);
        let (status, critical, warning, score, root): (String, i64, i64, f64, String) = db
            .conn()
            .query_row(
                "SELECT status, critical_count, warning_count, integrity_score, commitment_root
             FROM integrity_sessions WHERE id = 'snapshot-session'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            (status.as_str(), critical, warning, score),
            ("flagged", 1, 0, 0.9)
        );
        assert_eq!(trust_penalty(critical, warning), 0.2);
        let canonical = format!("{}|0.9|[\"bot_suspected\"]", snapshot.id);
        assert_eq!(
            root,
            crate::domain::integrity_commitment::fold_commitment("", canonical.as_bytes())
        );
    }

    #[test]
    fn ended_flagged_session_cannot_accept_late_snapshots() {
        let db = snapshot_db();
        db.conn()
            .execute(
                "UPDATE integrity_sessions SET status = 'flagged', ended_at = datetime('now'),
             integrity_score = 0.9, commitment_root = 'original' WHERE id = 'snapshot-session'",
                [],
            )
            .unwrap();
        assert!(persist_snapshot(db.conn(), &snapshot_request())
            .unwrap_err()
            .contains("not accepting"));
        let root: String = db
            .conn()
            .query_row(
                "SELECT commitment_root FROM integrity_sessions WHERE id = 'snapshot-session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(root, "original");
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM integrity_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
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
