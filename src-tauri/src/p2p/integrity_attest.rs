//! Integrity-attestation gossip — inbound handler for
//! `/alexandria/integrity-attestation/1.0` (§ Integrity→VC bridge P1).
//!
//! Mirrors `p2p::sentinel::handle_sentinel_prior_message` in shape. The
//! flow is automated end to end — no human signs:
//!
//!   learner finalizes a session → broadcasts an attestation request →
//!   committee attestor nodes auto-co-sign the terminal payload →
//!   learner receives each co-signature here and records it.
//!
//! All trust checks live in `record_attestation_impl` (committee
//! membership, registered key binding, signature verification over the
//! session's terminal payload), so a forged announcement from a
//! non-committee peer — or a committee address paired with the wrong
//! key — cannot inflate a session's assurance level.

use crate::commands::integrity::record_attestation_impl;
use crate::db::Database;
use crate::domain::sentinel::IntegrityCoSignAnnouncement;
use crate::p2p::types::SignedGossipMessage;

/// Handle an incoming integrity co-signature announcement. Returns the
/// resolved assurance level on success.
pub fn handle_integrity_cosign_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<String, String> {
    let ann: IntegrityCoSignAnnouncement = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid integrity cosign announcement: {e}"))?;

    // The gossip broadcaster must be the attestor it claims to be — the
    // envelope's stake address is validated by the network layer; bind
    // it to the announcement so a peer can't relay someone else's
    // identity. (The co-signature itself is re-verified below.)
    if message.stake_address != ann.attestor_address {
        return Err(format!(
            "integrity cosign broadcaster '{}' does not match attestor '{}'",
            message.stake_address, ann.attestor_address
        ));
    }

    record_attestation_impl(
        db.conn(),
        &ann.session_id,
        &ann.attestor_address,
        &ann.public_key,
        &ann.signature,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signing::sign;
    use crate::domain::integrity_attestation::attestation_payload;
    use ed25519_dalek::SigningKey;
    use rusqlite::params;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    /// Finalized session + a single-member committee with a registered
    /// key. Returns (stake_address, signing key, terminal payload).
    fn setup(db: &Database) -> (String, SigningKey, Vec<u8>) {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO integrity_sessions
                (id, enrollment_id, status, integrity_score, critical_count, warning_count,
                 started_at, ended_at, commitment_root)
             VALUES ('sess1', NULL, 'completed', 0.9, 0, 0, '2026-01-01T00:00:00Z',
                 '2026-01-01T01:00:00Z', 'root_abc')",
            [],
        )
        .unwrap();
        let addr = "stake_attestor".to_string();
        let k = key(7);
        let pk_hex = hex::encode(k.verifying_key().to_bytes());
        conn.execute(
            "INSERT INTO governance_dao_members (dao_id, stake_address, role, joined_at)
             VALUES ('sentinel-dao', ?1, 'committee', '2026-01-01')",
            params![addr],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO stake_pubkey_registry (stake_address, public_key_hex, valid_from, source)
             VALUES (?1, ?2, 0, 'snapshot')",
            params![addr, pk_hex],
        )
        .unwrap();
        // committee of 1 → threshold 1, so one co-sig => high_assurance
        let payload = attestation_payload(
            "sess1",
            "completed",
            Some(0.9),
            0,
            0,
            "root_abc",
            "2026-01-01T01:00:00Z",
        );
        (addr, k, payload)
    }

    fn message(ann: &IntegrityCoSignAnnouncement, broadcaster: &str) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: "integrity-attestation".into(),
            payload: serde_json::to_vec(ann).unwrap(),
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: broadcaster.into(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        }
    }

    #[test]
    fn valid_cosign_is_recorded_and_promotes() {
        let db = test_db();
        let (addr, k, payload) = setup(&db);
        let s = sign(&payload, &k);
        let ann = IntegrityCoSignAnnouncement {
            session_id: "sess1".into(),
            attestor_address: addr.clone(),
            public_key: hex::encode(&s.public_key),
            signature: hex::encode(&s.signature),
        };
        let level = handle_integrity_cosign_message(&db, &message(&ann, &addr)).unwrap();
        assert_eq!(level, "high_assurance");
    }

    #[test]
    fn broadcaster_must_match_attestor() {
        let db = test_db();
        let (addr, k, payload) = setup(&db);
        let s = sign(&payload, &k);
        let ann = IntegrityCoSignAnnouncement {
            session_id: "sess1".into(),
            attestor_address: addr,
            public_key: hex::encode(&s.public_key),
            signature: hex::encode(&s.signature),
        };
        let err =
            handle_integrity_cosign_message(&db, &message(&ann, "stake_someone_else")).unwrap_err();
        assert!(err.contains("does not match attestor"), "got {err}");
    }

    #[test]
    fn non_committee_attestor_is_rejected() {
        let db = test_db();
        let (_addr, _k, payload) = setup(&db);
        // A non-committee signer with its own (unregistered) key.
        let rogue = key(9);
        let s = sign(&payload, &rogue);
        let ann = IntegrityCoSignAnnouncement {
            session_id: "sess1".into(),
            attestor_address: "stake_rogue".into(),
            public_key: hex::encode(&s.public_key),
            signature: hex::encode(&s.signature),
        };
        let err = handle_integrity_cosign_message(&db, &message(&ann, "stake_rogue")).unwrap_err();
        assert!(err.contains("committee"), "got {err}");
    }
}
