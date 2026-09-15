//! P2P gossip handler for the Field Commentary opinions topic.
//!
//! Receive-side processing of `TOPIC_OPINIONS` messages:
//!
//! 1. Deserialize the payload as an `OpinionPayload` (the canonical
//!    signed form from `domain::opinions`).
//! 2. Validate required fields + `author_address` matches the envelope
//!    signer + deterministic `opinion_id = blake2b(author + video_cid)`.
//! 3. Verify the Ed25519 signature over the canonical payload bytes.
//! 4. Check that the `subject_field_id` exists locally.
//! 5. Refuse the opinion unless a pinned subject qualification policy
//!    governs posting in that field, then check the referenced
//!    `credential_proof_ids` (at most `MAX_OPINION_CREDENTIAL_PROOFS`) with
//!    `db::opinion_eligibility`: each is re-verified from its signed bytes
//!    and must satisfy the policy with the signer as subject. If no
//!    reference is conclusively refused but some are unknown or await
//!    verification evidence, queue the opinion in
//!    `opinions_pending_verification` for later promotion.
//! 6. If all checks pass, UPSERT into the `opinions` table.
//!
//! The outgoing side (building + publishing) lives in the `publish_opinion`
//! tauri command in `commands::opinions`.

use ed25519_dalek::{Signature, VerifyingKey};
use rusqlite::params;

use alexandria_verify::qualification::{
    NotQualifiedReason, QualificationAction, QualificationPolicySet,
};

use crate::crypto::hash::entity_id;
use crate::db::opinion_eligibility::{
    check_opinion_credential, opinion_refusal_message, verification_time_now,
    OpinionCredentialEligibility, MAX_OPINION_CREDENTIAL_PROOFS,
};
use crate::db::Database;
use crate::domain::opinions::OpinionPayload;
use crate::network_profile::embedded_qualification_policies;
use crate::p2p::types::SignedGossipMessage;

/// Outcome of processing an inbound opinion announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpinionIngest {
    /// Opinion was stored in `opinions`.
    Stored,
    /// Opinion was queued in `opinions_pending_verification` because
    /// none of the referenced credential_proof_ids are known yet.
    Pending,
    /// The opinion was already present at an equal-or-newer published_at.
    Ignored,
}

/// Handle an incoming opinion announcement from the P2P network against the
/// embedded network's pinned qualification policies.
pub fn handle_opinion_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<OpinionIngest, String> {
    let policies = embedded_qualification_policies().map_err(|error| error.to_string())?;
    handle_opinion_message_with(db, message, policies, &verification_time_now())
}

