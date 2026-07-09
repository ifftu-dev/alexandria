use std::collections::HashSet;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::sentinel_dao::SENTINEL_DAO_ID;
use crate::crypto::hash::entity_id;
use crate::domain::integrity_attestation::{
    attestation_payload, count_valid_committee_cosigs, resolve_assurance, CoSignature,
};
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
        "devtools_detected"
        | "bot_suspected"
        | "face_mismatch"
        | "paste_classifier_critical"
        | "device_glance" => Severity::Critical,
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
) -> Result<StartSessionResponse, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let seed = enrollment_id.as_deref().unwrap_or("standalone");
    let session_id = entity_id(&[seed, &chrono::Utc::now().to_rfc3339()]);

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

    // Fold this snapshot into the session's running commitment chain
    // (P1 attestation): the chained root fixes the order + contents of
    // the flag stream so it is tamper-evident and can be anchored /
    // co-signed. Canonical bytes cover the immutable persisted fields.
    let prev_root: Option<String> = db
        .conn()
        .query_row(
            "SELECT commitment_root FROM integrity_sessions WHERE id = ?1",
            params![req.session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let snapshot_canonical = format!("{snapshot_id}|{composite}|{anomaly_flags_json}");
    let commitment_hash = crate::domain::integrity_attestation::fold_commitment(
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

    db.conn()
        .execute(
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
    db.conn()
        .execute(
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

    // Resolve the assurance ladder now the session is finalized — picks
    // up any anchor / committee co-signatures already recorded.
    recompute_assurance(db.conn(), &session_id)?;

    read_session(db.conn(), &session_id)
}

// ============================================================================
// Automated attestation (P1) — anchor + committee co-signatures
// ============================================================================

#[derive(Debug, Serialize)]
pub struct AssuranceInfo {
    pub session_id: String,
    pub assurance_level: String,
    pub committee_size: usize,
    pub valid_attestations: usize,
    pub anchored: bool,
    pub commitment_root: Option<String>,
}

/// Current Sentinel-DAO committee stake addresses.
fn load_committee(conn: &rusqlite::Connection) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT stake_address FROM governance_dao_members
             WHERE dao_id = ?1 AND role = 'committee'",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![SENTINEL_DAO_ID], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut set = HashSet::new();
    for r in rows {
        set.insert(r.map_err(|e| e.to_string())?);
    }
    Ok(set)
}

/// True if `(stake_address, public_key_hex)` is a registered key binding
/// — defends against pairing a real committee member's address with an
/// attacker-controlled pubkey.
fn pubkey_registered(
    conn: &rusqlite::Connection,
    stake_address: &str,
    public_key_hex: &str,
) -> Result<bool, String> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM stake_pubkey_registry
             WHERE stake_address = ?1 AND public_key_hex = ?2",
            params![stake_address, public_key_hex.to_lowercase()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(n > 0)
}

/// `(status, integrity_score, critical, warning, commitment_root,
/// anchor_ref, ended_at)` — the terminal session fields attestation
/// resolution reads.
type SessionAttestRow = (
    String,
    Option<f64>,
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Re-derive and persist `assurance_level` for a session from its
/// terminal state + recorded anchor + committee co-signatures. Pure
/// resolution lives in `domain::integrity_attestation`.
fn recompute_assurance(conn: &rusqlite::Connection, session_id: &str) -> Result<String, String> {
    let (status, score, critical, warning, root, anchor, ended): SessionAttestRow = conn
        .query_row(
            "SELECT status, integrity_score, critical_count, warning_count,
                    commitment_root, anchor_ref, ended_at
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;

    let root_str = root.clone().unwrap_or_default();
    let payload = attestation_payload(
        session_id,
        &status,
        score,
        critical,
        warning,
        &root_str,
        ended.as_deref().unwrap_or(""),
    );

    let committee = load_committee(conn)?;
    let cosigs = load_attestations(conn, session_id)?;
    let valid = count_valid_committee_cosigs(&payload, &cosigs, &committee);
    let level = resolve_assurance(anchor.is_some(), valid, committee.len());

    conn.execute(
        "UPDATE integrity_sessions SET assurance_level = ?2 WHERE id = ?1",
        params![session_id, level.as_str()],
    )
    .map_err(|e| e.to_string())?;
    Ok(level.as_str().to_string())
}

fn load_attestations(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<CoSignature>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT attestor_address, public_key, signature
             FROM integrity_attestations WHERE session_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |r| {
            Ok(CoSignature {
                attestor_address: r.get(0)?,
                public_key_hex: r.get(1)?,
                signature_hex: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Record a committee co-signature over a finalized session's terminal
/// attestation payload, then re-resolve the assurance ladder. Called by
/// the P2P co-sign ingest path (and directly in tests). Rejects
/// non-committee signers, unregistered keys, and invalid signatures.
#[tauri::command]
pub async fn integrity_record_attestation(
    state: State<'_, AppState>,
    session_id: String,
    attestor_address: String,
    public_key: String,
    signature: String,
) -> Result<String, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    record_attestation_impl(
        db.conn(),
        &session_id,
        &attestor_address,
        &public_key,
        &signature,
    )
}

/// Verify + store one committee co-signature over a finalized session's
/// terminal payload, then re-resolve the assurance ladder. Shared by the
/// IPC command and the P2P co-sign ingest handler. Rejects non-committee
/// signers, unregistered key bindings, unfinalized sessions, and
/// signatures that don't verify.
pub(crate) fn record_attestation_impl(
    conn: &rusqlite::Connection,
    session_id: &str,
    attestor_address: &str,
    public_key: &str,
    signature: &str,
) -> Result<String, String> {
    // Session must be finalized — co-signatures are over the terminal
    // payload (status/score/root/ended_at).
    let ended: Option<String> = conn
        .query_row(
            "SELECT ended_at FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;
    if ended.is_none() {
        return Err("session not finalized — cannot attest".into());
    }

    // Authorization: committee membership + registered key binding.
    if !load_committee(conn)?.contains(attestor_address) {
        return Err("attestor is not a Sentinel DAO committee member".into());
    }
    if !pubkey_registered(conn, attestor_address, public_key)? {
        return Err("attestor public key is not a registered binding".into());
    }

    // Signature must verify over the terminal payload before we store it.
    let committee: HashSet<String> = std::iter::once(attestor_address.to_string()).collect();
    let cosig = CoSignature {
        attestor_address: attestor_address.to_string(),
        public_key_hex: public_key.to_string(),
        signature_hex: signature.to_string(),
    };
    let payload = terminal_payload(conn, session_id)?;
    if count_valid_committee_cosigs(&payload, std::slice::from_ref(&cosig), &committee) != 1 {
        return Err("attestation signature failed verification".into());
    }

    conn.execute(
        "INSERT INTO integrity_attestations (session_id, attestor_address, public_key, signature)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(session_id, attestor_address) DO UPDATE SET
             public_key = excluded.public_key,
             signature = excluded.signature,
             signed_at = datetime('now')",
        params![
            session_id,
            attestor_address,
            public_key.to_lowercase(),
            signature
        ],
    )
    .map_err(|e| e.to_string())?;

    recompute_assurance(conn, session_id)
}

/// Record a confirmed anchor reference (DHT/chain) for the session's
/// commitment root, then re-resolve the assurance ladder (→ at least
/// `anchored`).
#[tauri::command]
pub async fn integrity_set_anchor(
    state: State<'_, AppState>,
    session_id: String,
    anchor_ref: String,
) -> Result<String, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();
    let n = conn
        .execute(
            "UPDATE integrity_sessions SET anchor_ref = ?2 WHERE id = ?1",
            params![session_id, anchor_ref],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("session not found".into());
    }
    recompute_assurance(conn, &session_id)
}

/// Read the current assurance state of a session.
#[tauri::command]
pub async fn integrity_get_assurance(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<AssuranceInfo, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();
    let (level, root, anchor): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT assurance_level, commitment_root, anchor_ref
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "session not found".to_string(),
            other => other.to_string(),
        })?;
    let committee = load_committee(conn)?;
    let payload = terminal_payload(conn, &session_id)?;
    let cosigs = load_attestations(conn, &session_id)?;
    let valid = count_valid_committee_cosigs(&payload, &cosigs, &committee);
    Ok(AssuranceInfo {
        session_id,
        assurance_level: level,
        committee_size: committee.len(),
        valid_attestations: valid,
        anchored: anchor.is_some(),
        commitment_root: root,
    })
}

/// Build the terminal attestation payload for a session (shared by the
/// record + read paths).
fn terminal_payload(conn: &rusqlite::Connection, session_id: &str) -> Result<Vec<u8>, String> {
    let (status, score, critical, warning, root, ended): (
        String,
        Option<f64>,
        i64,
        i64,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT status, integrity_score, critical_count, warning_count,
                    commitment_root, ended_at
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    Ok(attestation_payload(
        session_id,
        &status,
        score,
        critical,
        warning,
        root.as_deref().unwrap_or(""),
        ended.as_deref().unwrap_or(""),
    ))
}

/// Build the canonical terminal payload a committee attestor signs for a
/// session — exposed so the attestor node / tests produce identical
/// bytes. Returns hex of the payload for transport convenience.
pub fn attestation_payload_hex(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<String, String> {
    Ok(hex::encode(terminal_payload(conn, session_id)?))
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

        assert_eq!(flag_severity("paste_classifier_anomaly"), Severity::Warning);
        assert_eq!(
            flag_severity("paste_classifier_critical"),
            Severity::Critical
        );

        // Unknown flags default to Info rather than auto-escalating.
        assert_eq!(flag_severity("totally_made_up"), Severity::Info);
    }

    // ---- Attestation (P1 inc 2) DB-backed flow ----------------------
    use crate::crypto::signing::sign;
    use crate::db::Database;
    use ed25519_dalek::SigningKey;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    /// Seed a finalized session + a committee of `n` members (registered
    /// keys) and return their signing keys + stake addresses.
    fn seed_attestation_env(conn: &rusqlite::Connection, n: u8) -> Vec<(String, SigningKey)> {
        conn.execute(
            "INSERT INTO integrity_sessions
                (id, enrollment_id, status, integrity_score, critical_count, warning_count,
                 started_at, ended_at, commitment_root)
             VALUES ('sess1', NULL, 'completed', 0.9, 0, 1, '2026-01-01T00:00:00Z',
                 '2026-01-01T01:00:00Z', 'root_abc')",
            [],
        )
        .unwrap();
        let mut members = Vec::new();
        for i in 0..n {
            let addr = format!("stake_member_{i}");
            let k = key(i + 1);
            let pk_hex = hex::encode(k.verifying_key().to_bytes());
            conn.execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role, joined_at)
                 VALUES ('sentinel-dao', ?1, 'committee', '2026-01-01')",
                params![addr],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO stake_pubkey_registry
                    (stake_address, public_key_hex, valid_from, source)
                 VALUES (?1, ?2, 0, 'snapshot')",
                params![addr, pk_hex],
            )
            .unwrap();
            members.push((addr, k));
        }
        members
    }

    #[test]
    fn anchor_promotes_to_anchored() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        seed_attestation_env(conn, 3);
        conn.execute(
            "UPDATE integrity_sessions SET anchor_ref = 'dht:abc' WHERE id = 'sess1'",
            [],
        )
        .unwrap();
        assert_eq!(recompute_assurance(conn, "sess1").unwrap(), "anchored");
    }

    #[test]
    fn committee_supermajority_promotes_to_high_assurance() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        let members = seed_attestation_env(conn, 3); // threshold = 2
        let payload = terminal_payload(conn, "sess1").unwrap();

        // One valid co-sig → still below threshold (anchored=false → local).
        let (a0, k0) = &members[0];
        let s0 = sign(&payload, k0);
        conn.execute(
            "INSERT INTO integrity_attestations (session_id, attestor_address, public_key, signature)
             VALUES ('sess1', ?1, ?2, ?3)",
            params![a0, hex::encode(&s0.public_key), hex::encode(&s0.signature)],
        )
        .unwrap();
        assert_eq!(recompute_assurance(conn, "sess1").unwrap(), "local");

        // Second valid co-sig → 2/3 supermajority → high_assurance.
        let (a1, k1) = &members[1];
        let s1 = sign(&payload, k1);
        conn.execute(
            "INSERT INTO integrity_attestations (session_id, attestor_address, public_key, signature)
             VALUES ('sess1', ?1, ?2, ?3)",
            params![a1, hex::encode(&s1.public_key), hex::encode(&s1.signature)],
        )
        .unwrap();
        assert_eq!(
            recompute_assurance(conn, "sess1").unwrap(),
            "high_assurance"
        );
    }

    #[test]
    fn forged_cosig_with_wrong_payload_does_not_count() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        let members = seed_attestation_env(conn, 3);
        // Sign a DIFFERENT payload than the session's terminal one.
        let bogus = b"not-the-real-payload".to_vec();
        for (addr, k) in &members {
            let s = sign(&bogus, k);
            conn.execute(
                "INSERT INTO integrity_attestations (session_id, attestor_address, public_key, signature)
                 VALUES ('sess1', ?1, ?2, ?3)",
                params![addr, hex::encode(&s.public_key), hex::encode(&s.signature)],
            )
            .unwrap();
        }
        // None verify against the terminal payload → stays local.
        assert_eq!(recompute_assurance(conn, "sess1").unwrap(), "local");
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
