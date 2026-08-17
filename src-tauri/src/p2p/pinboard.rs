//! `TOPIC_PINBOARD` handler — peers broadcast opt-in commitments to
//! pin specific subjects' content for community redundancy.
//! Stub — implementation in PR 10.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PinboardCommitment {
    pub id: String,
    pub pinner_did: String,
    pub subject_did: String,
    pub scope: Vec<String>,
    pub commitment_since: String,
    pub revoked_at: Option<String>,
    pub signature: String,
    pub public_key: String,
}

/// The bytes a pinner signs to authorise a commitment.
///
/// Covers everything that changes what the commitment means: who is pinning,
/// whose content, the scope, since when, and whether it has been revoked.
/// Length-prefixed so no two different commitments collide.
pub fn canonical_commitment_bytes(c: &PinboardCommitment) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"alexandria-pinboard-commitment-v1");
    let scope = c.scope.join(",");
    for field in [
        c.id.as_str(),
        c.pinner_did.as_str(),
        c.subject_did.as_str(),
        scope.as_str(),
        c.commitment_since.as_str(),
        c.revoked_at.as_deref().unwrap_or(""),
    ] {
        out.extend_from_slice(&(field.len() as u64).to_be_bytes());
        out.extend_from_slice(field.as_bytes());
    }
    out
}

/// Sign `commitment` in place with the pinner's key.
///
/// `public_key` is filled from the key rather than taken as an argument, so a
/// commitment cannot be signed by one key and labelled with another.
pub fn sign_commitment(
    commitment: &mut PinboardCommitment,
    signing_key: &ed25519_dalek::SigningKey,
) {
    use base64::Engine;
    use ed25519_dalek::Signer;
    let sig = signing_key.sign(&canonical_commitment_bytes(commitment));
    commitment.signature = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
    commitment.public_key =
        base64::engine::general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes());
}

/// Whether `commitment` carries a valid signature by the DID it names.
///
/// Two things are checked, and both matter: the signature verifies against the
/// embedded `public_key`, and that public key is the one `pinner_did`
/// self-resolves to. Checking only the first would let anyone sign a
/// commitment with their own key and label it with someone else's DID.
pub fn commitment_is_signed(commitment: &PinboardCommitment) -> bool {
    use base64::Engine;

    let Ok(sig_bytes) =
        base64::engine::general_purpose::STANDARD.decode(commitment.signature.as_bytes())
    else {
        return false;
    };
    let Ok(sig_array): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let Ok(pk_bytes) =
        base64::engine::general_purpose::STANDARD.decode(commitment.public_key.as_bytes())
    else {
        return false;
    };
    let Ok(pk_array): Result<[u8; 32], _> = pk_bytes.try_into() else {
        return false;
    };
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(&pk_array) else {
        return false;
    };
    // The key must be the one the claimed DID resolves to, not merely *a* key.
    if alexandria_verify::did::did_from_verifying_key(&vk).as_str() != commitment.pinner_did {
        return false;
    }
    vk.verify_strict(
        &canonical_commitment_bytes(commitment),
        &ed25519_dalek::Signature::from_bytes(&sig_array),
    )
    .is_ok()
}

