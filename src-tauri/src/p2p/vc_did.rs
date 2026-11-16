//! `TOPIC_VC_DID` handler — issuers announce their DID.
//!
//! A `did:key` is its key, so an announcement records nothing: no registry
//! row is written and nothing can change how the DID resolves. Its one effect
//! is to retry, through full verification, the credentials from that issuer
//! still waiting in the pending queue. Rotation announcements are retired; a
//! `rotated_to` field is ignored.

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
    /// verified and moved into the main table.
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

/// Retry the credentials queued in `credentials_pending_verification` under
/// `issuer_did`, each through [`crate::credentials::receive::receive`], the
/// same full verification every received credential gets.
///
/// An accepted credential is stored and leaves the queue; a refused one, or
/// one that does not parse, leaves the queue unstored; one still pending (a
/// status list not yet held, say) stays queued. Nothing reaches `credentials`
/// without verifying. Returns the number stored.
pub fn promote_pending_for(db: &Database, issuer_did: &str) -> Result<u32, String> {
    use crate::credentials::receive::{receive, Receipt};

    let conn = db.conn();
    let rows: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, signed_vc_json FROM credentials_pending_verification \
                 WHERE issuer_did = ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![issuer_did], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };

    let now = crate::clock::now_rfc3339();
    let mut promoted = 0u32;
    for (id, json) in rows {
        let decided = match alexandria_verify::vc::decode_credential(json.as_bytes()) {
            Err(error) => {
                log::warn!("vc-did: queued credential {id} does not parse: {error}");
                true
            }
            Ok(vc) => match receive(conn, &vc, &now) {
                Ok(Receipt::Stored) => {
                    promoted += 1;
                    true
                }
                Ok(Receipt::AlreadyPresent) => true,
                Ok(Receipt::Rejected(why)) => {
                    log::info!("vc-did: queued credential {id} refused: {}", why.join(", "));
                    true
                }
                Ok(Receipt::Pending(_)) => false,
                Err(error) => {
                    log::warn!("vc-did: queued credential {id}: {error}");
                    false
                }
            },
        };
        if decided {
            unqueue_pending(conn, &id)?;
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

    const QUEUED_ID: &str = "urn:uuid:queued";

    /// Queue a credential `sk` signed, with `status` as its status entries.
    fn queue_credential_from(
        db: &Database,
        sk: &SigningKey,
        status: Option<crate::domain::vc::CredentialStatuses>,
    ) {
        use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
        use crate::domain::vc::{CredentialSubject, Proof, VerifiableCredential};
        let issuer = alexandria_verify::did::derive_did_key(sk);
        let mut properties = serde_json::Map::new();
        properties.insert("skillId".into(), serde_json::json!("s"));
        let vc = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(QUEUED_ID.into()),
            type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
            issuer: issuer.clone().into(),
            valid_from: "2026-04-13T00:00:00Z".into(),
            valid_until: None,
            credential_subject: CredentialSubject {
                id: Did("did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".into()),
                properties,
            },
            credential_status: status,
            terms_of_use: None,
            witness: None,
            integrity: None,
            evidence: Vec::new(),
            related_resource: Vec::new(),
            proof: Proof::unsigned("2026-04-13T00:00:00Z"),
        };
        let vc = sign_credential(UnsignedCredential { credential: vc }, sk, &issuer).unwrap();
        queue_pending(
            db.conn(),
            QUEUED_ID,
            issuer.as_str(),
            vc.credential_subject.id.as_str(),
            &serde_json::to_string(&vc).unwrap(),
        )
        .unwrap();
    }

    /// A revocation entry at slot 0 of `list_id`.
    fn revocation_entry(list_id: &str) -> crate::domain::vc::CredentialStatuses {
        crate::domain::vc::CredentialStatus {
            id: format!("{list_id}#0"),
            type_: "BitstringStatusListEntry".into(),
            status_purpose: "revocation".into(),
            status_list_index: "0".into(),
            status_list_credential: list_id.into(),
        }
        .into()
    }

    fn count(db: &Database, table: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn announce(db: &Database, sk: &SigningKey) -> DidIngest {
        let did = alexandria_verify::did::derive_did_key(sk);
        let payload = format!(r#"{{"did":"{}"}}"#, did.as_str());
        handle_did_message(db, &msg_from(sk, payload.as_bytes())).unwrap()
    }

    /// An announcement retries the queue through full verification: a
    /// credential whose list is not held stays queued, unstored.
    #[test]
    fn a_credential_with_an_unfetchable_list_is_not_promoted() {
        let db = test_db();
        let sk = SigningKey::from_bytes(&[3u8; 32]);
        queue_credential_from(&db, &sk, Some(revocation_entry("urn:test:never-fetched")));

        assert_eq!(announce(&db, &sk), DidIngest::Announced { promoted: 0 });
        assert_eq!(count(&db, "credentials"), 0);
        assert_eq!(count(&db, "credentials_pending_verification"), 1);
    }

    /// A credential its issuer revoked is refused and leaves the queue.
    #[test]
    fn a_revoked_credential_is_not_promoted() {
        let db = test_db();
        let sk = SigningKey::from_bytes(&[4u8; 32]);
        let issuer = alexandria_verify::did::derive_did_key(&sk);
        let list_id = format!("urn:alexandria:status-list:{}:1", issuer.as_str());
        let mut bits = vec![0u8; alexandria_verify::vc::status::MIN_BITS / 8];
        bits[0] = 0x80;
        db.conn()
            .execute(
                "INSERT INTO credential_status_lists \
                 (list_id, issuer_did, version, status_purpose, bits, bit_length) \
                 VALUES (?1, ?2, 1, 'revocation', ?3, ?4)",
                rusqlite::params![list_id, issuer.as_str(), bits, bits.len() as i64 * 8],
            )
            .unwrap();
        queue_credential_from(&db, &sk, Some(revocation_entry(&list_id)));

        assert_eq!(announce(&db, &sk), DidIngest::Announced { promoted: 0 });
        assert_eq!(count(&db, "credentials"), 0);
        assert_eq!(count(&db, "credentials_pending_verification"), 0);
    }

    /// A queued credential that was never signed by its issuer is dropped.
    #[test]
    fn a_forged_queued_credential_is_not_promoted() {
        let db = test_db();
        let sk = SigningKey::from_bytes(&[8u8; 32]);
        queue_credential_from(&db, &sk, None);
        db.conn()
            .execute(
                "UPDATE credentials_pending_verification \
                 SET signed_vc_json = replace(signed_vc_json, '\"skillId\":\"s\"', '\"skillId\":\"t\"')",
                [],
            )
            .unwrap();

        assert_eq!(announce(&db, &sk), DidIngest::Announced { promoted: 0 });
        assert_eq!(count(&db, "credentials"), 0);
        assert_eq!(count(&db, "credentials_pending_verification"), 0);
    }

    /// A `did:key` is its key, so an announcement has nothing to record: it
    /// only promotes credentials that were waiting on that issuer.
    #[test]
    fn an_announcement_promotes_waiting_credentials_and_writes_no_registry_row() {
        let db = test_db();
        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let did = alexandria_verify::did::derive_did_key(&sk);
        queue_credential_from(&db, &sk, None);

        assert_eq!(announce(&db, &sk), DidIngest::Announced { promoted: 1 });
        assert_eq!(count(&db, "credentials"), 1);
        assert_eq!(count(&db, "credentials_pending_verification"), 0);
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
