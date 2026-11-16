//! `TOPIC_VC_DID` handler — issuers announce their DID.
//!
//! A `did:key` is its key, so an announcement records nothing: no registry
//! row is written and nothing can change how the DID resolves. Its one effect
//! is to promote credentials that were waiting on that issuer. Rotation
//! announcements are retired; a `rotated_to` field is ignored.

use crate::crypto::did::Did;
use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

/// Wire shape of a DID gossip payload: `{"did": "..."}`. Unknown fields,
/// including the retired `rotated_to`, are ignored.
#[derive(serde::Deserialize)]
struct DidMessage {
    did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DidIngest {
    /// The DID's holder announced it; `promoted` credentials waiting on it
    /// moved into the main table.
    Announced {
        promoted: u32,
    },
    Ignored,
    /// The announcement was well-formed but the envelope signer does not
    /// control the DID it speaks for.
    IgnoredNotTheSubject,
}

/// Whether the gossip envelope was signed by the key `did` resolves to.
///
/// `did:key` is self-resolving, so this is a byte comparison against the
/// envelope's public key — no registry lookup, no trust-on-first-use, nothing
/// an attacker can arrange in advance. `p2p::signing` has already verified that
/// the envelope signature is valid for that public key, so an equal key means
/// the sender holds the DID's private key.
fn envelope_signer_controls(did: &Did, message: &SignedGossipMessage) -> bool {
    let Ok(vk_bytes): Result<[u8; 32], _> = message.public_key.clone().try_into() else {
        return false;
    };
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(&vk_bytes) else {
        return false;
    };
    alexandria_verify::did::did_from_verifying_key(&vk).as_str() == did.as_str()
}

pub fn handle_did_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<DidIngest, String> {
    // Garbage payload? Drop silently — gossip is lossy.
    let parsed: DidMessage = match serde_json::from_slice(&message.payload) {
        Ok(p) => p,
        Err(_) => return Ok(DidIngest::Ignored),
    };
    let did = Did(parsed.did);

    // A DID announcement speaks for exactly one identity, so only the holder
    // of that identity's key may make one. That also keeps a peer from
    // promoting credentials queued under someone else's DID.
    if !envelope_signer_controls(&did, message) {
        log::warn!(
            "vc-did: announcement for {} was not signed by that DID — dropping",
            did.as_str()
        );
        return Ok(DidIngest::IgnoredNotTheSubject);
    }

    // Credentials from this issuer that landed before its announcement can
    // now be promoted into the main `credentials` table. A failed sweep is
    // retried on the next announcement, so it does not fail the message.
    let promoted = promote_pending_for(db, did.as_str()).unwrap_or_else(|e| {
        log::warn!("vc-did: promoting credentials for {}: {e}", did.as_str());
        0
    });
    Ok(DidIngest::Announced { promoted })
}

/// Promote credentials queued in `credentials_pending_verification`
/// that match `issuer_did` into the main `credentials` table.
/// Called by the DID-doc gossip handler whenever an issuer becomes
/// resolvable. Returns the number of rows promoted.
pub fn promote_pending_for(db: &Database, issuer_did: &str) -> Result<u32, String> {
    let conn = db.conn();
    let rows: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, subject_did, signed_vc_json \
                 FROM credentials_pending_verification \
                 WHERE issuer_did = ?1",
            )
            .map_err(|e| e.to_string())?;
        let iter = stmt
            .query_map(rusqlite::params![issuer_did], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in iter {
            out.push(r.map_err(|e| e.to_string())?);
        }
        out
    };

    let mut promoted = 0u32;
    for (id, subject_did, json) in rows {
        // Parse + extract minimal fields for the hoisted columns.
        // If parsing fails, leave the row in pending — a future DID
        // doc version might fix it, or an operator can clean up.
        let vc: Result<serde_json::Value, _> = serde_json::from_str(&json);
        let vc = match vc {
            Ok(v) => v,
            Err(_) => continue,
        };
        let type_str = vc
            .get("type")
            .and_then(|v| v.as_array())
            .and_then(|a| {
                a.iter()
                    .find_map(|x| x.as_str().filter(|s| *s != "VerifiableCredential"))
            })
            .unwrap_or("Credential")
            .to_string();
        let issuance_date = vc
            .get("validFrom")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // W3C VC v2: claim properties are inline on credentialSubject
        // rather than nested under a `claim` discriminator. We classify
        // by marker property: `skillId` ⇒ skill, `role` ⇒ role, else
        // custom.
        let subject = vc.pointer("/credentialSubject");
        let skill_id = subject
            .and_then(|s| s.get("skillId"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let claim_kind = if skill_id.is_some() {
            "skill"
        } else if subject
            .and_then(|s| s.get("role"))
            .and_then(|v| v.as_str())
            .is_some()
        {
            "role"
        } else {
            "custom"
        }
        .to_string();

        let inserted = conn.execute(
            "INSERT OR IGNORE INTO credentials \
             (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
              issuance_date, signed_vc_json, integrity_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '')",
            rusqlite::params![
                id,
                issuer_did,
                subject_did,
                type_str,
                claim_kind,
                skill_id,
                issuance_date,
                json,
            ],
        );
        if let Ok(n) = inserted {
            if n > 0 {
                conn.execute(
                    "DELETE FROM credentials_pending_verification WHERE id = ?1",
                    rusqlite::params![id],
                )
                .map_err(|e| e.to_string())?;
                promoted += 1;
            }
        }
    }
    Ok(promoted)
}

/// Queue a signed VC whose verification is pending. Queuing the same id
/// again keeps the first copy. `credentials::collect` queues an
/// organisation-issued credential whose status lists could not be read yet
/// and retries it on the next pull.
pub fn queue_pending(
    conn: &rusqlite::Connection,
    id: &str,
    issuer_did: &str,
    subject_did: &str,
    signed_vc_json: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO credentials_pending_verification \
         (id, issuer_did, subject_did, signed_vc_json) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, issuer_did, subject_did, signed_vc_json],
    )
    .map_err(|e| format!("queue_pending: {e}"))?;
    Ok(())
}

