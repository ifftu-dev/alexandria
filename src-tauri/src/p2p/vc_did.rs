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

    // Rotation announcement: record the new entry. We trust the
    // sender enough to mark a closed historical row — the signed
    // gossip envelope already proves authenticity at this layer.
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
            .get("issuance_date")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let claim_kind = vc
            .pointer("/credential_subject/claim/kind")
            .and_then(|v| v.as_str())
            .unwrap_or("custom")
            .to_string();
        let skill_id = vc
            .pointer("/credential_subject/claim/skill_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);

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

    fn stub_msg(topic: &str, payload: &[u8]) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: topic.into(),
            payload: payload.to_vec(),
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            stake_address: "stake_test1...".into(),
            timestamp: 1_712_880_000,
            encrypted: false,
            key_id: None,
        }
    }

    #[test]
    fn first_observation_of_unknown_did_is_stored() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg("/alexandria/vc-did/1.0", br#"{"did":"did:key:zNew"}"#);
        let outcome = handle_did_message(&db, &msg).unwrap();
        assert_eq!(outcome, DidIngest::Stored);
    }

    #[test]
    fn second_observation_updates_registry_for_rotation() {
        // A later message that carries a rotation record must transition
        // from `Stored` on first sight to `UpdatedRegistry` on the key
        // rotation — this is what lets historical verification (§5.3)
        // survive across peers.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let first = stub_msg("/alexandria/vc-did/1.0", br#"{"did":"did:key:zA"}"#);
        let _ = handle_did_message(&db, &first).unwrap();
        let rotation = stub_msg(
            "/alexandria/vc-did/1.0",
            br#"{"did":"did:key:zA","rotated_to":"did:key:zA2"}"#,
        );
        let outcome = handle_did_message(&db, &rotation).unwrap();
        assert_eq!(outcome, DidIngest::UpdatedRegistry);
    }

    #[test]
    fn malformed_payload_is_ignored_not_errored() {
        // Gossip is noisy. Garbage payloads should return `Ignored`
        // rather than propagating up as errors — an error stops the
        // whole gossip pump for that peer.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg("/alexandria/vc-did/1.0", b"garbage");
        let outcome = handle_did_message(&db, &msg).unwrap();
        assert_eq!(outcome, DidIngest::Ignored);
    }
}
