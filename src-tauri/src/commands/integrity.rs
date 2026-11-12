use crate::sentinel::snapshots::{
    flag_severity, map_snapshot, persist_snapshot, IntegritySnapshot, Severity,
    SubmitSnapshotRequest,
};

use crate::profile::scope::ProfileState as State;
use rusqlite::{params, OptionalExtension};
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
    /// `local`, or `device_attested` once the platform attestation stored
    /// for this session verified. Re-verified at issuance.
    pub assurance_level: String,
    /// Items whose first visit arrived with response-process timing.
    pub response_items: i64,
    /// The locked trust profile; `None` until one is chosen.
    pub trust_profile: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StartSessionResponse {
    pub session_id: String,
}

/// The assurance every session has. `device_attested` is reached only
/// through a platform attestation that verifies (see
/// `sentinel::attestation`); anchoring and committee co-signing still have
/// no verified path, and a stored `anchor_ref` never raises what a
/// credential claims.
pub(crate) const ACHIEVED_ASSURANCE_LEVEL: &str = crate::sentinel::attestation::ASSURANCE_LOCAL;

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
    // An assessment or interview check is starting: stop any chat answer that
    // is still generating. The chat commands also refuse while it runs.
    state.ai_chat.cancel_all();
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

/// Result of asking the platform to attest a session.
#[derive(Debug, Serialize)]
pub struct AttestSessionResponse {
    /// False on desktop and wherever the platform cannot attest.
    pub supported: bool,
    /// An attestation was captured and stored on the session.
    pub stored: bool,
    /// The session's assurance after this call.
    pub assurance_level: String,
    /// Why the stored attestation did not verify, if it did not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unverified: Option<crate::sentinel::attestation::Unverified>,
}

/// Ask the platform to attest the session nonce and store the result.
///
/// Called by the frontend right after `integrity_start_session`. Blocking
/// platform work runs off the async runtime. Nothing here can fail the
/// session: an unsupported or failed attestation leaves it `local`.
#[tauri::command]
pub async fn integrity_attest_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<AttestSessionResponse, String> {
    use crate::sentinel::attestation::{self as att, native};

    if !native::supported() {
        return Ok(AttestSessionResponse {
            supported: false,
            stored: false,
            assurance_level: att::ASSURANCE_LOCAL.into(),
            unverified: None,
        });
    }
    let sid = session_id.clone();
    let (started_at, existing): (String, Option<String>) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.attest-session.read",
            move |db| {
                db.conn()
                    .query_row(
                        "SELECT started_at, attestation_json FROM integrity_sessions
                          WHERE id = ?1 AND ended_at IS NULL",
                        params![sid],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => "session not found or ended".into(),
                        other => other.to_string(),
                    })
            },
        )
        .await?;
    if existing.is_some() {
        return Err("session already carries an attestation".into());
    }
    let nonce = att::session_nonce(&session_id, &started_at);
    let captured = tokio::task::spawn_blocking(move || native::capture(&nonce))
        .await
        .map_err(|e| format!("attestation worker failed: {e}"))??;
    let Some(captured) = captured else {
        return Ok(AttestSessionResponse {
            supported: true,
            stored: false,
            assurance_level: att::ASSURANCE_LOCAL.into(),
            unverified: None,
        });
    };
    let record = att::DeviceAttestation {
        platform: captured.platform,
        nonce_hex: hex::encode(nonce),
        app_id: captured.app_id,
        payload: captured.payload,
        captured_at: crate::clock::now_rfc3339(),
        companion: None,
    };
    // Verify what we just captured (the end-of-session assertion is not
    // required yet: no commitment root exists).
    let cfg = att::TrustConfig::from_profile();
    let now = chrono::Utc::now().timestamp();
    let (level, unverified) = match att::verify(&record, &nonce, None, &cfg, now) {
        Ok(_) => (att::ASSURANCE_DEVICE_ATTESTED.to_string(), None),
        Err(why) => (att::ASSURANCE_LOCAL.to_string(), Some(why)),
    };
    let json = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    let (sid, lvl) = (session_id.clone(), level.clone());
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.attest-session.store",
            move |db| {
                db.conn()
                    .execute(
                        "UPDATE integrity_sessions SET attestation_json = ?2, assurance_level = ?3
                          WHERE id = ?1 AND ended_at IS NULL",
                        params![sid, json, lvl],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        )
        .await?;
    Ok(AttestSessionResponse {
        supported: true,
        stored: true,
        assurance_level: level,
        unverified,
    })
}

