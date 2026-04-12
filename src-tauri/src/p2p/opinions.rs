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
//! 5. Check that at least one of the referenced
//!    `credential_proof_ids` corresponds to a local `skill_proof`
//!    under that subject field at level `apply`+. If *no* referenced
//!    proofs are known yet, queue the opinion in
//!    `opinions_pending_verification` — a future sweep can promote it
//!    once the referenced proofs arrive via the evidence topic.
//! 6. If all checks pass, UPSERT into the `opinions` table.
//!
//! The outgoing side (building + publishing) lives in the `publish_opinion`
//! tauri command in `commands::opinions`.

use ed25519_dalek::{Signature, VerifyingKey};
use rusqlite::params;

use crate::crypto::hash::entity_id;
use crate::db::Database;
use crate::domain::opinions::OpinionPayload;
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

/// Handle an incoming opinion announcement from the P2P network.
pub fn handle_opinion_message(
    db: &Database,
    message: &SignedGossipMessage,
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
    verify_payload_signature(&payload, message)?;

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

    // Credential check. We want at least one referenced proof to:
    //   (a) exist locally
    //   (b) be at a qualifying level (apply+)
    //   (c) cover a skill under the target subject_field
    // If (a) fails for ALL proofs, queue the opinion — maybe the
    // referenced proofs will arrive later. If (a) passes for some
    // but (b)+(c) fail for all, reject outright.
    let mut any_known = false;
    let mut any_qualifying = false;
    for proof_id in &payload.credential_proof_ids {
        let known: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM skill_proofs WHERE id = ?1",
                params![proof_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if known == 0 {
            continue;
        }
        any_known = true;
        let ok: i64 = db
            .conn()
            .query_row(
                "SELECT CASE WHEN EXISTS ( \
                   SELECT 1 \
                   FROM skill_proofs p \
                   JOIN skills s ON s.id = p.skill_id \
                   JOIN subjects sub ON sub.id = s.subject_id \
                   WHERE p.id = ?1 \
                     AND sub.subject_field_id = ?2 \
                     AND p.proficiency_level IN ('apply','analyze','evaluate','create') \
                 ) THEN 1 ELSE 0 END",
                params![proof_id, payload.subject_field_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if ok == 1 {
            any_qualifying = true;
            break;
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
    } else if !any_known {
        // Queue for later — the referenced proofs may arrive via the
        // evidence topic, at which point a sweeper promotes queued
        // opinions into the main table.
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
        // We know the referenced proofs but none of them qualify the
        // author to post in this subject field. Hard-reject — invalid
        // under our current view. Reputation scoring will penalize the
        // sender per the opinions topic `invalid_message_deliveries_weight`.
        Err(format!(
            "opinion references known skill_proofs but none qualify under subject_field '{}'",
            payload.subject_field_id
        ))
    }
}

/// Promote any queued opinions whose referenced credential proofs
/// have now landed in `skill_proofs`. Intended to be called after
/// inbound `evidence` or `skill_proof` updates.
pub fn promote_pending_opinions(db: &Database) -> Result<u32, String> {
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

    let mut promoted = 0_u32;
    for (opinion_id, subject_field_id, proof_ids_json, _pk) in rows {
        let proof_ids: Vec<String> = serde_json::from_str(&proof_ids_json).unwrap_or_default();
        let mut qualifies = false;
        for pid in &proof_ids {
            let ok: i64 = db
                .conn()
                .query_row(
                    "SELECT CASE WHEN EXISTS ( \
                       SELECT 1 FROM skill_proofs p \
                       JOIN skills s ON s.id = p.skill_id \
                       JOIN subjects sub ON sub.id = s.subject_id \
                       WHERE p.id = ?1 AND sub.subject_field_id = ?2 \
                         AND p.proficiency_level IN ('apply','analyze','evaluate','create') \
                     ) THEN 1 ELSE 0 END",
                    params![pid, subject_field_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if ok == 1 {
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
}

/// Verify the signature on a payload matches the envelope signer's key.
/// Uses `message.public_key` (which is pinned to `message.stake_address`
/// via the TOFU binding in `p2p::validation`).
fn verify_payload_signature(
    payload: &OpinionPayload,
    message: &SignedGossipMessage,
) -> Result<(), String> {
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
        .map_err(|e| format!("opinion signature verification failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::types::SignedGossipMessage;
    use ed25519_dalek::{Signer, SigningKey};

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
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

    fn seed_qualifying_proof(db: &Database, proof_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO skill_proofs (id, skill_id, proficiency_level, confidence) \
                 VALUES (?1, 'skill_graphs', 'apply', 0.9)",
                rusqlite::params![proof_id],
            )
            .unwrap();
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
        let outcome = handle_opinion_message(&db, &msg).unwrap();
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
        let outcome = handle_opinion_message(&db, &msg).unwrap();
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
        let err = handle_opinion_message(&db, &msg).unwrap_err();
        assert!(err.contains("unknown subject_field_id"));
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
        let err = handle_opinion_message(&db, &msg).unwrap_err();
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
        handle_opinion_message(&db, &msg).unwrap();
        // Now the proof shows up
        seed_qualifying_proof(&db, "proof_late");
        let n = promote_pending_opinions(&db).unwrap();
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
    fn ignores_duplicate_announcement() {
        let db = test_db();
        seed_taxonomy(&db);
        seed_qualifying_proof(&db, "proof_a");
        let key = test_key();
        let payload = build_payload(vec!["proof_a".into()], "cid_dup");
        let bytes = serde_json::to_vec(&payload).unwrap();
        let msg = sign_message(&key, "/alexandria/opinions/1.0", &bytes);
        assert_eq!(
            handle_opinion_message(&db, &msg).unwrap(),
            OpinionIngest::Stored
        );
        assert_eq!(
            handle_opinion_message(&db, &msg).unwrap(),
            OpinionIngest::Ignored
        );
    }
}
