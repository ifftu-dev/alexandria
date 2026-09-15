//! IPC commands for signed, as-of reputation snapshots.
//!
//! New snapshots are self-issued `DerivedCredential` VCs. Their signed payload
//! freezes every score and contributing eligible credential, and the existing
//! credential-hash queue optionally anchors the VC integrity hash. Historical
//! CIP-68 rows are retained only to reconcile an already-signed transaction.

use crate::profile::scope::ProfileState as State;
use rusqlite::params;

use crate::cardano::{anchor_queue, snapshot_recovery, submission};
use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::domain::reputation::{CreateSnapshotParams, ReputationRole, SnapshotRecord};
use crate::domain::vc::{Claim, CredentialType, CustomClaim};
use crate::AppState;

const SNAPSHOT_FORMAT: &str = "credential_hash_vc";
const SNAPSHOT_SCOPE: &str = "as_of_all_eligible_evidence";
const SCORE_SCALE: i64 = 1_000_000;
const CONFIDENCE_SCALE: i64 = 10_000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotEvidenceRef {
    credential_id: String,
    integrity_hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotSkill {
    skill_id: String,
    proficiency_level: String,
    impact_score_ppm: i64,
    confidence_bps: i64,
    confidence_method: String,
    evidence_count: i64,
    computation_spec: String,
    evidence: Vec<SnapshotEvidenceRef>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReputationSnapshotClaim {
    snapshot_kind: String,
    version: u32,
    snapshot_id: String,
    scope: String,
    as_of: String,
    subject_id: String,
    role: String,
    score_scale: i64,
    confidence_scale: i64,
    skills: Vec<SnapshotSkill>,
}

/// Create a reputation snapshot for on-chain anchoring.
///
/// Gathers the current reputation assertions and their complete eligible VC
/// inputs, signs that immutable claim, and queues its hash without waiting for
/// a chain provider.
#[tauri::command]
pub async fn snapshot_reputation(
    state: State<'_, AppState>,
    params: CreateSnapshotParams,
) -> Result<SnapshotRecord, String> {
    let wallet = {
        let guard = state.keystore.lock().await;
        let keystore = guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = keystore.retrieve_mnemonic().map_err(|e| e.to_string())?;
        crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?
    };
    let workload = if params.role == ReputationRole::Instructor.as_str() {
        DatabaseWorkload::Instructor
    } else {
        DatabaseWorkload::Learner
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    state
        .db_executor
        .execute(
            workload,
            state.profile_lease(),
            "snapshot.create",
            move |db| create_snapshot(db.conn(), &wallet, &params, now_ms),
        )
        .await
}

fn create_snapshot(
    conn: &rusqlite::Connection,
    wallet: &crate::crypto::wallet::Wallet,
    request: &CreateSnapshotParams,
    now_ms: i64,
) -> Result<SnapshotRecord, String> {
    let role = ReputationRole::from_str(&request.role)
        .filter(|role| matches!(role, ReputationRole::Instructor | ReputationRole::Learner))
        .ok_or("only learner and instructor snapshot roles are supported")?;
    let actor_did = crate::crypto::did::did_from_verifying_key(&wallet.signing_key.verifying_key());
    let now = chrono::DateTime::from_timestamp_millis(now_ms)
        .ok_or("invalid snapshot time")?
        .to_rfc3339();
    let snapshot_id = entity_id(&[actor_did.as_str(), &request.subject_id, &request.role, &now]);
    crate::db::with_transaction(conn, || {
        crate::evidence::reputation::refresh_invalidated(conn)?;
        let skills = collect_scores(conn, actor_did.as_str(), &request.subject_id, role)?;
        let computation_specs = skills
            .iter()
            .map(|skill| skill.computation_spec.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let computation_spec = if computation_specs.is_empty() {
            "v4-verified-vc".to_string()
        } else {
            computation_specs.into_iter().collect::<Vec<_>>().join("+")
        };
        let claim = ReputationSnapshotClaim {
            snapshot_kind: "reputation_snapshot".into(),
            version: 2,
            snapshot_id: snapshot_id.clone(),
            scope: SNAPSHOT_SCOPE.into(),
            as_of: now.clone(),
            subject_id: request.subject_id.clone(),
            role: role.as_str().into(),
            score_scale: SCORE_SCALE,
            confidence_scale: CONFIDENCE_SCALE,
            skills,
        };
        let properties = serde_json::to_value(&claim)
            .map_err(|error| error.to_string())?
            .as_object()
            .cloned()
            .ok_or("snapshot claim must serialize as an object")?;
        let credential = crate::commands::credentials::issue_credential_impl(
            conn,
            &wallet.signing_key,
            &actor_did,
            &crate::commands::credentials::IssueCredentialRequest {
                credential_type: CredentialType::DerivedCredential,
                subject: actor_did.clone(),
                claim: Claim::Custom(CustomClaim { properties }),
                evidence_refs: Vec::new(),
                expiration_date: None,
                supersedes: None,
                integrity_session_id: None,
                integrity_policy: None,
            },
            &now,
        )?;
        let credential_id = credential.id.ok_or("snapshot credential has no id")?;
        // The common issuance path currently treats queue insertion as a soft
        // convenience. A snapshot explicitly promises this optional path, so
        // require the local durable queue row in the same transaction.
        anchor_queue::enqueue(conn, &credential_id)?;
        conn.execute(
            "INSERT INTO reputation_snapshots
             (id, actor_address, subject_id, role, skill_count, tx_status,
              snapshot_at, snapshot_format, snapshot_scope, computation_spec, credential_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?8, ?9, ?10)",
            params![
                snapshot_id,
                wallet.stake_address,
                request.subject_id,
                role.as_str(),
                claim.skills.len() as i64,
                now,
                SNAPSHOT_FORMAT,
                SNAPSHOT_SCOPE,
                computation_spec,
                credential_id,
            ],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "INSERT INTO reputation_snapshot_inputs (snapshot_id, context_json)
             VALUES (?1, ?2)",
            params![
                snapshot_id,
                serde_json::to_string(&claim).map_err(|error| error.to_string())?
            ],
        )
        .map_err(|error| error.to_string())?;
        snapshot_recovery::record(conn, &snapshot_id)
    })
}

fn collect_scores(
    conn: &rusqlite::Connection,
    actor_did: &str,
    subject_id: &str,
    role: ReputationRole,
) -> Result<Vec<SnapshotSkill>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ra.skill_id, ra.proficiency_level, ra.score, ra.evidence_count,
                    ra.computation_spec
         FROM current_reputation_assertions ra JOIN skills s ON s.id = ra.skill_id
         WHERE ra.actor_address = ?1 AND ra.role = ?2 AND s.subject_id = ?3
         ORDER BY ra.skill_id, ra.proficiency_level",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![actor_did, role.as_str(), subject_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(skill_id, level, score, count, computation_spec)| {
            if !score.is_finite()
                || !(0.0..=1.0).contains(&score)
                || count < 0
                || computation_spec.is_empty()
                || ![
                    "remember",
                    "understand",
                    "apply",
                    "analyze",
                    "evaluate",
                    "create",
                ]
                .contains(&level.as_str())
            {
                return Err("invalid reputation score in snapshot input".into());
            }
            let evidence = collect_evidence(conn, actor_did, role, &skill_id, &level)?;
            if i64::try_from(evidence.len()).ok() != Some(count) {
                return Err(format!(
                    "reputation input count changed for {skill_id}/{level}; recompute before snapshotting"
                ));
            }
            let confidence = if role == ReputationRole::Instructor {
                count as f64 / (count as f64 + 5.0)
            } else {
                score
            };
            Ok(SnapshotSkill {
                skill_id,
                proficiency_level: level,
                impact_score_ppm: (score * SCORE_SCALE as f64).round() as i64,
                confidence_bps: (confidence * CONFIDENCE_SCALE as f64).round() as i64,
                confidence_method: if role == ReputationRole::Instructor {
                    "sample_count_over_sample_count_plus_5".into()
                } else {
                    "headline_score".into()
                },
                evidence_count: count,
                computation_spec,
                evidence,
            })
        })
        .collect()
}

fn collect_evidence(
    conn: &rusqlite::Connection,
    actor_did: &str,
    role: ReputationRole,
    skill_id: &str,
    level: &str,
) -> Result<Vec<SnapshotEvidenceRef>, String> {
    let role_name = match role {
        ReputationRole::Learner => "learner",
        ReputationRole::Instructor => "instructor",
        _ => return Err("unsupported reputation snapshot role".into()),
    };
    let level = crate::cardano::snapshot::proficiency_to_index(level) as i64;
    // Freeze exactly the verified set that produced the reputation row, so a
    // snapshot can never cite an unverified or self-issued instructor input.
    let evidence = crate::evidence::reputation::verified_reputation_inputs(
        conn, role_name, actor_did, skill_id, level,
    )?
    .into_iter()
    .map(|input| SnapshotEvidenceRef {
        credential_id: input.credential_id,
        integrity_hash: input.integrity_hash,
    })
    .collect::<Vec<_>>();
    for item in &evidence {
        if hex::decode(&item.integrity_hash)
            .ok()
            .is_none_or(|bytes| bytes.len() != 32)
        {
            return Err(format!(
                "credential {} has an invalid integrity hash",
                item.credential_id
            ));
        }
    }
    Ok(evidence)
}

/// List reputation snapshots with optional status filter.
#[tauri::command]
pub async fn list_snapshots(
    state: State<'_, AppState>,
    status: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<SnapshotRecord>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "snapshot.list",
            move |db| list_snapshots_db(db.conn(), status, limit),
        )
        .await
}