/// Take a credential out of the pending queue once a decision on it is no
/// longer pending, accepted or refused.
pub fn unqueue_pending(conn: &rusqlite::Connection, id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM credentials_pending_verification WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| format!("unqueue_pending: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    /// An envelope as `p2p::signing` would produce it: the `public_key` field
    /// is what the signature was made with, and the pipeline has already
    /// verified that pairing before a handler ever runs.
    fn msg_from(sk: &SigningKey, payload: &[u8]) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: "/alexandria/vc-did/1.0".into(),
            payload: payload.to_vec(),
            signature: vec![0u8; 64],
            public_key: sk.verifying_key().to_bytes().to_vec(),
            stake_address: "stake_test1...".into(),
            timestamp: 1_712_880_000,
            encrypted: false,
            key_id: None,
        }
    }

    fn test_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn registry_rows(db: &Database, did: &str) -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM key_registry WHERE did = ?1",
                [did],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn queue_credential_from(db: &Database, issuer: &str) {
        let vc = serde_json::json!({
            "type": ["VerifiableCredential", "FormalCredential"],
            "issuer": issuer,
            "validFrom": "2026-04-13T00:00:00Z",
            "credentialSubject": {"id": "did:key:zSubject", "skillId": "s"},
        });
        queue_pending(
            db.conn(),
            "urn:uuid:queued",
            issuer,
            "did:key:zSubject",
            &vc.to_string(),
        )
        .unwrap();
    }

    /// A `did:key` is its key, so an announcement has nothing to record: it
    /// only promotes credentials that were waiting on that issuer.
    #[test]
    fn an_announcement_promotes_waiting_credentials_and_writes_no_registry_row() {
        let db = test_db();
        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let did = alexandria_verify::did::derive_did_key(&sk);
        queue_credential_from(&db, did.as_str());

        let payload = format!(r#"{{"did":"{}"}}"#, did.as_str());
        let outcome = handle_did_message(&db, &msg_from(&sk, payload.as_bytes())).unwrap();

        assert_eq!(outcome, DidIngest::Announced { promoted: 1 });
        assert_eq!(registry_rows(&db, did.as_str()), 0);
    }

    /// Rotation announcements are retired. A payload that still carries a
    /// `rotated_to` field is read as a plain announcement: it writes nothing
    /// and cannot change how the DID resolves.
    #[test]
    fn a_rotation_field_writes_no_registry_row() {
        let db = test_db();
        let sk = SigningKey::from_bytes(&[2u8; 32]);
        let did = alexandria_verify::did::derive_did_key(&sk);

        let rotation = format!(r#"{{"did":"{}","rotated_to":"did:key:zA2"}}"#, did.as_str());
        let outcome = handle_did_message(&db, &msg_from(&sk, rotation.as_bytes())).unwrap();

        assert_eq!(outcome, DidIngest::Announced { promoted: 0 });
        let rows: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM key_registry", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    /// The same gate stops the unauthenticated insert that let a peer
    /// manufacture the "known issuer" precondition the status-list handler
    /// used to rely on.
    #[test]
    fn an_announcement_for_someone_elses_did_does_not_populate_the_registry() {
        let db = test_db();
        let attacker = SigningKey::from_bytes(&[5u8; 32]);
        let victim_did =
            alexandria_verify::did::derive_did_key(&SigningKey::from_bytes(&[6u8; 32]));

        let payload = format!(r#"{{"did":"{}"}}"#, victim_did.as_str());
        assert_eq!(
            handle_did_message(&db, &msg_from(&attacker, payload.as_bytes())).unwrap(),
            DidIngest::IgnoredNotTheSubject
        );

        assert_eq!(registry_rows(&db, victim_did.as_str()), 0);
    }

    #[test]
    fn malformed_payload_is_ignored_not_errored() {
        // Gossip is noisy. Garbage payloads should return `Ignored`
        // rather than propagating up as errors — an error stops the
        // whole gossip pump for that peer.
        let db = test_db();
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let outcome = handle_did_message(&db, &msg_from(&sk, b"garbage")).unwrap();
        assert_eq!(outcome, DidIngest::Ignored);
    }
}
