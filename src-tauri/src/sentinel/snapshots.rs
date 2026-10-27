//! The integrity snapshot chain: how a snapshot (from the client, or a flag
//! the backend records itself) is validated, folded into its session's
//! commitment chain, counted by severity, and allowed to escalate the
//! session.
//!
//! Lives outside the commands layer so the integrity commands and the AI
//! access policy ([`crate::ai_access`]) write flags through one path.

use std::collections::HashSet;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::crypto::hash::entity_id;

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
    /// The session's status once this snapshot was counted. Only a submit
    /// sets it, so the app can tell the learner when monitoring suspends
    /// the attempt instead of failing at submit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_status: Option<String>,
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

// ============================================================================
// Severity + outcome evaluation
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }
}

/// Maps anomaly flag names (emitted by `useSentinel.computeScores()`) to
/// severity per docs/sentinel.md §Flagging Logic. Unknown flags default to
/// Info so a client/server version skew never auto-suspends a session.
pub(crate) fn flag_severity(flag: &str) -> Severity {
    match flag {
        "bot_suspected"
        | "face_mismatch"
        | "paste_classifier_critical"
        | "device_glance"
        | "split_screen"
        | "screen_captured"
        | "obscured_touch"
        | "hidden_overlay"
        | "cheat_tool_process"
        | "phantom_hotkey_repeated"
        // Recorded by the backend, never sent by a client: an AI feature
        // or an external assistant was used while the attempt was open.
        | "in_app_assistant_during_attempt"
        | "mcp_call_during_attempt" => Severity::Critical,
        "behavior_shift"
        | "paste_detected"
        | "multiple_faces"
        | "prolonged_absence"
        | "low_integrity"
        | "paste_classifier_anomaly"
        | "gaze_wander"
        | "gaze_occluded"
        | "app_switch"
        | "display_change"
        | "foreign_accessibility_service"
        | "debug_bridge_enabled"
        | "virtual_camera"
        | "spoof_suspected"
        | "unauthorized_process"
        | "phantom_hotkey" => Severity::Warning,
        "tab_switching"
        | "no_face"
        | "frequent_absence"
        | "external_display"
        | "ai_assistant_running"
        | "screen_share_running" => Severity::Info,
        _ => Severity::Info,
    }
}

/// Computes session outcome from cumulative counters + running integrity score.
/// Ordered strong→weak: suspended takes precedence over flagged.
pub(crate) fn compute_outcome(
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

/// Flags every live integrity session with `flag`, through the same
/// committed snapshot chain a client's snapshots use, so the flag is counted,
/// escalates the session, and is tamper-evident like any other. The snapshot
/// carries the session's current score, leaving its average unchanged. A
/// session is flagged at most once per flag, so a polling assistant cannot
/// flood it. Returns how many sessions were flagged.
pub(crate) fn record_backend_flag(
    conn: &rusqlite::Connection,
    flag: &str,
) -> Result<usize, String> {
    let sessions = alexandria_studio::gate::live_session_ids(conn).map_err(|e| e.to_string())?;
    let needle = format!("%\"{flag}\"%");
    let mut flagged = 0;
    for session_id in sessions {
        let (already, score): (bool, Option<f64>) = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM integrity_snapshots WHERE session_id = ?1 AND anomaly_flags LIKE ?2),
                        (SELECT integrity_score FROM integrity_sessions WHERE id = ?1)",
                params![session_id, needle],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if already {
            continue;
        }
        let request = SubmitSnapshotRequest {
            session_id,
            element_id: "alexandria".into(),
            integrity_score: score.unwrap_or(1.0).clamp(0.0, 1.0),
            consistency_score: 1.0,
            typing_score: None,
            mouse_score: None,
            human_score: None,
            tab_score: None,
            paste_score: None,
            devtools_score: None,
            camera_score: None,
            ai_paste_anomaly: None,
            gaze_offscreen_ratio: None,
            anomaly_flags: vec![flag.to_string()],
        };
        // A session that stopped accepting snapshots is already final.
        if persist_snapshot(conn, &request).is_ok() {
            flagged += 1;
        }
    }
    Ok(flagged)
}

pub(crate) fn persist_snapshot(
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

    let mut snapshot = read_snapshot(conn, &snapshot_id)?;
    snapshot.session_status = Some(if should_update {
        new_status.to_string()
    } else {
        current_status
    });
    tx.commit().map_err(|e| e.to_string())?;
    Ok(snapshot)
}

pub(crate) fn map_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<IntegritySnapshot> {
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
        session_status: None,
    })
}

pub(crate) fn read_snapshot(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<IntegritySnapshot, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn ai_use_during_an_attempt_is_flagged_once_in_the_committed_chain() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        assert_eq!(
            record_backend_flag(conn, "mcp_call_during_attempt").unwrap(),
            0
        );
        conn.execute(
            "INSERT INTO integrity_sessions (id, status, integrity_score) VALUES ('live', 'active', 0.8)",
            [],
        )
        .unwrap();
        assert_eq!(
            record_backend_flag(conn, "mcp_call_during_attempt").unwrap(),
            1
        );
        assert_eq!(
            record_backend_flag(conn, "mcp_call_during_attempt").unwrap(),
            0,
            "once per session"
        );
        assert_eq!(
            record_backend_flag(conn, "in_app_assistant_during_attempt").unwrap(),
            1
        );
        let (critical, score, root): (i64, f64, Option<String>) = conn
            .query_row(
                "SELECT critical_count, integrity_score, commitment_root FROM integrity_sessions WHERE id = 'live'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(critical, 2);
        assert!((score - 0.8).abs() < 1e-9, "the average is unchanged");
        assert!(root.is_some(), "the flags are folded into the chain");
        assert_eq!(flag_severity("mcp_call_during_attempt"), Severity::Critical);
        assert_eq!(
            flag_severity("in_app_assistant_during_attempt"),
            Severity::Critical
        );
    }
}