fn handle_opinion_message_with(
    db: &Database,
    message: &SignedGossipMessage,
    policies: &QualificationPolicySet,
    verification_time: &str,
) -> Result<OpinionIngest, String> {
    let payload: OpinionPayload = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid opinion payload: {e}"))?;

    // Required-field guards
    if payload.opinion_id.is_empty() {
        return Err("opinion missing opinion_id".into());
    }
    if payload.author_address.is_empty() {
        return Err("opinion missing author_address".into());
    }
    if payload.subject_field_id.is_empty() {
        return Err("opinion missing subject_field_id".into());
    }
    if payload.title.is_empty() {
        return Err("opinion missing title".into());
    }
    if payload.video_cid.is_empty() {
        return Err("opinion missing video_cid".into());
    }
    if payload.credential_proof_ids.is_empty() {
        return Err("opinion missing credential_proof_ids".into());
    }
    if payload.credential_proof_ids.len() > MAX_OPINION_CREDENTIAL_PROOFS {
        return Err(format!(
            "opinion references too many credentials (max {MAX_OPINION_CREDENTIAL_PROOFS})"
        ));
    }

    // Envelope signer == claimed author
    if payload.author_address != message.stake_address {
        return Err("opinion author does not match envelope signer".into());
    }

    // Deterministic ID check — prevents a peer from laundering
    // someone else's video under a new opinion_id.
    let expected_id = entity_id(&[&payload.author_address, &payload.video_cid]);
    if payload.opinion_id != expected_id {
        return Err("opinion has invalid deterministic opinion_id".into());
    }

    // Ed25519 signature verification over the canonical payload.
    let verifying_key = verify_payload_signature(&payload, message)?;
    let author_did = crate::crypto::did::did_from_verifying_key(&verifying_key);

    // subject_field must exist locally (can't verify credentials if
    // we don't know the taxonomy yet — drop, don't queue. Taxonomy
    // syncs via its own gossip topic; the publisher will try again.)
    let sf_exists: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM subject_fields WHERE id = ?1",
            params![payload.subject_field_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if sf_exists == 0 {
        return Err(format!(
            "unknown subject_field_id '{}' — taxonomy not synced yet",
            payload.subject_field_id
        ));
    }

    // Without a pinned policy for this field nothing can qualify, so there
    // is nothing to wait for.
    if policies
        .applicable(
            QualificationAction::OpinionPosting,
            &payload.subject_field_id,
        )
        .is_none()
    {
        return Err(opinion_refusal_message(
            &payload.subject_field_id,
            Some(NotQualifiedReason::NoApplicablePolicy),
        ));
    }

    // At least one reference must satisfy the policy. Unknown and pending
    // references may still arrive; a conclusive refusal with nothing
    // qualifying rejects the opinion under our current view.
    let mut any_qualifying = false;
    let mut refusal = None;
    for proof_id in &payload.credential_proof_ids {
        match check_opinion_credential(
            db.conn(),
            policies,
            proof_id,
            &author_did,
            &payload.subject_field_id,
            verification_time,
        )? {
            OpinionCredentialEligibility::Unknown | OpinionCredentialEligibility::Pending(_) => {}
            OpinionCredentialEligibility::Unqualified(reason) => {
                refusal.get_or_insert(reason);
            }
            OpinionCredentialEligibility::Qualified(_) => {
                any_qualifying = true;
                break;
            }
        }
    }

    // Idempotency: if we've already stored this opinion at the same
    // or newer published_at, skip.
    let existing_published_at: Option<String> = db
        .conn()
        .query_row(
            "SELECT published_at FROM opinions WHERE id = ?1",
            params![payload.opinion_id],
            |row| row.get(0),
        )
        .ok();
    if existing_published_at.is_some() {
        return Ok(OpinionIngest::Ignored);
    }

    let credential_proof_ids_json =
        serde_json::to_string(&payload.credential_proof_ids).unwrap_or_else(|_| "[]".into());
    let published_at_str = chrono::DateTime::from_timestamp(payload.published_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    let signature_hex = hex::encode(&message.signature);
    let public_key_hex = hex::encode(&message.public_key);

    if any_qualifying {
        db.conn()
            .execute(
                "INSERT INTO opinions (id, author_address, subject_field_id, title, summary, \
                 video_cid, thumbnail_cid, duration_seconds, credential_proof_ids, signature, \
                 public_key, published_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT(id) DO NOTHING",
                params![
                    payload.opinion_id,
                    payload.author_address,
                    payload.subject_field_id,
                    payload.title,
                    payload.summary,
                    payload.video_cid,
                    payload.thumbnail_cid,
                    payload.duration_seconds,
                    credential_proof_ids_json,
                    signature_hex,
                    public_key_hex,
                    published_at_str,
                ],
            )
            .map_err(|e| format!("insert opinion: {e}"))?;
        Ok(OpinionIngest::Stored)
    } else if refusal.is_none() {
        // Queue for later — the referenced credentials or their status
        // evidence may arrive via VC gossip (`vc-did` / `vc-status`), at
        // which point a sweeper promotes queued opinions into the main table.
        db.conn()
            .execute(
                "INSERT INTO opinions_pending_verification (id, author_address, subject_field_id, \
                 title, summary, video_cid, thumbnail_cid, duration_seconds, \
                 credential_proof_ids, signature, public_key, published_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT(id) DO NOTHING",
                params![
                    payload.opinion_id,
                    payload.author_address,
                    payload.subject_field_id,
                    payload.title,
                    payload.summary,
                    payload.video_cid,
                    payload.thumbnail_cid,
                    payload.duration_seconds,
                    credential_proof_ids_json,
                    signature_hex,
                    public_key_hex,
                    published_at_str,
                ],
            )
            .map_err(|e| format!("queue pending opinion: {e}"))?;
        Ok(OpinionIngest::Pending)
    } else {
        // A referenced credential was conclusively refused and none
        // qualify the author to post in this subject field. Hard-reject —
        // invalid under our current view. Reputation scoring will
        // penalize the sender per the opinions topic
        // `invalid_message_deliveries_weight`.
        Err(opinion_refusal_message(&payload.subject_field_id, refusal))
    }
}

/// Promote any queued opinions whose referenced credentials now satisfy the
/// embedded network's pinned qualification policy. Intended to be called
/// after inbound VC gossip updates.
pub fn promote_pending_opinions(db: &Database) -> Result<u32, String> {
    let policies = embedded_qualification_policies().map_err(|error| error.to_string())?;
    promote_pending_opinions_with(db, policies, &verification_time_now())
}

fn promote_pending_opinions_with(
    db: &Database,
    policies: &QualificationPolicySet,
    verification_time: &str,
) -> Result<u32, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, author_address, subject_field_id, title, summary, video_cid, \
             thumbnail_cid, duration_seconds, credential_proof_ids, signature, public_key, \
             published_at \
             FROM opinions_pending_verification",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<(String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,                              // id
                row.get::<_, String>(2)?,                              // subject_field_id
                row.get::<_, String>(8)?,                              // credential_proof_ids_json
                row.get::<_, Option<String>>(10)?.unwrap_or_default(), // public_key
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    crate::db::with_transaction(db.conn(), || {
        let mut promoted = 0_u32;
        for (opinion_id, subject_field_id, proof_ids_json, public_key_hex) in rows {
            let public_key: [u8; 32] = match hex::decode(public_key_hex)
                .ok()
                .and_then(|bytes| bytes.try_into().ok())
            {
                Some(public_key) => public_key,
                None => continue,
            };
            let verifying_key = match VerifyingKey::from_bytes(&public_key) {
                Ok(verifying_key) => verifying_key,
                Err(_) => continue,
            };
            let author_did = crate::crypto::did::did_from_verifying_key(&verifying_key);
            let proof_ids: Vec<String> = serde_json::from_str(&proof_ids_json).unwrap_or_default();
            let mut qualifies = false;
            for pid in proof_ids.iter().take(MAX_OPINION_CREDENTIAL_PROOFS) {
                if matches!(
                    check_opinion_credential(
                        db.conn(),
                        policies,
                        pid,
                        &author_did,
                        &subject_field_id,
                        verification_time,
                    )?,
                    OpinionCredentialEligibility::Qualified(_)
                ) {
                    qualifies = true;
                    break;
                }
            }
            if !qualifies {
                continue;
            }
            db.conn()
                .execute(
                    "INSERT INTO opinions (id, author_address, subject_field_id, title, summary, \
                     video_cid, thumbnail_cid, duration_seconds, credential_proof_ids, signature, \
                     public_key, published_at) \
                     SELECT id, author_address, subject_field_id, title, summary, \
                            video_cid, thumbnail_cid, duration_seconds, credential_proof_ids, \
                            signature, public_key, published_at \
                     FROM opinions_pending_verification WHERE id = ?1 \
                     ON CONFLICT(id) DO NOTHING",
                    params![opinion_id],
                )
                .map_err(|e| format!("promote pending opinion: {e}"))?;
            db.conn()
                .execute(
                    "DELETE FROM opinions_pending_verification WHERE id = ?1",
                    params![opinion_id],
                )
                .map_err(|e| e.to_string())?;
            promoted += 1;
        }
        Ok(promoted)
    })
}

/// Verify the signature on a payload matches the envelope signer's key.
/// Uses `message.public_key` (which is pinned to `message.stake_address`
/// via the TOFU binding in `p2p::validation`).
fn verify_payload_signature(
    payload: &OpinionPayload,
    message: &SignedGossipMessage,
) -> Result<VerifyingKey, String> {
    let payload_bytes = serde_json::to_vec(payload)
        .map_err(|e| format!("serialize opinion payload for verify: {e}"))?;

    if message.public_key.len() != 32 {
        return Err("envelope public_key is not 32 bytes".into());
    }
    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(&message.public_key);
    let verifying_key =
        VerifyingKey::from_bytes(&pk_bytes).map_err(|e| format!("parse verifying key: {e}"))?;

    if message.signature.len() != 64 {
        return Err("envelope signature is not 64 bytes".into());
    }
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(&message.signature);
    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify_strict(&payload_bytes, &signature)
        .map_err(|e| format!("opinion signature verification failed: {e}"))?;
    Ok(verifying_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::opinion_eligibility::test_support;
    use crate::p2p::types::SignedGossipMessage;
    use alexandria_verify::Did;
    use ed25519_dalek::{Signer, SigningKey};

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn test_did() -> String {
        crate::crypto::did::did_from_verifying_key(&test_key().verifying_key())
            .as_str()
            .to_string()
    }

    fn seed_taxonomy(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf_cs', 'Computer Science')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) \
                 VALUES ('sub_algo', 'Algorithms', 'sf_cs')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, subject_id, bloom_level) \
                 VALUES ('skill_graphs', 'Graph Theory', 'sub_algo', 'apply')",
                [],
            )
            .unwrap();
    }

    /// The accepted instructor in the test policy for `sf_cs`.
    fn instructor_key() -> SigningKey {
        SigningKey::from_bytes(&[3u8; 32])
    }

    fn test_policies() -> QualificationPolicySet {
        test_support::policy_set(&[&instructor_key()], &["sf_cs"])
    }

    fn ingest(db: &Database, message: &SignedGossipMessage) -> Result<OpinionIngest, String> {
        handle_opinion_message_with(db, message, &test_policies(), test_support::NOW)
    }

    fn promote(db: &Database) -> Result<u32, String> {
        promote_pending_opinions_with(db, &test_policies(), test_support::NOW)
    }

    fn seed_qualifying_proof(db: &Database, cred_id: &str) {
        seed_credential(db, cred_id, 2 /* apply */);
    }

    fn seed_credential(db: &Database, cred_id: &str, level: u8) {
        seed_credential_for(db, cred_id, level, &test_did());
    }

    /// Stores a signed skill credential issued by the accepted instructor.
    fn seed_credential_for(db: &Database, cred_id: &str, level: u8, subject_did: &str) {
        test_support::store_skill_credential(
            db,
            cred_id,
            &instructor_key(),
            &Did(subject_did.to_string()),
            "skill_graphs",
            level,
        );
    }

    fn opinion_count(db: &Database, table: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn rejects_self_issued_credential_even_at_high_level() {
        let db = test_db();
        seed_taxonomy(&db);
        let key = test_key();
        test_support::store_skill_credential(
            &db,
            "self_claim",
            &key,
            &Did(test_did()),
            "skill_graphs",
            5,
        );
        let payload = build_payload(vec!["self_claim".into()], "cid_self");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let message = sign_message(&key, "/alexandria/opinions/1.0", &bytes);

        let error = ingest(&db, &message).unwrap_err();

        assert!(
            error.contains("your own claims"),
            "unexpected error: {error}"
        );
        assert_eq!(opinion_count(&db, "opinions"), 0);
        assert_eq!(opinion_count(&db, "opinions_pending_verification"), 0);
    }

    #[test]
    fn rejects_every_opinion_without_a_pinned_policy() {
        let db = test_db();
        seed_taxonomy(&db);
        seed_qualifying_proof(&db, "proof_a");
        let key = test_key();
        let payload = build_payload(vec!["proof_a".into()], "cid_no_policy");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let message = sign_message(&key, "/alexandria/opinions/1.0", &bytes);

        let error = handle_opinion_message_with(
            &db,
            &message,
            &test_support::empty_policy_set(),
            test_support::NOW,
        )
        .unwrap_err();

        assert!(
            error.contains("no pinned qualification policy"),
            "unexpected error: {error}"
        );
        assert_eq!(opinion_count(&db, "opinions"), 0);
        assert_eq!(opinion_count(&db, "opinions_pending_verification"), 0);
    }

    #[test]
    fn rejects_announcements_with_too_many_credential_references() {
        let db = test_db();
        seed_taxonomy(&db);
        let key = test_key();
        let proof_ids = (0..=MAX_OPINION_CREDENTIAL_PROOFS)
            .map(|index| format!("proof_{index}"))
            .collect();
        let payload = build_payload(proof_ids, "cid_many");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let message = sign_message(&key, "/alexandria/opinions/1.0", &bytes);

        let error = ingest(&db, &message).unwrap_err();

        assert!(error.contains("too many"), "unexpected error: {error}");
        assert_eq!(opinion_count(&db, "opinions_pending_verification"), 0);
    }

    fn sign_message(key: &SigningKey, topic: &str, payload: &[u8]) -> SignedGossipMessage {
        let signature = key.sign(payload);
        SignedGossipMessage {
            topic: topic.to_string(),
            payload: payload.to_vec(),
            signature: signature.to_bytes().to_vec(),
            public_key: key.verifying_key().to_bytes().to_vec(),
            stake_address: "stake_test1uqauthor".into(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            encrypted: false,
            key_id: None,
        }
    }

    fn build_payload(proof_ids: Vec<String>, video_cid: &str) -> OpinionPayload {
        let author = "stake_test1uqauthor";
        let opinion_id = entity_id(&[author, video_cid]);
        OpinionPayload {
            opinion_id,
            author_address: author.to_string(),
            subject_field_id: "sf_cs".into(),
            title: "Take".into(),
            summary: None,
            video_cid: video_cid.to_string(),
            thumbnail_cid: None,
            duration_seconds: Some(120),
            credential_proof_ids: proof_ids,
            published_at: chrono::Utc::now().timestamp(),
        }
    }

    #[test]
    fn stores_when_credentials_qualify() {
        let db = test_db();
        seed_taxonomy(&db);
        seed_qualifying_proof(&db, "proof_a");
        let key = test_key();
        let payload = build_payload(vec!["proof_a".into()], "cid_a");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        let outcome = ingest(&db, &msg).unwrap();
        assert_eq!(outcome, OpinionIngest::Stored);
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM opinions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn queues_when_credentials_unknown() {
        let db = test_db();
        seed_taxonomy(&db);
        // No proof seeded — credential is unknown
        let key = test_key();
        let payload = build_payload(vec!["proof_unknown".into()], "cid_b");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        let outcome = ingest(&db, &msg).unwrap();
        assert_eq!(outcome, OpinionIngest::Pending);
        let queued: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM opinions_pending_verification",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn rejects_when_unknown_subject_field() {
        let db = test_db();
        // No taxonomy seeded at all
        let key = test_key();
        let payload = build_payload(vec!["proof_a".into()], "cid_c");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        let err = ingest(&db, &msg).unwrap_err();
        assert!(err.contains("unknown subject_field_id"));
    }

    #[test]
    fn rejects_when_credential_is_below_apply_level() {
        // A known credential at `understand` (level 1) should be
        // rejected outright — it's not queued, because the id is
        // already present locally and failing the proficiency gate.
        let db = test_db();
        seed_taxonomy(&db);
        seed_credential(&db, "proof_below_apply", 1 /* understand */);
        let key = test_key();
        let payload = build_payload(vec!["proof_below_apply".into()], "cid_below");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        let err = ingest(&db, &msg).unwrap_err();
        assert!(err.contains("none qualify"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_credential_owned_by_different_signer() {
        let db = test_db();
        seed_taxonomy(&db);
        seed_credential_for(&db, "proof_foreign", 5, "did:key:zForeign");
        let key = test_key();
        let payload = build_payload(vec!["proof_foreign".into()], "cid_foreign");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let message = sign_message(&key, "/alexandria/opinions/1.0", &bytes);

        let error = ingest(&db, &message).unwrap_err();

        assert!(error.contains("none qualify"), "unexpected error: {error}");
        let stored: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM opinions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, 0);
    }

    #[test]
    fn rejects_when_signer_mismatch() {
        let db = test_db();
        seed_taxonomy(&db);
        seed_qualifying_proof(&db, "proof_a");
        let key = test_key();
        let payload = build_payload(vec!["proof_a".into()], "cid_d");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let mut msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        msg.stake_address = "stake_test1uq_someone_else".into();
        let err = ingest(&db, &msg).unwrap_err();
        assert!(err.contains("author does not match envelope signer"));
    }

    #[test]
    fn promote_pending_when_proof_arrives() {
        let db = test_db();
        seed_taxonomy(&db);
        // Queue first (proof unknown)
        let key = test_key();
        let payload = build_payload(vec!["proof_late".into()], "cid_late");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        ingest(&db, &msg).unwrap();
        // Now the proof shows up
        seed_qualifying_proof(&db, "proof_late");
        let n = promote(&db).unwrap();
        assert_eq!(n, 1);
        let stored: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM opinions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, 1);
        let queued: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM opinions_pending_verification",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(queued, 0);
    }

    #[test]
    fn pending_promotion_rolls_back_when_queue_delete_fails() {
        let db = test_db();
        seed_taxonomy(&db);
        let key = test_key();
        let payload = build_payload(vec!["proof_late".into()], "cid_atomic");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let message = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        assert_eq!(ingest(&db, &message).unwrap(), OpinionIngest::Pending);
        seed_qualifying_proof(&db, "proof_late");
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_pending_opinion_delete \
                 BEFORE DELETE ON opinions_pending_verification \
                 BEGIN SELECT RAISE(ABORT, 'injected delete failure'); END;",
            )
            .unwrap();

        let error = promote(&db).unwrap_err();

        assert!(error.contains("injected delete failure"));
        let stored: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM opinions", [], |row| row.get(0))
            .unwrap();
        let pending: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM opinions_pending_verification",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, 0);
        assert_eq!(pending, 1);

        db.conn()
            .execute_batch("DROP TRIGGER fail_pending_opinion_delete")
            .unwrap();
        assert_eq!(promote(&db).unwrap(), 1);
    }

    #[test]
    fn ignores_duplicate_announcement() {
        let db = test_db();
        seed_taxonomy(&db);
        seed_qualifying_proof(&db, "proof_a");
        let key = test_key();
        let payload = build_payload(vec!["proof_a".into()], "cid_dup");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        assert_eq!(ingest(&db, &msg).unwrap(), OpinionIngest::Stored);
        assert_eq!(ingest(&db, &msg).unwrap(), OpinionIngest::Ignored);
    }
}
