//! Inbound governance gossip on `/alexandria/governance/1.0`.
//!
//! The obsolete local election, proposal and committee protocol is deleted.
//! Its events carried authority checked only against local committee rows, so
//! every governance event is rejected before any database access until the
//! replacement protocol consumes verified committee certificates. The topic
//! itself is removed together with the relay and observer configuration.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

/// Reject an inbound governance event without reading or writing the
/// database, so a rejected message records no `sync_log` entry and changes
/// no governance rows.
pub fn handle_governance_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<(), String> {
    Err(crate::domain::governance::LEGACY_GOVERNANCE_DISABLED.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sync_log` is the only table a rejected message could still touch: the
    /// governance DAO, proposal and election tables are not in the baseline
    /// schema at all, so there is nothing else left to mutate.
    fn sync_log_rows(db: &Database) -> Vec<String> {
        let mut statement = db.conn().prepare("SELECT * FROM sync_log").unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get::<_, rusqlite::types::Value>(index))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .map(|row| format!("{:?}", row.unwrap()))
            .collect()
    }

    fn message(payload: &str) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: "/alexandria/governance/1.0".into(),
            payload: payload.as_bytes().to_vec(),
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: "stake_test1proposer".into(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        }
    }

    /// Events in the retired wire shape, including ones that once replaced a
    /// committee or resolved a proposal, are refused without touching state.
    #[test]
    fn every_governance_event_is_rejected_without_mutation() {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db.conn()
            .execute_batch("INSERT INTO subject_fields (id, name) VALUES ('sf1', 'Test Field');")
            .unwrap();
        let before = sync_log_rows(&db);

        for payload in [
            r#"{"event_type":{"type":"CommitteeUpdated","data":{"members":["stake_test1attacker"],"on_chain_tx":null}},"dao_id":"dao1","timestamp":1700000000}"#,
            r#"{"event_type":{"type":"ProposalResolved","data":{"proposal_id":"prop1","status":"approved","votes_for":5,"votes_against":0,"on_chain_tx":"tx"}},"dao_id":"dao1","timestamp":1700000000}"#,
            r#"{"event_type":{"type":"ElectionFinalized","data":{"election_id":"elec1","winner_nominee_ids":["nom1"]}},"dao_id":"dao1","timestamp":1700000000}"#,
            "not a governance event",
        ] {
            let error = handle_governance_message(&db, &message(payload)).unwrap_err();
            assert!(error.contains("verified committee certificates"), "{error}");
        }
        assert_eq!(sync_log_rows(&db), before);

        // The sender can no longer hold a local committee row, because the
        // table holding it is gone from the schema rather than merely unread.
        for table in [
            "governance_daos",
            "governance_dao_members",
            "governance_elections",
            "governance_election_nominees",
            "governance_election_votes",
            "governance_proposals",
            "governance_proposal_votes",
        ] {
            let present: i64 = db
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(present, 0, "{table} must not exist in the baseline schema");
        }
    }
}