fn list_snapshots_db(
    conn: &rusqlite::Connection,
    status: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<SnapshotRecord>, String> {
    let max = limit.unwrap_or(50);

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref s) = status {
            (
                "SELECT rs.id, rs.actor_address, rs.subject_id, rs.role, rs.skill_count,
                 CASE WHEN rs.credential_id IS NULL THEN rs.tx_status ELSE ca.anchor_status END,
                 CASE WHEN rs.credential_id IS NULL THEN rs.tx_hash ELSE ca.anchor_tx_hash END,
                 rs.policy_id, rs.ref_asset_name, rs.user_asset_name,
                 CASE WHEN rs.credential_id IS NULL THEN rs.error_message ELSE ca.last_error END,
                 rs.snapshot_at,
                 CASE WHEN rs.credential_id IS NULL THEN rs.confirmed_at ELSE ca.confirmed_at END,
                 rs.snapshot_format, rs.snapshot_scope, rs.computation_spec, rs.credential_id
                 FROM reputation_snapshots rs
                 LEFT JOIN credential_anchors ca ON ca.credential_id = rs.credential_id
                 WHERE CASE WHEN rs.credential_id IS NULL THEN rs.tx_status
                            ELSE ca.anchor_status END = ?1
                 ORDER BY rs.snapshot_at DESC LIMIT ?2"
                    .into(),
                vec![Box::new(s.clone()), Box::new(max)],
            )
        } else {
            (
                "SELECT rs.id, rs.actor_address, rs.subject_id, rs.role, rs.skill_count,
                 CASE WHEN rs.credential_id IS NULL THEN rs.tx_status ELSE ca.anchor_status END,
                 CASE WHEN rs.credential_id IS NULL THEN rs.tx_hash ELSE ca.anchor_tx_hash END,
                 rs.policy_id, rs.ref_asset_name, rs.user_asset_name,
                 CASE WHEN rs.credential_id IS NULL THEN rs.error_message ELSE ca.last_error END,
                 rs.snapshot_at,
                 CASE WHEN rs.credential_id IS NULL THEN rs.confirmed_at ELSE ca.confirmed_at END,
                 rs.snapshot_format, rs.snapshot_scope, rs.computation_spec, rs.credential_id
                 FROM reputation_snapshots rs
                 LEFT JOIN credential_anchors ca ON ca.credential_id = rs.credential_id
                 ORDER BY rs.snapshot_at DESC LIMIT ?1"
                    .into(),
                vec![Box::new(max)],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let snapshots = stmt
        .query_map(params_ref.as_slice(), snapshot_recovery::record_from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(snapshots)
}

/// Get a specific snapshot by ID.
#[tauri::command]
pub async fn get_snapshot(
    state: State<'_, AppState>,
    snapshot_id: String,
) -> Result<SnapshotRecord, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "snapshot.get",
            move |db| snapshot_recovery::record(db.conn(), &snapshot_id),
        )
        .await
}

/// Request background anchoring for a credential-backed snapshot.
///
/// This command never contacts a chain provider. It makes an unsigned failed
/// anchor retryable, while a durable signed transaction remains bound to its
/// original journal entry. Legacy CIP-68 snapshots cannot start a new mint.
#[tauri::command]
pub async fn submit_snapshot_tx(
    state: State<'_, AppState>,
    snapshot_id: String,
) -> Result<SnapshotRecord, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "snapshot.request_anchor",
            move |db| request_snapshot_anchor(db.conn(), &snapshot_id),
        )
        .await
}