/// Choose and lock the session's trust profile.
///
/// Called once, after `integrity_attest_session` has settled and before the
/// first snapshot. The backend decides: the stored attestation result, the
/// reported capabilities, the device benchmark (run here only when a camera
/// was reported and consented), the role's policy floor and the consent. The
/// reply carries the floor as judged now, so the learner is told before the
/// first item.
#[tauri::command]
pub async fn integrity_select_profile(
    state: State<'_, AppState>,
    session_id: String,
    request: crate::sentinel::trust_profile::ProfileRequest,
) -> Result<crate::sentinel::trust_profile::SelectedProfile, String> {
    use crate::sentinel::trust_profile::select_for_session;
    // When a paired phone is the camera, its benchmark is the one that
    // matters; the desktop's own camera is not used. Without companion
    // consent the phone is dropped from the session here, before anything
    // it reported is read.
    let phone = state
        .content_node
        .companion()
        .consented_role(&session_id, request.consent.companion_phone)
        .filter(|role| role.camera);
    let benchmark = if let Some(role) = phone {
        role.benchmark.filter(|_| request.needs_benchmark())
    } else if request.needs_benchmark() {
        // A benchmark that fails is not a pass: the camera profiles are then
        // excluded as not benchmarked.
        tokio::task::spawn_blocking(crate::sentinel::camera_budget::benchmark)
            .await
            .map_err(|e| format!("benchmark worker failed: {e}"))?
            .map_err(|e| log::warn!(target: "sentinel", "device benchmark failed: {e}"))
            .ok()
    } else {
        None
    };
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.select-profile",
            move |db| select_for_session(db.conn(), &session_id, &request, benchmark.as_ref()),
        )
        .await
}

/// The coverage statement a credential's integrity assertion names by
/// digest, resolved offline from the statements bundled with the app.
/// `None` when no bundled statement has that digest (another version, or a
/// certification this build does not carry).
#[tauri::command]
pub async fn integrity_coverage_statement(
    digest: String,
) -> Result<Option<crate::sentinel::scoring::CoverageStatement>, String> {
    Ok(crate::sentinel::scoring::resolve_coverage_statement(
        &digest,
    ))
}

/// Every coverage statement bundled with the app under the certification in
/// force, one per trust profile and verification rung, so a sponsor reads
/// what a policy's minimums cover before choosing them.
#[tauri::command]
pub async fn integrity_coverage_statements(
) -> Result<Vec<crate::sentinel::scoring::CoverageStatement>, String> {
    let certification = crate::sentinel::scoring::active_certification();
    Ok(crate::sentinel::scoring::coverage_statements(
        certification.as_ref().map(|(c, d)| (c, d.as_str())),
    ))
}

/// iOS: extend a stored App Attest record with an assertion over the
/// terminal commitment root, so the attested key vouches for the snapshot
/// stream as well as the start nonce. Best effort; returns the JSON to
/// store, or `None` when nothing changes.
fn assert_commitment_for(
    attestation_json: Option<&str>,
    commitment_root: Option<&str>,
) -> Option<String> {
    let (key_id, root) = needs_assertion(attestation_json, commitment_root)?;
    let assertion = crate::sentinel::attestation::native::assert_commitment(&key_id, &root)
        .map_err(|e| log::warn!(target: "sentinel", "app attest assertion failed: {e}"))
        .ok()??;
    with_assertion(attestation_json, &root, assertion)
}

/// The App Attest key id and root to assert, when a stored record has no
/// assertion yet and the session has a root.
fn needs_assertion(
    attestation_json: Option<&str>,
    commitment_root: Option<&str>,
) -> Option<(String, String)> {
    use crate::sentinel::attestation::{AttestationPayload, DeviceAttestation};
    let root = commitment_root?;
    let record: DeviceAttestation = serde_json::from_str(attestation_json?).ok()?;
    match record.payload {
        AttestationPayload::AppAttest {
            key_id_b64,
            assertion_b64: None,
            ..
        } => Some((key_id_b64, root.to_string())),
        _ => None,
    }
}

