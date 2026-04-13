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
    if inserted > 0 {
        Ok(DidIngest::Stored)
    } else {
        Ok(DidIngest::Ignored)
    }
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
