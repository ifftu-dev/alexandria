//! `TOPIC_VC_DID` handler — issuers broadcast their DID document and
//! key rotations. Receivers reflect the broadcast into their local
//! `key_registry` so historical verification (§5.3) survives across
//! peers.

use crate::crypto::did::Did;
use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

/// Wire shape of a DID gossip payload. The unit tests pin two
/// shapes: a bare announcement `{"did": "..."}` and a rotation
/// announcement `{"did": "...", "rotated_to": "..."}`. Additional
/// fields can be added later without breaking the parser.
#[derive(serde::Deserialize)]
struct DidMessage {
    did: String,
    #[serde(default)]
    rotated_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DidIngest {
    Stored,
    UpdatedRegistry,
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
    // of that identity's key may make one. Without this the handler accepted a
    // rotation for *any* DID from *any* peer, which is a key-rollback
    // primitive: forging a rotation closes the victim's current registry row
    // and opens one with an empty key, so `resolve_issuer_key` falls through to
    // `did:key` self-resolution — the pre-rotation key. If the rotation
    // happened because that key was compromised, an attacker holding it can
    // restore its acceptance across the network.
    //
    // It also closed the unauthenticated-insert path into `key_registry`, which
    // was what let a peer manufacture the "known issuer" precondition the
    // status-list handler used to rely on.
    if !envelope_signer_controls(&did, message) {
        log::warn!(
            "vc-did: announcement for {} was not signed by that DID — dropping",
            did.as_str()
        );
        return Ok(DidIngest::IgnoredNotTheSubject);
    }

    // Rotation announcement: record the new entry. The envelope is signed by
    // the DID's own key (checked above), so the subject really is asking for
    // this.
    if let Some(rotated_to) = parsed.rotated_to {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let conn = db.conn();
        // Close any open entry for this DID.
        conn.execute(
            "UPDATE key_registry SET valid_until = ?2, rotated_by = ?3 \
             WHERE did = ?1 AND valid_until IS NULL",
            rusqlite::params![did.as_str(), &now, &rotated_to],
        )
        .map_err(|e| e.to_string())?;
        // Insert a new open row for the rotated DID, derivable from
        // self-resolution at verify time.
        let key_id: String = conn
            .query_row(
                "SELECT 'key-' || (COALESCE(MAX(CAST(substr(key_id, 5) AS INTEGER)), 0) + 1) \
                 FROM key_registry WHERE did = ?1",
                rusqlite::params![did.as_str()],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "key-2".to_string());
        conn.execute(
            "INSERT OR IGNORE INTO key_registry \
             (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
             VALUES (?1, ?2, '', ?3, NULL, NULL)",
            rusqlite::params![did.as_str(), &key_id, &now],
        )
        .map_err(|e| e.to_string())?;
        return Ok(DidIngest::UpdatedRegistry);
    }

    // First-sight announcement: record the DID with an empty pubkey
    // (callers can self-resolve via `did:key`). Idempotent insert.
    let inserted = db
        .conn()
        .execute(
            "INSERT OR IGNORE INTO key_registry \
             (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
             VALUES (?1, 'key-1', '', '1970-01-01T00:00:00Z', NULL, NULL)",
            rusqlite::params![did.as_str()],
        )
        .map_err(|e| e.to_string())?;

    // Kick the pending-verification sweeper now that the issuer's
    // DID is known — credentials from this issuer that landed before
    // the DID doc can be promoted into the main `credentials` table.
    let _ = promote_pending_for(db, did.as_str());

    if inserted > 0 {
        Ok(DidIngest::Stored)
    } else {
        Ok(DidIngest::Ignored)
    }
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

/// Queue a signed VC whose issuer DID isn't yet known. Callers are
/// typically the credential gossip handler (a future PR) — for now
/// tests drive this directly.
pub fn queue_pending(
    db: &Database,
    id: &str,
    issuer_did: &str,
    subject_did: &str,
    signed_vc_json: &str,
) -> Result<(), String> {
    db.conn()
        .execute(
            "INSERT OR IGNORE INTO credentials_pending_verification \
             (id, issuer_did, subject_did, signed_vc_json) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, issuer_did, subject_did, signed_vc_json],
        )
        .map_err(|e| format!("queue_pending: {e}"))?;
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

    #[test]
    fn first_observation_of_unknown_did_is_stored() {
        let db = test_db();
        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let did = alexandria_verify::did::derive_did_key(&sk);
        let payload = format!(r#"{{"did":"{}"}}"#, did.as_str());
        let outcome = handle_did_message(&db, &msg_from(&sk, payload.as_bytes())).unwrap();
        assert_eq!(outcome, DidIngest::Stored);
    }

    #[test]
    fn second_observation_updates_registry_for_rotation() {
        // A later message that carries a rotation record must transition
        // from `Stored` on first sight to `UpdatedRegistry` on the key
        // rotation — this is what lets historical verification (§5.3)
        // survive across peers.
        let db = test_db();
        let sk = SigningKey::from_bytes(&[2u8; 32]);
        let did = alexandria_verify::did::derive_did_key(&sk);
        let first = format!(r#"{{"did":"{}"}}"#, did.as_str());
        let _ = handle_did_message(&db, &msg_from(&sk, first.as_bytes())).unwrap();

        let rotation = format!(r#"{{"did":"{}","rotated_to":"did:key:zA2"}}"#, did.as_str());
        let outcome = handle_did_message(&db, &msg_from(&sk, rotation.as_bytes())).unwrap();
        assert_eq!(outcome, DidIngest::UpdatedRegistry);
    }

    /// The finding: any peer could announce a rotation for any DID. That
    /// closes the victim's open registry row and opens an empty-key one, so
    /// verification falls back to `did:key` self-resolution — the key the
    /// rotation was performed to retire.
    #[test]
    fn a_rotation_announced_by_someone_else_is_dropped() {
        let db = test_db();
        let victim = SigningKey::from_bytes(&[3u8; 32]);
        let attacker = SigningKey::from_bytes(&[4u8; 32]);
        let victim_did = alexandria_verify::did::derive_did_key(&victim);

        // The victim publishes their DID normally.
        let announce = format!(r#"{{"did":"{}"}}"#, victim_did.as_str());
        assert_eq!(
            handle_did_message(&db, &msg_from(&victim, announce.as_bytes())).unwrap(),
            DidIngest::Stored
        );

        // The attacker tries to rotate it out from under them.
        let forged = format!(
            r#"{{"did":"{}","rotated_to":"did:key:zAttacker"}}"#,
            victim_did.as_str()
        );
        assert_eq!(
            handle_did_message(&db, &msg_from(&attacker, forged.as_bytes())).unwrap(),
            DidIngest::IgnoredNotTheSubject
        );

        // And the victim's row is untouched — still open, never rotated.
        let closed: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM key_registry WHERE did = ?1 AND valid_until IS NOT NULL",
                rusqlite::params![victim_did.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(closed, 0, "a forged rotation closed the victim's key row");
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

        let rows: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM key_registry WHERE did = ?1",
                rusqlite::params![victim_did.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
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