/// The stored record extended with `assertion` over `root`.
fn with_assertion(attestation_json: Option<&str>, root: &str, assertion: String) -> Option<String> {
    use crate::sentinel::attestation::{AttestationPayload, DeviceAttestation};
    let mut record: DeviceAttestation = serde_json::from_str(attestation_json?).ok()?;
    let AttestationPayload::AppAttest {
        assertion_b64,
        asserted_root,
        ..
    } = &mut record.payload
    else {
        return None;
    };
    *assertion_b64 = Some(assertion);
    *asserted_root = Some(root.to_string());
    serde_json::to_string(&record).ok()
}

/// Submit an integrity snapshot with signal scores.
#[tauri::command]
pub async fn integrity_submit_snapshot(
    state: State<'_, AppState>,
    req: SubmitSnapshotRequest,
) -> Result<IntegritySnapshot, String> {
    let staging_session_id = req.session_id.clone();
    // A paired phone that is the camera supplies the window's camera
    // scores; the desktop client's camera group is not used.
    let mut req = req;
    let companion = state.content_node.companion();
    if companion
        .role(&req.session_id)
        .is_some_and(|role| role.camera)
    {
        if let Some(features) = req.features.as_mut() {
            features.camera = companion.take_camera_window(&req.session_id);
        }
    }
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

/// End an integrity session. Its score and status are the backend's own,
/// derived from the committed snapshots; the client sends no scores.
#[tauri::command]
pub async fn integrity_end_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<IntegritySession, String> {
    // On iOS, let the attested key sign the terminal commitment root before
    // the row is finalised. Platform work stays off the DB thread.
    let sid = session_id.clone();
    let pending: Option<(Option<String>, Option<String>)> = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.end-session.read",
            move |db| {
                db.conn()
                    .query_row(
                        "SELECT attestation_json, commitment_root FROM integrity_sessions
                          WHERE id = ?1 AND ended_at IS NULL AND attestation_json IS NOT NULL",
                        params![sid],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()
                    .map_err(|e| e.to_string())
            },
        )
        .await?;
    let companion = state.content_node.companion();
    let asserted = match pending {
        // A paired phone signs the desktop's root with its attested key.
        Some((json, root))
            if crate::sentinel::companion::session::stored_binding(json.as_deref()).is_some() =>
        {
            match needs_assertion(json.as_deref(), root.as_deref()) {
                Some((_, root)) => match companion.request_assertion(&session_id, &root).await {
                    Ok(Some(assertion)) => with_assertion(json.as_deref(), &root, assertion),
                    Ok(None) => None,
                    Err(e) => {
                        log::warn!(target: "sentinel", "companion assertion failed: {e}");
                        None
                    }
                },
                None => None,
            }
        }
        Some((json, root)) => tokio::task::spawn_blocking(move || {
            assert_commitment_for(json.as_deref(), root.as_deref())
        })
        .await
        .map_err(|e| format!("attestation worker failed: {e}"))?,
        None => None,
    };
    let closing = session_id.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.end-session",
            move |db| {
                if let Some(json) = &asserted {
                    db.conn()
                        .execute(
                            "UPDATE integrity_sessions SET attestation_json = ?2
                              WHERE id = ?1 AND ended_at IS NULL",
                            params![session_id, json],
                        )
                        .map_err(|e| e.to_string())?;
                }
                finish_session(db.conn(), &session_id)
            },
        )
        .await
        .inspect(|_| companion.close(&closing))
}

fn finish_session(
    connection: &rusqlite::Connection,
    session_id: &str,
) -> Result<IntegritySession, String> {
    // Backend flags still waiting for the chain to start are committed
    // before the session closes.
    crate::sentinel::snapshots::flush_pending_flags(connection, session_id)?;
    let tx =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let conn = &tx;
    let (current_status, ended_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, ended_at FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
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

    // Every committed snapshot already moved the status as far as the
    // decision rule allows. Ending only turns a clean session completed;
    // review, flagged and suspended stand. Nothing is rescored.
    conn.execute(
        "UPDATE integrity_sessions SET
                status = CASE status WHEN 'active' THEN 'completed' ELSE status END,
                ended_at = datetime('now')
             WHERE id = ?1",
        params![session_id],
    )
    .map_err(|e| e.to_string())?;

    let session = read_session(conn, session_id)?;
    tx.commit().map_err(|e| e.to_string())?;
    if session.status != "completed" {
        log::warn!(
            "integrity session {session_id} ended '{}' (criticals={}, warnings={})",
            session.status,
            session.critical_count,
            session.warning_count
        );
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
            move |db| latest_session_for_enrollment(db.conn(), &enrollment_id),
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
            move |db| list_sessions(db.conn(), status.as_deref()),
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

/// The columns `map_session` reads, in order. Every session query selects
/// exactly these, so a field added to one cannot be missing from another.
const SESSION_COLUMNS: &str = "id, enrollment_id, status, integrity_score, critical_count, \
     warning_count, started_at, ended_at, assurance_level, response_items, trust_profile";

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
        assurance_level: row.get(8)?,
        response_items: row.get(9)?,
        trust_profile: row.get(10)?,
    })
}

