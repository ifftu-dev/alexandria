//! `TOPIC_VC_STATUS` handler — issuers broadcast status list snapshots
//! and deltas for revocation/suspension. Stub — PR 9.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusIngest {
    Applied,
    IgnoredNewer,
    IgnoredUnknownIssuer,
}

pub fn handle_status_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<StatusIngest, String> {
    unimplemented!("PR 9 — status list propagation")
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

    #[test]
    #[ignore = "pending PR 9 — status list propagation"]
    fn fresh_status_list_is_applied() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(br#"{"issuer":"did:key:zI","version":1,"bits":"AQID"}"#);
        let out = handle_status_message(&db, &msg).unwrap();
        assert_eq!(out, StatusIngest::Applied);
    }

    #[test]
    #[ignore = "pending PR 9 — status list propagation"]
    fn older_version_is_ignored() {
        // Spec §11.2: each revocable credential has a resolvable status
        // reference; the status list itself is versioned. A lower or
        // equal version MUST NOT overwrite a newer one we already hold.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let v2 = stub_msg(br#"{"issuer":"did:key:zI","version":2,"bits":"AgID"}"#);
        let _ = handle_status_message(&db, &v2).unwrap();
        let v1 = stub_msg(br#"{"issuer":"did:key:zI","version":1,"bits":"AQID"}"#);
        let out = handle_status_message(&db, &v1).unwrap();
        assert_eq!(out, StatusIngest::IgnoredNewer);
    }

    #[test]
    #[ignore = "pending PR 9 — status list propagation"]
    fn unknown_issuer_is_deferred() {
        // If we don't yet have a DID doc for the issuer, we can't
        // verify the status list's inner signature — defer instead of
        // applying or dropping.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(br#"{"issuer":"did:key:zUnknown","version":1,"bits":"AQ"}"#);
        let out = handle_status_message(&db, &msg).unwrap();
        assert_eq!(out, StatusIngest::IgnoredUnknownIssuer);
    }
}
