//! Sentinel prior gossip — `/alexandria/sentinel-priors/1.0`.
//!
//! The community prior library, its ratification and runtime classifier
//! replacement are deleted. The topic stays subscribed until the
//! coordinated wire-protocol removal, and every inbound message is
//! rejected before any database access.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

pub const LEGACY_SENTINEL_PRIORS_DISABLED: &str =
    "Sentinel prior gossip is retired; the community prior library and its ratification are deleted";

/// Reject an inbound Sentinel prior announcement.
pub fn handle_sentinel_prior_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<(), String> {
    Err(LEGACY_SENTINEL_PRIORS_DISABLED.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(db: &Database, table: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn committee_signed_prior_announcement_is_rejected_without_mutation() {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        // No committee row is set up because the tables that carried that
        // authority are not in the baseline schema. The rejection is
        // unconditional, so a sender that once looked authorised is refused
        // on exactly the same path as any other.
        let signer = "stake_test1signer";
        let sync_before = count(&db, "sync_log");

        let announcement = serde_json::json!({
            "prior_id": "prior1",
            "proposal_id": "prop1",
            "cid": "cid-123",
            "model_kind": "keystroke",
            "label": "paste_macro",
            "schema_version": 1,
            "sample_count": 42,
            "notes": null,
            "signature": "deadbeef",
            "ratified_at": "2026-04-18T00:00:00Z",
        });
        let message = SignedGossipMessage {
            topic: crate::p2p::types::TOPIC_SENTINEL_PRIORS.into(),
            payload: serde_json::to_vec(&announcement).unwrap(),
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: signer.into(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        };

        let error = handle_sentinel_prior_message(&db, &message).unwrap_err();

        assert_eq!(error, LEGACY_SENTINEL_PRIORS_DISABLED);
        assert_eq!(count(&db, "sync_log"), sync_before);
        let priors: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'sentinel_priors'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(priors, 0, "sentinel_priors must not exist in the baseline");
    }
}