fn read_session(conn: &rusqlite::Connection, id: &str) -> Result<IntegritySession, String> {
    conn.query_row(
        &format!("SELECT {SESSION_COLUMNS} FROM integrity_sessions WHERE id = ?1"),
        params![id],
        map_session,
    )
    .map_err(|e| e.to_string())
}

fn latest_session_for_enrollment(
    conn: &rusqlite::Connection,
    enrollment_id: &str,
) -> Result<Option<IntegritySession>, String> {
    let result = conn.query_row(
        &format!(
            "SELECT {SESSION_COLUMNS} FROM integrity_sessions
             WHERE enrollment_id = ?1
             ORDER BY started_at DESC LIMIT 1"
        ),
        params![enrollment_id],
        map_session,
    );
    match result {
        Ok(session) => Ok(Some(session)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn list_sessions(
    conn: &rusqlite::Connection,
    status: Option<&str>,
) -> Result<Vec<IntegritySession>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM integrity_sessions
             WHERE ?1 IS NULL OR status = ?1
             ORDER BY started_at DESC"
        ))
        .map_err(|e| e.to_string())?;
    let sessions = stmt
        .query_map(params![status], map_session)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(sessions)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::Database;
    use crate::sentinel::scoring::{
        DisplayFeatures, ProcessFeatures, ProfileRatios, RatioManifest, WindowFeatures,
        DEFAULT_PROFILE,
    };
    use crate::sentinel::snapshots::{persist_snapshot_with, record_backend_flag};

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

    fn window(features: WindowFeatures) -> SubmitSnapshotRequest {
        SubmitSnapshotRequest::from_features("snapshot-session", "element", features)
    }

    fn split_screen() -> WindowFeatures {
        WindowFeatures {
            display: Some(DisplayFeatures {
                display_count: 1,
                split_screen: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn cheat_tool() -> WindowFeatures {
        WindowFeatures {
            processes: Some(ProcessFeatures {
                categories: vec!["interview_cheat".into()],
                watched: 1,
                scanned: 200,
            }),
            ..Default::default()
        }
    }

    /// A manifest that has measured `split_screen` and `cheat_tool_process`
    /// (two independent channels) and `screen_captured` (the display channel
    /// again), each over the threshold on its own.
    fn measured() -> RatioManifest {
        let mut manifest = RatioManifest::uncertified();
        manifest.version = "test-certified".into();
        manifest.profiles.insert(
            DEFAULT_PROFILE.into(),
            ProfileRatios {
                threshold: 2.0,
                ratios: [
                    ("split_screen".to_string(), 2.5),
                    ("screen_captured".to_string(), 2.5),
                    ("cheat_tool_process".to_string(), 3.0),
                ]
                .into(),
            },
        );
        manifest
    }

    fn status(db: &Database) -> String {
        read_session(db.conn(), "snapshot-session").unwrap().status
    }

    #[test]
    fn a_single_flag_of_any_severity_puts_the_session_in_review_never_suspended() {
        let db = snapshot_db();
        // `split_screen` is critical. Under the old rule a second critical
        // window suspended the attempt.
        for _ in 0..3 {
            let snapshot = persist_snapshot(db.conn(), &window(split_screen())).unwrap();
            assert_eq!(snapshot.anomaly_flags, vec!["split_screen".to_string()]);
            assert_eq!(snapshot.session_status.as_deref(), Some("review"));
        }
        let ended = finish_session(db.conn(), "snapshot-session").unwrap();
        assert_eq!(ended.status, "review");
    }

    #[test]
    fn two_independent_channels_over_threshold_flag_the_session() {
        let db = snapshot_db();
        let manifest = measured();
        let first = persist_snapshot_with(db.conn(), &window(split_screen()), &manifest).unwrap();
        assert_eq!(first.session_status.as_deref(), Some("review"));
        let second = persist_snapshot_with(db.conn(), &window(cheat_tool()), &manifest).unwrap();
        assert_eq!(second.anomaly_flags, vec!["cheat_tool_process".to_string()]);
        assert_eq!(second.session_status.as_deref(), Some("flagged"));
        let fusion = second.fusion.unwrap();
        assert_eq!(fusion.manifest_version, "test-certified");
        let over: Vec<_> = fusion
            .channels
            .iter()
            .filter(|c| c.over_threshold)
            .map(|c| c.channel.as_str())
            .collect();
        assert_eq!(over, ["display", "process"]);
        assert!(fusion.advisory.is_empty());
        assert_eq!(
            finish_session(db.conn(), "snapshot-session")
                .unwrap()
                .status,
            "flagged"
        );
    }

    #[test]
    fn two_measured_signals_in_one_channel_are_one_piece_of_evidence() {
        let db = snapshot_db();
        let both = WindowFeatures {
            display: Some(DisplayFeatures {
                display_count: 1,
                split_screen: true,
                screen_captured: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        for _ in 0..3 {
            let snapshot =
                persist_snapshot_with(db.conn(), &window(both.clone()), &measured()).unwrap();
            assert_eq!(snapshot.session_status.as_deref(), Some("review"));
        }
    }

    #[test]
    fn unmeasured_signals_contribute_zero_and_are_reported_advisory() {
        let db = snapshot_db();
        // Every window raises flags in several channels, which under the old
        // counting rule suspended the session at the second snapshot.
        let mut noisy = split_screen();
        noisy.processes = cheat_tool().processes;
        noisy.obscured_touches = 2;
        for _ in 0..4 {
            let snapshot = persist_snapshot(db.conn(), &window(noisy.clone())).unwrap();
            assert_eq!(snapshot.session_status.as_deref(), Some("review"));
            let fusion = snapshot.fusion.unwrap();
            assert_eq!(fusion.manifest_version, "uncertified");
            assert_eq!(fusion.threshold, None);
            assert!(fusion
                .channels
                .iter()
                .all(|c| c.llr == 0.0 && !c.over_threshold));
            assert_eq!(
                fusion.advisory,
                ["cheat_tool_process", "obscured_touch", "split_screen"]
            );
        }
        // A measured manifest that does not cover these signals is the same.
        let mut partial = measured();
        partial
            .profiles
            .get_mut(DEFAULT_PROFILE)
            .unwrap()
            .ratios
            .retain(|signal, _| signal == "screen_captured");
        let snapshot = persist_snapshot_with(db.conn(), &window(noisy), &partial).unwrap();
        assert_eq!(snapshot.session_status.as_deref(), Some("review"));
    }

    #[test]
    fn a_backend_tamper_flag_suspends_and_the_session_takes_no_more_snapshots() {
        let db = snapshot_db();
        let clean = persist_snapshot(db.conn(), &window(WindowFeatures::default())).unwrap();
        assert_eq!(clean.session_status.as_deref(), Some("active"));
        assert!(clean.anomaly_flags.is_empty());
        persist_snapshot(db.conn(), &window(split_screen())).unwrap();
        assert_eq!(status(&db), "review");

        assert_eq!(
            record_backend_flag(db.conn(), "mcp_call_during_attempt").unwrap(),
            1
        );
        assert_eq!(status(&db), "suspended");
        assert!(
            persist_snapshot(db.conn(), &window(WindowFeatures::default()))
                .unwrap_err()
                .contains("status: suspended")
        );
        assert_eq!(
            finish_session(db.conn(), "snapshot-session")
                .unwrap()
                .status,
            "suspended"
        );
    }

    #[test]
    fn an_interview_check_is_flagged_by_a_tamper_flag_never_suspended() {
        let db = snapshot_db();
        db.conn()
            .execute("UPDATE integrity_sessions SET purpose = 'interview'", [])
            .unwrap();
        crate::sentinel::trust_profile::select_for_session(
            db.conn(),
            "snapshot-session",
            &Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(
            record_backend_flag(db.conn(), "mcp_call_during_attempt").unwrap(),
            1
        );
        assert_eq!(status(&db), "flagged");
    }

    #[test]
    fn the_backend_derives_scores_and_flags_from_raw_features() {
        let db = snapshot_db();
        let mut features = WindowFeatures::default();
        features.clipboard.paste_events = 1;
        features.clipboard.pasted_chars = 600;
        let snapshot = persist_snapshot(db.conn(), &window(features)).unwrap();
        assert_eq!(snapshot.anomaly_flags, vec!["paste_detected".to_string()]);
        assert_eq!(snapshot.paste_score, Some(0.4));
        assert_eq!(snapshot.devtools_score, None);
        let ended = finish_session(db.conn(), "snapshot-session").unwrap();
        assert_eq!(ended.integrity_score, snapshot.composite_score);
    }

    #[test]
    fn flag_rules_moved_from_the_client_hold_at_the_snapshot_boundary() {
        use crate::sentinel::scoring::{
            AndroidFeatures, CameraFeatures, HotkeyFeatures, OverlayFeatures,
        };
        let display = |external, split, captured, toggle, changes| WindowFeatures {
            display: Some(DisplayFeatures {
                display_count: if external { 2 } else { 1 },
                external_display: external,
                split_screen: split,
                screen_captured: captured,
                native_toggle: toggle,
            }),
            display_changes: changes,
            ..Default::default()
        };
        let processes = |categories: &[&str]| WindowFeatures {
            processes: Some(ProcessFeatures {
                categories: categories.iter().map(|c| c.to_string()).collect(),
                watched: categories.len() as u32,
                scanned: 300,
            }),
            ..Default::default()
        };
        let hotkeys = |phantom| WindowFeatures {
            hotkeys: Some(HotkeyFeatures { native: 5, phantom }),
            ..Default::default()
        };
        let camera = |camera: CameraFeatures| WindowFeatures {
            camera: Some(camera),
            ..Default::default()
        };
        let mut android = WindowFeatures {
            android: Some(AndroidFeatures {
                foreign_accessibility_services: 1,
                adb_enabled: true,
            }),
            obscured_touches: 3,
            ..Default::default()
        };
        let mut models = WindowFeatures::default();
        models.models.keystroke_anomaly = Some(0.7);
        models.models.mouse_human_prob = Some(0.3);
        models.models.paste_anomaly = Some(0.96);
        let mut critical_paste = WindowFeatures::default();
        critical_paste.models.paste_anomaly = Some(0.995);
        let mut focus = WindowFeatures::default();
        focus.focus.app_switches = 1;
        focus.focus.tab_switches = 11;

        let cases: Vec<(WindowFeatures, &[&str])> = vec![
            (display(true, false, false, false, 0), &["external_display"]),
            (
                display(true, false, false, false, 1),
                &["external_display", "display_change"],
            ),
            (
                display(false, true, true, false, 0),
                &["split_screen", "screen_captured"],
            ),
            (
                display(false, false, false, true, 2),
                &["split_screen", "display_change"],
            ),
            (
                android.clone(),
                &[
                    "foreign_accessibility_service",
                    "debug_bridge_enabled",
                    "obscured_touch",
                ],
            ),
            (
                WindowFeatures {
                    overlay: Some(OverlayFeatures {
                        suspicious: 1,
                        scanned: 12,
                        allowlisted: 0,
                    }),
                    ..Default::default()
                },
                &["hidden_overlay"],
            ),
            (
                WindowFeatures {
                    overlay: Some(OverlayFeatures {
                        suspicious: 0,
                        scanned: 17,
                        allowlisted: 1,
                    }),
                    ..Default::default()
                },
                &[],
            ),
            (
                processes(&[
                    "interview_cheat",
                    "remote_desktop",
                    "ai_assistant",
                    "screen_share",
                ]),
                &[
                    "cheat_tool_process",
                    "unauthorized_process",
                    "ai_assistant_running",
                    "screen_share_running",
                ],
            ),
            (
                processes(&["virtual_camera", "virtual_machine"]),
                &["unauthorized_process"],
            ),
            (hotkeys(1), &["phantom_hotkey"]),
            (hotkeys(3), &["phantom_hotkey_repeated", "phantom_hotkey"]),
            (
                camera(CameraFeatures {
                    device_virtual: Some(true),
                    ..Default::default()
                }),
                &["virtual_camera"],
            ),
            (
                camera(CameraFeatures {
                    gaze_checks: 10,
                    gaze_down_glances: 3,
                    gaze_offscreen_checks: 4,
                    gaze_occluded_checks: 5,
                    ..Default::default()
                }),
                &["device_glance", "gaze_wander", "gaze_occluded"],
            ),
            (
                camera(CameraFeatures {
                    liveness_checks: 4,
                    liveness_spoof_checks: 3,
                    ..Default::default()
                }),
                &["spoof_suspected"],
            ),
            // Too few liveness ticks to trust a spoof ratio.
            (
                camera(CameraFeatures {
                    liveness_checks: 2,
                    liveness_spoof_checks: 2,
                    ..Default::default()
                }),
                &[],
            ),
            (
                models,
                &[
                    "behavior_shift",
                    "bot_suspected",
                    "paste_classifier_anomaly",
                ],
            ),
            (critical_paste, &["paste_classifier_critical"]),
            (focus, &["tab_switching", "app_switch"]),
        ];
        for (features, expected) in cases {
            let db = snapshot_db();
            let snapshot = persist_snapshot(db.conn(), &window(features.clone())).unwrap();
            assert_eq!(snapshot.anomaly_flags, expected, "{features:?}");
        }
        // Off Android the same counters are simply absent.
        android.android = None;
        android.obscured_touches = 0;
        let db = snapshot_db();
        assert!(persist_snapshot(db.conn(), &window(android))
            .unwrap()
            .anomaly_flags
            .is_empty());
    }

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
        assert_eq!(flag_severity("split_screen"), Severity::Critical);
        assert_eq!(flag_severity("screen_captured"), Severity::Critical);
        assert_eq!(flag_severity("display_change"), Severity::Warning);
        assert_eq!(flag_severity("external_display"), Severity::Info);
        assert_eq!(flag_severity("obscured_touch"), Severity::Critical);
        assert_eq!(
            flag_severity("foreign_accessibility_service"),
            Severity::Warning
        );
        assert_eq!(flag_severity("debug_bridge_enabled"), Severity::Warning);
        assert_eq!(flag_severity("hidden_overlay"), Severity::Critical);
        assert_eq!(flag_severity("virtual_camera"), Severity::Warning);
        assert_eq!(flag_severity("spoof_suspected"), Severity::Warning);
        assert_eq!(flag_severity("cheat_tool_process"), Severity::Critical);
        assert_eq!(flag_severity("unauthorized_process"), Severity::Warning);
        assert_eq!(flag_severity("ai_assistant_running"), Severity::Info);
        assert_eq!(flag_severity("screen_share_running"), Severity::Info);
        assert_eq!(flag_severity("phantom_hotkey"), Severity::Warning);
        assert_eq!(flag_severity("phantom_hotkey_repeated"), Severity::Critical);
        assert_eq!(flag_severity("paste_classifier_anomaly"), Severity::Warning);
        assert_eq!(
            flag_severity("paste_classifier_critical"),
            Severity::Critical
        );
        // Unknown flags default to Info rather than auto-escalating.
        assert_eq!(flag_severity("totally_made_up"), Severity::Info);
    }

    #[test]
    fn every_session_query_reads_the_columns_map_session_expects() {
        let db = snapshot_db();
        // The enrollment row itself is beside the point; only the column mapping is.
        db.conn()
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 UPDATE integrity_sessions SET enrollment_id = 'enrollment' WHERE id = 'snapshot-session';",
            )
            .unwrap();

        let all = list_sessions(db.conn(), None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].assurance_level, "local");
        assert_eq!(list_sessions(db.conn(), Some("active")).unwrap().len(), 1);
        assert!(list_sessions(db.conn(), Some("completed"))
            .unwrap()
            .is_empty());

        let latest = latest_session_for_enrollment(db.conn(), "enrollment").unwrap();
        assert_eq!(latest.map(|s| s.id).as_deref(), Some("snapshot-session"));
        assert!(latest_session_for_enrollment(db.conn(), "other")
            .unwrap()
            .is_none());

        assert_eq!(
            read_session(db.conn(), "snapshot-session").unwrap().status,
            "active"
        );
    }

    #[test]
    fn standalone_session_finish_is_retryable_without_rewriting_terminal_evidence() {
        for (features, expected) in [
            (WindowFeatures::default(), "completed"),
            (split_screen(), "review"),
        ] {
            let db = snapshot_db();
            let request = window(features);
            let committed = persist_snapshot(db.conn(), &request).unwrap();
            let first = finish_session(db.conn(), &request.session_id).unwrap();
            assert_eq!(first.enrollment_id, None);
            assert!(first.ended_at.is_some());
            assert_eq!(first.integrity_score, committed.composite_score);
            assert_eq!(first.status, expected);
            let retry = finish_session(db.conn(), &request.session_id).unwrap();
            assert_eq!(
                serde_json::to_value(first).unwrap(),
                serde_json::to_value(retry).unwrap()
            );
            assert!(persist_snapshot(db.conn(), &request).is_err());
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

    fn legacy_request(extra: serde_json::Value) -> SubmitSnapshotRequest {
        let mut body =
            serde_json::json!({ "session_id": "snapshot-session", "element_id": "element" });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(body).unwrap()
    }

    #[test]
    fn client_computed_scores_and_flags_are_refused_without_writes() {
        let db = snapshot_db();
        // The pre-collector client sent its own scores and flags; the
        // retired resize/devtools scorer sent a devtools score.
        let old_client = serde_json::json!({
            "integrity_score": 0.9, "consistency_score": 0.8, "typing_score": 0.9,
            "mouse_score": 0.9, "human_score": 1.0, "tab_score": 1.0, "paste_score": 1.0,
            "devtools_score": null, "camera_score": null, "ai_paste_anomaly": null,
            "anomaly_flags": [],
        });
        for extra in [
            old_client.clone(),
            serde_json::json!({ "devtools_score": 0.0 }),
            serde_json::json!({ "anomaly_flags": ["devtools_detected"] }),
            serde_json::json!({ "features": {}, "integrity_score": 1.0 }),
        ] {
            assert!(persist_snapshot(db.conn(), &legacy_request(extra))
                .unwrap_err()
                .contains("no longer accepted"));
            assert_empty_snapshot_session(db.conn());
        }
        // Raw features are accepted.
        assert!(persist_snapshot(
            db.conn(),
            &legacy_request(serde_json::json!({ "features": {} }))
        )
        .is_ok());
    }

    #[test]
    fn features_no_honest_collector_produces_are_refused_without_writes() {
        let db = snapshot_db();
        let mut nan = WindowFeatures::default();
        nan.typing.mean_dwell_ms = f64::NAN;
        let mut negative = WindowFeatures::default();
        negative.focus.unfocused_ms = -1.0;
        let mut probability = WindowFeatures::default();
        probability.models.paste_anomaly = Some(1.1);
        let mut inconsistent = WindowFeatures::default();
        inconsistent.mouse.velocity_samples = 5;
        for features in [nan, negative, probability, inconsistent] {
            assert!(persist_snapshot(db.conn(), &window(features)).is_err());
            assert_empty_snapshot_session(db.conn());
        }
        let unknown = serde_json::from_value::<SubmitSnapshotRequest>(serde_json::json!({
            "session_id": "snapshot-session", "element_id": "e", "features": { "typing_score": 1.0 },
        }));
        assert!(unknown.is_err(), "features take measurements only");
    }

    #[test]
    fn snapshot_and_counters_rollback_if_final_outcome_update_fails() {
        let db = snapshot_db();
        db.conn().execute_batch(
            "CREATE TEMP TRIGGER fail_snapshot_outcome BEFORE UPDATE OF status ON integrity_sessions
             BEGIN SELECT RAISE(ABORT, 'injected outcome failure'); END;",
        ).unwrap();
        let request = window(split_screen());
        assert!(persist_snapshot(db.conn(), &request)
            .unwrap_err()
            .contains("injected outcome failure"));
        assert_empty_snapshot_session(db.conn());
        db.conn()
            .execute_batch("DROP TRIGGER fail_snapshot_outcome;")
            .unwrap();
        let snapshot = persist_snapshot(db.conn(), &request).unwrap();
        assert_eq!(snapshot.anomaly_flags, vec!["split_screen".to_string()]);
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
        let composite = snapshot.composite_score.unwrap();
        assert_eq!((status.as_str(), critical, warning), ("review", 1, 0));
        assert_eq!(score, composite);
        let canonical = format!("{}|{composite}|[\"split_screen\"]", snapshot.id);
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
        assert!(
            persist_snapshot(db.conn(), &window(WindowFeatures::default()))
                .unwrap_err()
                .contains("not accepting")
        );
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
}

#[cfg(test)]
#[path = "integrity_response_tests.rs"]
mod response_process_tests;
