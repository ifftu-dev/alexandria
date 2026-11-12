//! The integrity snapshot chain: how a snapshot (raw window features from the
//! client, or a flag the backend records itself) is validated, scored, folded
//! into its session's commitment chain, counted by severity, and allowed to
//! move the session's status.
//!
//! Scores, flags and the session decision come from [`crate::sentinel::scoring`];
//! this module only persists them. Lives outside the commands layer so the
//! integrity commands and the AI access policy ([`crate::ai_access`]) write
//! flags through one path.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::crypto::hash::entity_id;
use crate::sentinel::corpus_capture;
use crate::sentinel::scoring::measurement::CorpusWindow;
use crate::sentinel::scoring::{
    self, decide, derive_window, Decision, FusionSummary, RatioManifest, SessionEvidence,
    SessionStatus, WindowFeatures, DEFAULT_PROFILE,
};
use crate::sentinel::trust_profile;

pub(crate) use crate::sentinel::scoring::{severity as flag_severity, Severity};

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
    /// The window's consistency score. Not persisted; submit only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consistency_score: Option<f64>,
    /// What the fusion saw once this snapshot was counted (submit only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion: Option<FusionSummary>,
}

/// One snapshot window from the client: raw features only. The backend
/// derives every score and flag.
#[derive(Debug, Deserialize)]
pub struct SubmitSnapshotRequest {
    pub session_id: String,
    pub element_id: String,
    #[serde(default)]
    pub features: Option<WindowFeatures>,
    /// Anything else the client sent. An out-of-date client sends its own
    /// scores and flags here; they are refused, never reinterpreted.
    #[serde(flatten)]
    pub retired: serde_json::Map<String, serde_json::Value>,
}

impl SubmitSnapshotRequest {
    pub fn from_features(session_id: &str, element_id: &str, features: WindowFeatures) -> Self {
        Self {
            session_id: session_id.into(),
            element_id: element_id.into(),
            features: Some(features),
            retired: serde_json::Map::new(),
        }
    }
}

/// Score columns a committed snapshot carries. The backend's own flags carry
/// none of them.
#[derive(Default)]
struct ScoreColumns {
    typing: Option<f64>,
    mouse: Option<f64>,
    human: Option<f64>,
    tab: Option<f64>,
    paste: Option<f64>,
    camera: Option<f64>,
    ai_paste_anomaly: Option<f64>,
    gaze_offscreen_ratio: Option<f64>,
}

/// Flags every live integrity session with `flag`, through the same
/// committed snapshot chain a client's snapshots use, so the flag is counted,
/// moves the session's status, and is tamper-evident like any other. The
/// snapshot carries the session's current score, leaving its average
/// unchanged. A session is flagged at most once per flag, so a polling
/// assistant cannot flood it. Returns how many sessions were flagged.
pub(crate) fn record_backend_flag(
    conn: &rusqlite::Connection,
    flag: &str,
) -> Result<usize, String> {
    let sessions = alexandria_studio::gate::live_session_ids(conn).map_err(|e| e.to_string())?;
    let needle = format!("%\"{flag}\"%");
    let mut flagged = 0;
    for session_id in sessions {
        let (already, chain_started): (bool, bool) = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM integrity_snapshots WHERE session_id = ?1 AND anomaly_flags LIKE ?2)
                        OR EXISTS(SELECT 1 FROM integrity_sessions s, json_each(s.pending_backend_flags) p
                                   WHERE s.id = ?1 AND p.value = ?3),
                        (SELECT commitment_root IS NOT NULL FROM integrity_sessions WHERE id = ?1)",
                params![session_id, needle, flag],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if already {
            continue;
        }
        // Before the chain's first link (the profile selection, or a phone
        // pairing ahead of it) the flag waits: committing it now would take
        // that place and block the selection.
        if !chain_started {
            flagged += conn
                .execute(
                    "UPDATE integrity_sessions
                        SET pending_backend_flags = json_insert(pending_backend_flags, '$[#]', ?2)
                      WHERE id = ?1 AND ended_at IS NULL",
                    params![session_id, flag],
                )
                .map_err(|e| e.to_string())?;
            continue;
        }
        if commit_backend_flag(conn, &session_id, flag) {
            flagged += 1;
        }
    }
    Ok(flagged)
}

