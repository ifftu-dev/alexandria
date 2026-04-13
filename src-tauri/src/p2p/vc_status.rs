//! `TOPIC_VC_STATUS` handler — issuers broadcast status list snapshots
//! and deltas for revocation/suspension.
//!
//! Receivers verify the issuer is known to their `key_registry`
//! (otherwise they have no public key to validate the inner signature
//! against) and then upsert the list, refusing older versions to
//! prevent rollback.

use base64::Engine;

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusIngest {
    Applied,
    IgnoredNewer,
    IgnoredUnknownIssuer,
}

#[derive(serde::Deserialize)]
struct StatusMessage {
    issuer: String,
    /// Optional explicit list_id — if absent we derive
    /// `urn:alexandria:status-list:<issuer>:1` so this matches the
    /// list_id format `commands::credentials` uses for local issuance.
    #[serde(default)]
    list_id: Option<String>,
    version: i64,
    /// Base64-encoded bitmap.
    bits: String,
}

pub fn handle_status_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<StatusIngest, String> {
    let parsed: StatusMessage = match serde_json::from_slice(&message.payload) {
        Ok(p) => p,
        Err(_) => return Ok(StatusIngest::IgnoredUnknownIssuer),
    };

    // Issuer must be known via the local key registry — otherwise we
    // have no public key to validate this list against, and we should
    // wait for the DID doc to arrive before applying. (PR 10's pinning
    // queue can revisit deferred lists, not in-scope here.)
    let known: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM key_registry WHERE did = ?1",
            rusqlite::params![&parsed.issuer],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if known == 0 {
        return Ok(StatusIngest::IgnoredUnknownIssuer);
    }

    let list_id = parsed
        .list_id
        .unwrap_or_else(|| format!("urn:alexandria:status-list:{}:1", parsed.issuer));

    // Refuse older versions to prevent rollback (§11.2).
    let existing: Option<i64> = db
        .conn()
        .query_row(
            "SELECT version FROM credential_status_lists WHERE list_id = ?1",
            rusqlite::params![&list_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(prev) = existing {
        if parsed.version <= prev {
            return Ok(StatusIngest::IgnoredNewer);
        }
    }

    let bits = base64::engine::general_purpose::STANDARD
        .decode(parsed.bits.as_bytes())
        .map_err(|e| format!("base64 decode bits: {e}"))?;
    let bit_length = (bits.len() as i64) * 8;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    db.conn()
        .execute(
            "INSERT INTO credential_status_lists \
             (list_id, issuer_did, version, status_purpose, bits, bit_length, updated_at) \
             VALUES (?1, ?2, ?3, 'revocation', ?4, ?5, ?6) \
             ON CONFLICT(list_id) DO UPDATE SET \
                version = excluded.version, \
                bits = excluded.bits, \
                bit_length = excluded.bit_length, \
                updated_at = excluded.updated_at",
            rusqlite::params![
                &list_id,
                &parsed.issuer,
                parsed.version,
                bits,
                bit_length,
                &now
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(StatusIngest::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_msg(payload: &[u8]) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: "/alexandria/vc-status/1.0".into(),
            payload: payload.to_vec(),
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            stake_address: "stake_test1...".into(),
            timestamp: 1_712_880_000,
            encrypted: false,
            key_id: None,
        }
    }

    /// Pre-register the issuer in `key_registry` so the status
    /// handler sees it as "known". Real propagation gets this from
    /// a prior `handle_did_message` call; tests fast-path the row.
    fn register_issuer(db: &Database, did: &str) {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO key_registry \
                 (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
                 VALUES (?1, 'key-1', '', '1970-01-01T00:00:00Z', NULL, NULL)",
                rusqlite::params![did],
            )
            .unwrap();
    }

    #[test]
    fn fresh_status_list_is_applied() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        register_issuer(&db, "did:key:zI");
        let msg = stub_msg(br#"{"issuer":"did:key:zI","version":1,"bits":"AQID"}"#);
        let out = handle_status_message(&db, &msg).unwrap();
        assert_eq!(out, StatusIngest::Applied);
    }

    #[test]
    fn older_version_is_ignored() {
        // Spec §11.2: status lists are versioned; a lower or equal
        // version MUST NOT overwrite a newer one we already hold.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        register_issuer(&db, "did:key:zI");
        let v2 = stub_msg(br#"{"issuer":"did:key:zI","version":2,"bits":"AgID"}"#);
        assert_eq!(
            handle_status_message(&db, &v2).unwrap(),
            StatusIngest::Applied
        );
        let v1 = stub_msg(br#"{"issuer":"did:key:zI","version":1,"bits":"AQID"}"#);
        let out = handle_status_message(&db, &v1).unwrap();
        assert_eq!(out, StatusIngest::IgnoredNewer);
    }

    #[test]
    fn unknown_issuer_is_deferred() {
        // No registry entry ⇒ we can't validate the inner signature
        // yet, so defer rather than apply or drop.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(br#"{"issuer":"did:key:zUnknown","version":1,"bits":"AQ"}"#);
        let out = handle_status_message(&db, &msg).unwrap();
        assert_eq!(out, StatusIngest::IgnoredUnknownIssuer);
    }
}