fn request_snapshot_anchor(
    conn: &rusqlite::Connection,
    snapshot_id: &str,
) -> Result<SnapshotRecord, String> {
    let record = snapshot_recovery::record(conn, snapshot_id)?;
    if let Some(credential_id) = &record.credential_id {
        anchor_queue::enqueue_or_retry(conn, credential_id)?;
        return snapshot_recovery::record(conn, snapshot_id);
    }
    let operation = submission::Operation {
        kind: snapshot_recovery::KIND,
        id: snapshot_id,
    };
    if let Some(saved) = submission::lookup(conn, operation)? {
        snapshot_recovery::project(conn, operation, &saved)?;
        return snapshot_recovery::record(conn, snapshot_id);
    }
    Err(
        "legacy CIP-68 snapshot has no signed transaction to recover; new minting is disabled"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::domain::reputation::SnapshotStatus;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    #[test]
    fn snapshot_uses_wallet_did_and_freezes_original_scores() {
        let db = test_db();
        let wallet = crate::crypto::wallet::wallet_from_mnemonic(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        ).unwrap();
        let did = crate::crypto::did::did_from_verifying_key(&wallet.signing_key.verifying_key());
        setup_reputation_data(&db, &wallet.signing_key);
        let request = CreateSnapshotParams {
            subject_id: "sub1".into(),
            role: "instructor".into(),
        };
        let record = create_snapshot(db.conn(), &wallet, &request, 1_714_000_000_000).unwrap();
        assert_eq!(record.actor_address, wallet.stake_address);
        assert_eq!(
            record.skill_count, 1,
            "DID-backed reputation must not be queried by stake address"
        );
        assert_eq!(record.snapshot_format, SNAPSHOT_FORMAT);
        assert_eq!(record.snapshot_scope, SNAPSHOT_SCOPE);
        assert_eq!(record.computation_spec.as_deref(), Some("v3-vc"));
        let credential_id = record.credential_id.as_deref().unwrap();
        let original_json: String = db
            .conn()
            .query_row(
                "SELECT context_json FROM reputation_snapshot_inputs WHERE snapshot_id = ?1",
                [&record.id],
                |row| row.get(0),
            )
            .unwrap();
        let original: ReputationSnapshotClaim = serde_json::from_str(&original_json).unwrap();
        assert_eq!(original.scope, SNAPSHOT_SCOPE);
        assert_eq!(original.as_of, "2024-04-24T23:06:40+00:00");
        assert_eq!(original.skills[0].impact_score_ppm, 850_000);
        assert_eq!(original.skills[0].computation_spec, "v3-vc");
        let source_hash: String = db
            .conn()
            .query_row(
                "SELECT integrity_hash FROM credentials WHERE id = 'source-credential'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            original.skills[0].evidence,
            vec![SnapshotEvidenceRef {
                credential_id: "source-credential".into(),
                integrity_hash: source_hash,
            }]
        );
        let (signed_json, integrity_hash, queued): (String, String, bool) = db
            .conn()
            .query_row(
                "SELECT c.signed_vc_json, c.integrity_hash, EXISTS(
                    SELECT 1 FROM credential_anchors a WHERE a.credential_id = c.id
                 ) FROM credentials c WHERE c.id = ?1",
                [credential_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let signed: crate::domain::vc::VerifiableCredential =
            serde_json::from_str(&signed_json).unwrap();
        assert_eq!(
            signed
                .credential_subject
                .properties
                .get("scope")
                .and_then(|value| value.as_str()),
            Some(SNAPSHOT_SCOPE)
        );
        assert_eq!(signed.issuer, did);
        assert_eq!(signed.credential_subject.id, did);
        assert!(signed.type_.iter().any(|kind| kind == "DerivedCredential"));
        assert_eq!(
            crate::commands::credentials::integrity_hash_of(&signed).unwrap(),
            integrity_hash
        );
        let signed_value: serde_json::Value = serde_json::from_str(&signed_json).unwrap();
        assert_eq!(
            signed_value
                .pointer("/credentialSubject/scope")
                .and_then(|value| value.as_str()),
            Some(SNAPSHOT_SCOPE)
        );
        assert!(queued);
        db.conn()
            .execute("UPDATE reputation_assertions SET score = 0.1", [])
            .unwrap();
        let frozen: String = db
            .conn()
            .query_row(
                "SELECT context_json FROM reputation_snapshot_inputs WHERE snapshot_id = ?1",
                [&record.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(original_json, frozen);
        assert_eq!(
            collect_scores(db.conn(), did.as_str(), "sub1", ReputationRole::Instructor).unwrap()[0]
                .impact_score_ppm,
            100_000
        );
    }

    fn setup_reputation_data(db: &Database, actor_key: &ed25519_dalek::SigningKey) {
        let conn = db.conn();
        let actor = crate::crypto::did::did_from_verifying_key(&actor_key.verifying_key());
        let actor_did = actor.as_str();

        // Identity
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1ulearner', 'addr_test1q123')",
            [],
        )
        .unwrap();

        // Taxonomy
        conn.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graphs', 'sub1')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO reputation_assertions \
             (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, \
              computation_spec) \
             VALUES ('ra1', ?1, 'instructor', 'sk1', 'apply', 0.85, 1, 'v3-vc')",
            [actor_did],
        )
        .unwrap();
        // A genuinely signed credential the actor issued to another learner:
        // snapshot evidence is re-verified, so an unsigned row would not count.
        let learner =
            crate::crypto::did::derive_did_key(&ed25519_dalek::SigningKey::from_bytes(&[5; 32]));
        crate::db::opinion_eligibility::test_support::store_scored_credential(
            db,
            "source-credential",
            actor_key,
            &learner,
            "sk1",
            2,
            0.85,
            None,
        );
    }

    #[test]
    fn snapshot_record_created() {
        let db = test_db();
        let conn = db.conn();

        let now = chrono::Utc::now().to_rfc3339();
        let snapshot_id = entity_id(&["stake_test1ulearner", "sub1", "instructor", &now]);

        conn.execute(
            "INSERT INTO reputation_snapshots \
             (id, actor_address, subject_id, role, skill_count, tx_status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                snapshot_id,
                "stake_test1ulearner",
                "sub1",
                "instructor",
                1,
                "pending",
            ],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM reputation_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn snapshot_status_update() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO reputation_snapshots \
             (id, actor_address, subject_id, role, skill_count, tx_status) \
             VALUES ('snap1', 'addr1', 'sub1', 'instructor', 1, 'pending')",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE reputation_snapshots SET tx_status = 'submitted', \
             tx_hash = 'abc123' WHERE id = 'snap1'",
            [],
        )
        .unwrap();

        let (status, hash): (String, Option<String>) = conn
            .query_row(
                "SELECT tx_status, tx_hash FROM reputation_snapshots WHERE id = 'snap1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(status, "submitted");
        assert_eq!(hash.unwrap(), "abc123");
    }

    #[test]
    fn snapshot_status_enum_roundtrip() {
        for status in &[
            SnapshotStatus::Pending,
            SnapshotStatus::Building,
            SnapshotStatus::Submitted,
            SnapshotStatus::Confirmed,
            SnapshotStatus::Failed,
        ] {
            let s = status.as_str();
            let parsed = SnapshotStatus::from_str(s).unwrap();
            assert_eq!(*status, parsed);
        }
    }

    #[test]
    fn snapshot_rejects_a_stale_evidence_count() {
        let db = test_db();
        let instructor = ed25519_dalek::SigningKey::from_bytes(&[21; 32]);
        let instructor_did =
            crate::crypto::did::did_from_verifying_key(&instructor.verifying_key());
        setup_reputation_data(&db, &instructor);
        db.conn()
            .execute(
                "UPDATE reputation_assertions SET evidence_count = 2 WHERE id = 'ra1'",
                [],
            )
            .unwrap();
        let error = collect_scores(
            db.conn(),
            instructor_did.as_str(),
            "sub1",
            ReputationRole::Instructor,
        )
        .unwrap_err();
        assert!(error.contains("recompute before snapshotting"));
    }

    #[test]
    fn unsigned_legacy_snapshot_cannot_start_a_new_mint() {
        let db = test_db();
        db.conn()
            .execute(
                "INSERT INTO reputation_snapshots
                 (id, actor_address, subject_id, role, skill_count, tx_status)
                 VALUES ('legacy', 'stake', 'subject', 'learner', 0, 'pending')",
                [],
            )
            .unwrap();
        let error = request_snapshot_anchor(db.conn(), "legacy").unwrap_err();
        assert!(error.contains("new minting is disabled"));
    }

    #[test]
    fn duplicate_snapshot_rolls_back_credential_and_anchor() {
        let db = test_db();
        let wallet = crate::crypto::wallet::wallet_from_mnemonic(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        )
        .unwrap();
        setup_reputation_data(&db, &wallet.signing_key);
        let request = CreateSnapshotParams {
            subject_id: "sub1".into(),
            role: "instructor".into(),
        };
        create_snapshot(db.conn(), &wallet, &request, 1_714_000_000_000).unwrap();
        let before: (i64, i64) = db
            .conn()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM credentials),
                        (SELECT COUNT(*) FROM credential_anchors)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(create_snapshot(db.conn(), &wallet, &request, 1_714_000_000_000).is_err());
        let after: (i64, i64) = db
            .conn()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM credentials),
                        (SELECT COUNT(*) FROM credential_anchors)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(before, after);
    }
}