pub fn handle_pinboard_message(db: &Database, message: &SignedGossipMessage) -> Result<(), String> {
    let commit: PinboardCommitment = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("malformed pinboard payload: {e}"))?;

    // The commitment has always carried `signature` and `public_key`; nothing
    // ever checked them, so the columns were a control that existed in the
    // schema and was enforced nowhere. Ingest was an unauthenticated,
    // unbounded write into `pinboard_observations` by any peer.
    //
    // Today that is local table growth and misreported redundancy. It becomes
    // data loss the moment the §12/§20.4 eviction policy consults this table:
    // forged commitments claiming N peers pin a subject would let a node
    // conclude its own copy is redundant and drop content that in fact exists
    // nowhere else. Checked now, before anything starts trusting it.
    if !commitment_is_signed(&commit) {
        return Err(format!(
            "pinboard commitment {} is not signed by {}",
            commit.id, commit.pinner_did
        ));
    }

    crate::content_store::pinboard::record_observation(db.conn(), &commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_commitment(revoked_at: Option<&str>) -> PinboardCommitment {
        PinboardCommitment {
            id: "commit-1".into(),
            pinner_did: "did:key:zPinner".into(),
            subject_did: "did:key:zSubject".into(),
            scope: vec!["credentials".into()],
            commitment_since: "2026-04-13T00:00:00Z".into(),
            revoked_at: revoked_at.map(Into::into),
            signature: "sig".into(),
            public_key: "pk".into(),
        }
    }

    #[test]
    fn pinboard_commitment_serde_round_trips() {
        // Gossip messages carry these as JSON payloads. Locking the
        // serde surface here prevents silent field-name drift between
        // the handler, the storage layer, and the IPC command.
        let c = stub_commitment(None);
        let s = serde_json::to_string(&c).unwrap();
        let back: PinboardCommitment = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, c.id);
        assert_eq!(back.scope, c.scope);
        assert!(back.revoked_at.is_none());
    }

    #[test]
    fn pinboard_commitment_revocation_round_trips() {
        let c = stub_commitment(Some("2026-05-01T00:00:00Z"));
        let s = serde_json::to_string(&c).unwrap();
        let back: PinboardCommitment = serde_json::from_str(&s).unwrap();
        assert_eq!(back.revoked_at.as_deref(), Some("2026-05-01T00:00:00Z"));
    }

    fn pinner_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[42u8; 32])
    }

    /// A commitment as a real pinner would broadcast it: `pinner_did` matches
    /// the signing key, and the signature covers the whole thing.
    fn signed_commitment(revoked_at: Option<&str>) -> PinboardCommitment {
        let sk = pinner_key();
        let mut c = stub_commitment(revoked_at);
        c.pinner_did = alexandria_verify::did::derive_did_key(&sk)
            .as_str()
            .to_string();
        sign_commitment(&mut c, &sk);
        c
    }

    fn msg_for(c: &PinboardCommitment) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: "/alexandria/pinboard/1.0".into(),
            payload: serde_json::to_vec(c).unwrap(),
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            stake_address: "stake_test1...".into(),
            timestamp: 1_712_880_000,
            encrypted: false,
            key_id: None,
        }
    }

    #[test]
    fn handle_pinboard_message_persists_observation() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let c = signed_commitment(None);
        handle_pinboard_message(&db, &msg_for(&c)).unwrap();
        // Round-trip: the observation must now be findable via the
        // local `list_pinners_for(subject)` query.
        let found = crate::content_store::pinboard::list_pinners_for(
            db.conn(),
            &crate::crypto::did::Did("did:key:zSubject".into()),
        )
        .unwrap();
        assert!(found.iter().any(|c| c.id == "commit-1"));
    }

    /// The finding: `signature` and `public_key` were stored and never
    /// checked, so any peer could write any claim into the table. The local
    /// declaration path even wrote the literal string "unsigned".
    #[test]
    fn an_unsigned_commitment_is_refused() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let c = stub_commitment(None); // signature: "sig", public_key: "pk"
        assert!(handle_pinboard_message(&db, &msg_for(&c)).is_err());

        let found = crate::content_store::pinboard::list_pinners_for(
            db.conn(),
            &crate::crypto::did::Did("did:key:zSubject".into()),
        )
        .unwrap();
        assert!(found.is_empty(), "an unsigned commitment reached the table");
    }

    /// Signing with your own key and labelling it with someone else's DID must
    /// not work — otherwise the signature proves possession of *a* key rather
    /// than of the identity being claimed.
    #[test]
    fn a_commitment_cannot_be_signed_under_someone_elses_did() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let mut c = signed_commitment(None);
        c.pinner_did = "did:key:z6MkubM4drVzMMYqS5wyWo2tqtWgLrGCMY4qNsEaUjHbLbAN".into();
        assert!(handle_pinboard_message(&db, &msg_for(&c)).is_err());
    }

    /// The revocation field is inside the signed material, so a commitment
    /// cannot be un-revoked in flight by stripping it.
    #[test]
    fn stripping_the_revocation_breaks_the_signature() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let mut c = signed_commitment(Some("2026-05-01T00:00:00Z"));
        c.revoked_at = None;
        assert!(handle_pinboard_message(&db, &msg_for(&c)).is_err());
    }
}