/// Commit `flag` as its own window in the session's chain, carrying the
/// session's current score so its average is unchanged. False when the
/// session stopped accepting snapshots: it is already final.
fn commit_backend_flag(conn: &rusqlite::Connection, session_id: &str, flag: &str) -> bool {
    let score: Option<f64> = conn
        .query_row(
            "SELECT integrity_score FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    let composite = score.unwrap_or(1.0).clamp(0.0, 1.0);
    let flags = [flag.to_string()];
    let Ok(snapshot) = commit_window(
        conn,
        session_id,
        "alexandria",
        composite,
        &ScoreColumns::default(),
        &flags,
        &[],
        &scoring::active_manifest(),
    ) else {
        return false;
    };
    corpus_capture::capture(session_id, || CorpusWindow {
        features: None,
        backend_flags: flags.to_vec(),
        snapshot: serde_json::to_value(&snapshot).ok(),
    });
    true
}

/// Commit the backend flags that waited for the session's chain to start,
/// in the order they were raised. Called right after the profile selection,
/// ahead of the first client window, and before the session ends, so the
/// chain shows the selection (or that window) first and every flag after.
pub(crate) fn flush_pending_flags(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<(), String> {
    let pending: Option<String> = conn
        .query_row(
            "SELECT pending_backend_flags FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let flags: Vec<String> = pending
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    if flags.is_empty() {
        return Ok(());
    }
    conn.execute(
        "UPDATE integrity_sessions SET pending_backend_flags = '[]' WHERE id = ?1",
        params![session_id],
    )
    .map_err(|e| e.to_string())?;
    for flag in flags {
        commit_backend_flag(conn, session_id, &flag);
    }
    Ok(())
}

pub(crate) fn persist_snapshot(
    connection: &rusqlite::Connection,
    req: &SubmitSnapshotRequest,
) -> Result<IntegritySnapshot, String> {
    persist_snapshot_with(connection, req, &scoring::active_manifest())
}

/// [`persist_snapshot`] against an explicit ratio manifest.
pub(crate) fn persist_snapshot_with(
    connection: &rusqlite::Connection,
    req: &SubmitSnapshotRequest,
    manifest: &RatioManifest,
) -> Result<IntegritySnapshot, String> {
    // A client that computes its own scores or flags is out of date. Its
    // numbers cannot be checked against features it never sent, so they are
    // refused before anything is written, never reinterpreted.
    let features = match (&req.features, req.retired.is_empty()) {
        (Some(features), true) => features,
        _ => {
            return Err(
                "client-computed integrity scores are no longer accepted; refresh the client"
                    .into(),
            )
        }
    };
    features.validate()?;
    // Flags that waited for the chain to start go in ahead of this window.
    flush_pending_flags(connection, &req.session_id)?;
    // The locked profile decides what the window may carry: signals it does
    // not run are dropped, signals it runs that are missing are flagged.
    let mut features = features.clone();
    let profile = trust_profile::session_profile(connection, &req.session_id)?;
    let degraded = trust_profile::shape_to_profile(profile, &mut features);
    let mut assessment = derive_window(&features);
    assessment.flags.extend(degraded);
    let s = &assessment.scores;
    let columns = ScoreColumns {
        typing: Some(s.typing),
        mouse: Some(s.mouse),
        human: Some(s.human),
        tab: Some(s.tab),
        paste: Some(s.paste),
        camera: s.camera,
        ai_paste_anomaly: s.ai_paste_anomaly,
        gaze_offscreen_ratio: s.gaze_offscreen_ratio,
    };
    let mut snapshot = commit_window(
        connection,
        &req.session_id,
        &req.element_id,
        s.integrity,
        &columns,
        &assessment.flags,
        &features
            .response
            .as_ref()
            .map(|r| r.observed_item_ids().collect::<Vec<_>>())
            .unwrap_or_default(),
        manifest,
    )?;
    corpus_capture::capture(&req.session_id, || CorpusWindow {
        features: Some(features.clone()),
        backend_flags: Vec::new(),
        snapshot: serde_json::to_value(&snapshot).ok(),
    });
    snapshot.consistency_score = Some(s.consistency);
    Ok(snapshot)
}

/// The flags of every committed snapshot of a session, in commit order.
fn committed_flags(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<Vec<String>>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT anomaly_flags FROM integrity_snapshots WHERE session_id = ?1
             ORDER BY captured_at, rowid",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| row.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?;
    let mut windows = Vec::new();
    for row in rows {
        let json = row.map_err(|e| e.to_string())?;
        windows.push(
            json.and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
        );
    }
    Ok(windows)
}

#[allow(clippy::too_many_arguments)]
fn commit_window(
    connection: &rusqlite::Connection,
    session_id: &str,
    element_id: &str,
    composite: f64,
    columns: &ScoreColumns,
    flags: &[String],
    observed_items: &[&str],
    manifest: &RatioManifest,
) -> Result<IntegritySnapshot, String> {
    let tx =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let conn = &tx;

    // Accept snapshots for any live session. Once a session is suspended or
    // completed it stops accepting data; review and flagged sessions continue,
    // since neither is a final verdict.
    type SessionRow = (String, Option<String>, String, Option<String>);
    let (current_status, ended_at, purpose, profile): SessionRow = conn
        .query_row(
            "SELECT status, ended_at, purpose, trust_profile FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
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

    let snapshot_id = entity_id(&[session_id, element_id, &chrono::Utc::now().to_rfc3339()]);
    let anomaly_flags_json = serde_json::to_string(flags).map_err(|e| e.to_string())?;

    // Fold this snapshot into the session's running commitment chain:
    // the chained root fixes the order + contents of the flag stream so
    // it is tamper-evident. Canonical bytes cover the immutable persisted
    // fields.
    let prev_root: Option<String> = conn
        .query_row(
            "SELECT commitment_root FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let snapshot_canonical = format!("{snapshot_id}|{composite}|{anomaly_flags_json}");
    let commitment_hash = crate::domain::integrity_commitment::fold_commitment(
        prev_root.as_deref().unwrap_or(""),
        snapshot_canonical.as_bytes(),
    );

    // Severity tallies are kept for display; they no longer decide status.
    let mut snap_critical: i64 = 0;
    let mut snap_warning: i64 = 0;
    for flag in flags {
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
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            snapshot_id,
            session_id,
            columns.typing,
            columns.mouse,
            columns.human,
            columns.tab,
            columns.paste,
            columns.camera,
            composite,
            columns.ai_paste_anomaly,
            columns.gaze_offscreen_ratio,
            commitment_hash,
            anomaly_flags_json,
        ],
    )
    .map_err(|e| e.to_string())?;

    // Each item the response tracker visited counts once per session, however
    // often it is reported (a resumed attempt reports items again).
    for item_id in observed_items {
        conn.execute(
            "INSERT OR IGNORE INTO integrity_observed_items (session_id, item_id) VALUES (?1, ?2)",
            params![session_id, item_id],
        )
        .map_err(|e| e.to_string())?;
    }

    // Running integrity score (average of snapshots) and severity counters
    // in one statement, so they stay consistent with each other.
    conn.execute(
        "UPDATE integrity_sessions SET
                integrity_score = (
                    SELECT AVG(composite_score) FROM integrity_snapshots WHERE session_id = ?1
                ),
                critical_count  = critical_count + ?2,
                warning_count   = warning_count  + ?3,
                commitment_root = ?4,
                response_items  = (
                    SELECT COUNT(*) FROM integrity_observed_items WHERE session_id = ?1
                )
             WHERE id = ?1",
        params![session_id, snap_critical, snap_warning, commitment_hash],
    )
    .map_err(|e| e.to_string())?;

    let evidence = SessionEvidence {
        windows: committed_flags(conn, session_id)?,
        verification_failed: crate::sentinel::verification::has_failed_verification(
            conn, session_id,
        )?,
        interview: purpose == "interview",
    };
    // Sessions without a locked profile are scored as `core`.
    let profile = profile.as_deref().unwrap_or(DEFAULT_PROFILE);
    let decision = decide(&evidence, manifest, profile, false);

    // Status only moves up (active → review → flagged → suspended). Terminal
    // transitions happen in end_session.
    let current = SessionStatus::parse(&current_status).unwrap_or(SessionStatus::Active);
    let should_update = decision.status > current;
    if should_update {
        conn.execute(
            "UPDATE integrity_sessions SET status = ?2 WHERE id = ?1",
            params![session_id, decision.status.as_str()],
        )
        .map_err(|e| e.to_string())?;
    }

    let mut snapshot = read_snapshot(conn, &snapshot_id)?;
    snapshot.session_status = Some(if should_update {
        decision.status.as_str().to_string()
    } else {
        current_status
    });
    snapshot.fusion = Some(decision.summary);
    tx.commit().map_err(|e| e.to_string())?;
    Ok(snapshot)
}

/// Applies the decision rule to a session's committed chain plus signals
/// the backend derives from the profile database once the attempt is
/// graded (longitudinal consistency). Grading needs a finished session, so
/// the signals are not written into the chain, which may already be final
/// and attested; only the status moves, and only up. The same signals are
/// derived again from the database whenever they are needed.
pub(crate) fn decide_with_verification(
    conn: &rusqlite::Connection,
    session_id: &str,
    signals: &[String],
    manifest: &RatioManifest,
) -> Result<Decision, String> {
    let (current_status, ended, purpose, profile): (String, bool, String, Option<String>) = conn
        .query_row(
            "SELECT status, ended_at IS NOT NULL, purpose, trust_profile
               FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;
    let mut windows = committed_flags(conn, session_id)?;
    windows.push(signals.to_vec());
    let evidence = SessionEvidence {
        windows,
        verification_failed: crate::sentinel::verification::has_failed_verification(
            conn, session_id,
        )?,
        interview: purpose == "interview",
    };
    // The session's locked profile, as when its windows were committed.
    let profile = profile.as_deref().unwrap_or(DEFAULT_PROFILE);
    let decision = decide(&evidence, manifest, profile, ended);
    let current = SessionStatus::parse(&current_status).unwrap_or(SessionStatus::Active);
    if decision.status > current {
        conn.execute(
            "UPDATE integrity_sessions SET status = ?2 WHERE id = ?1",
            params![session_id, decision.status.as_str()],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(decision)
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
        consistency_score: None,
        fusion: None,
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
            // An interview check keeps accepting snapshots once flagged; an
            // assessment would be suspended by the first tamper flag.
            "INSERT INTO integrity_sessions (id, status, integrity_score, purpose)
             VALUES ('live', 'active', 0.8, 'interview')",
            [],
        )
        .unwrap();
        // The chain has started: the profile is chosen.
        crate::sentinel::trust_profile::select_for_session(conn, "live", &Default::default(), None)
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
