//! Taxonomy gossip — `/alexandria/taxonomy/1.0`.
//!
//! Caller-declared taxonomy ratification is deleted; the skill graph comes
//! from the bundled public taxonomy. The topic stays subscribed until the
//! coordinated wire-protocol removal, and every inbound message is rejected
//! before any database access.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

pub const LEGACY_TAXONOMY_GOSSIP_DISABLED: &str =
    "taxonomy gossip is retired; caller-declared taxonomy ratification is deleted";

/// Reject an inbound taxonomy update.
pub fn handle_taxonomy_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<(), String> {
    Err(LEGACY_TAXONOMY_GOSSIP_DISABLED.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn counts(db: &Database) -> Vec<i64> {
        [
            "subject_fields",
            "subjects",
            "skills",
            "skill_prerequisites",
            "taxonomy_versions",
            "sync_log",
        ]
        .iter()
        .map(|table| {
            db.conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        })
        .collect()
    }

    #[test]
    fn committee_signed_taxonomy_update_is_rejected_without_mutation() {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        let signer = "stake_test1committee";
        db.conn()
            .execute(
                "INSERT INTO governance_daos \
                 (id, name, scope_type, scope_id, status, committee_size, election_interval_days) \
                 VALUES ('dao', 'DAO', 'subject_field', 'scope', 'active', 7, 365)",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role)
                 VALUES ('dao', ?1, 'committee')",
                params![signer],
            )
            .unwrap();
        let before = counts(&db);

        let update = serde_json::json!({
            "version": 1,
            "cid": "taxonomy-cid",
            "previous_cid": null,
            "ratified_by": [signer],
            "ratified_at": "2026-01-01T00:00:00Z",
            "changes": {
                "subject_fields": [{"id": "new_field", "name": "New", "description": null}],
                "subjects": [],
                "skills": [],
                "prerequisites": [],
                "removed_prerequisites": [],
            },
        });
        let message = SignedGossipMessage {
            topic: crate::p2p::types::TOPIC_TAXONOMY.into(),
            payload: serde_json::to_vec(&update).unwrap(),
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: signer.into(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        };

        let error = handle_taxonomy_message(&db, &message).unwrap_err();

        assert_eq!(error, LEGACY_TAXONOMY_GOSSIP_DISABLED);
        assert_eq!(counts(&db), before);
    }
}
