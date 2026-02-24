//! Governance sync — gossip-based DAO coordination.
//!
//! Handles incoming governance announcements on `/alexandria/governance/1.0`.
//! Three event types:
//!
//! 1. **ProposalCreated**: A new proposal for peer awareness
//! 2. **ProposalResolved**: Outcome of a proposal vote
//! 3. **CommitteeUpdated**: DAO committee membership changes
//!
//! Committee updates are critical — they modify the `governance_dao_members`
//! table which controls the authority check for taxonomy messages.

use rusqlite::params;

use crate::db::Database;
use crate::domain::governance::{GovernanceAnnouncement, GovernanceEventType};
use crate::p2p::types::SignedGossipMessage;

/// Handle an incoming governance announcement from the P2P network.
///
/// Deserializes the message payload, validates, and processes the event.
/// Committee updates modify `governance_dao_members` (affects taxonomy
/// authority checks). Proposal events update `governance_proposals`.
pub fn handle_governance_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<GovernanceAnnouncement, String> {
    let announcement: GovernanceAnnouncement = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid governance announcement: {e}"))?;

    // Validate DAO exists
    if announcement.dao_id.is_empty() {
        return Err("governance announcement missing dao_id".into());
    }

    let signature_hex = hex::encode(&message.signature);

    match &announcement.event_type {
        GovernanceEventType::ProposalCreated {
            proposal_id,
            title,
            description,
            category,
            proposer,
        } => {
            handle_proposal_created(
                db,
                &announcement.dao_id,
                proposal_id,
                title,
                description.as_deref(),
                category,
                proposer,
            )?;
        }
        GovernanceEventType::ProposalResolved {
            proposal_id,
            status,
            votes_for,
            votes_against,
            on_chain_tx,
        } => {
            handle_proposal_resolved(
                db,
                proposal_id,
                status,
                *votes_for,
                *votes_against,
                on_chain_tx.as_deref(),
            )?;
        }
        GovernanceEventType::CommitteeUpdated {
            members,
            on_chain_tx: _,
        } => {
            handle_committee_updated(db, &announcement.dao_id, members, &message.stake_address)?;
        }
    }

    // Record in sync_log
    let entity_id = match &announcement.event_type {
        GovernanceEventType::ProposalCreated { proposal_id, .. } => proposal_id.clone(),
        GovernanceEventType::ProposalResolved { proposal_id, .. } => proposal_id.clone(),
        GovernanceEventType::CommitteeUpdated { .. } => {
            format!("{}_committee", announcement.dao_id)
        }
    };

    db.conn()
        .execute(
            "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature) \
             VALUES ('governance', ?1, 'received', ?2, ?3)",
            params![entity_id, message.stake_address, signature_hex],
        )
        .map_err(|e| format!("failed to record sync_log: {e}"))?;

    Ok(announcement)
}

/// Handle a new proposal creation announcement.
fn handle_proposal_created(
    db: &Database,
    dao_id: &str,
    proposal_id: &str,
    title: &str,
    description: Option<&str>,
    category: &str,
    proposer: &str,
) -> Result<(), String> {
    // Check if the DAO exists locally (best-effort — the DAO may not be synced yet)
    let dao_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !dao_exists {
        log::debug!(
            "Governance: DAO '{}' not in local DB — skipping proposal '{}'",
            dao_id,
            proposal_id,
        );
        return Ok(());
    }

    db.conn()
        .execute(
            "INSERT OR IGNORE INTO governance_proposals \
             (id, dao_id, title, description, category, proposer, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'published')",
            params![proposal_id, dao_id, title, description, category, proposer],
        )
        .map_err(|e| format!("failed to insert proposal: {e}"))?;

    log::info!(
        "Governance: new proposal '{}' in DAO '{}' by {}",
        title,
        dao_id,
        proposer,
    );

    Ok(())
}

/// Handle a proposal resolution announcement.
fn handle_proposal_resolved(
    db: &Database,
    proposal_id: &str,
    status: &str,
    votes_for: i64,
    votes_against: i64,
    on_chain_tx: Option<&str>,
) -> Result<(), String> {
    let rows = db
        .conn()
        .execute(
            "UPDATE governance_proposals SET \
             status = ?1, votes_for = ?2, votes_against = ?3, \
             on_chain_tx = ?4, resolved_at = datetime('now') \
             WHERE id = ?5",
            params![status, votes_for, votes_against, on_chain_tx, proposal_id],
        )
        .map_err(|e| format!("failed to update proposal: {e}"))?;

    if rows == 0 {
        log::debug!(
            "Governance: proposal '{}' not in local DB — skipping resolution",
            proposal_id,
        );
    } else {
        log::info!(
            "Governance: proposal '{}' resolved as '{}' ({} for, {} against)",
            proposal_id,
            status,
            votes_for,
            votes_against,
        );
    }

    Ok(())
}

/// Check if the given stake address is a committee member or chair for a DAO.
fn is_committee_authority(db: &Database, dao_id: &str, stake_address: &str) -> bool {
    db.conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM governance_dao_members \
             WHERE dao_id = ?1 AND stake_address = ?2 AND role IN ('committee', 'chair')",
            params![dao_id, stake_address],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
}

/// Handle a committee membership update.
///
/// Replaces the current committee for the DAO with the new member list.
/// This is critical — it controls who can sign taxonomy updates.
///
/// Security: the gossip sender must be a current committee member or
/// chair of the DAO to authorize a committee change. Unauthenticated
/// committee updates are rejected to prevent governance takeover.
fn handle_committee_updated(
    db: &Database,
    dao_id: &str,
    members: &[String],
    sender_address: &str,
) -> Result<(), String> {
    // Check if the DAO exists
    let dao_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !dao_exists {
        log::debug!(
            "Governance: DAO '{}' not in local DB — skipping committee update",
            dao_id,
        );
        return Ok(());
    }

    // Verify sender is authorized (current committee member or chair)
    if !is_committee_authority(db, dao_id, sender_address) {
        return Err(format!(
            "unauthorized committee update: '{}' is not a committee member or chair of DAO '{}'",
            sender_address, dao_id,
        ));
    }

    // Remove existing committee members for this DAO
    db.conn()
        .execute(
            "DELETE FROM governance_dao_members WHERE dao_id = ?1",
            params![dao_id],
        )
        .map_err(|e| format!("failed to clear committee: {e}"))?;

    // Insert new committee members
    for addr in members {
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES (?1, ?2, 'committee')",
                params![dao_id, addr],
            )
            .map_err(|e| format!("failed to insert committee member: {e}"))?;
    }

    log::info!(
        "Governance: DAO '{}' committee updated by '{}' — {} members",
        dao_id,
        sender_address,
        members.len(),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    /// Insert a stub DAO for FK constraints in tests.
    fn insert_test_dao(db: &Database) {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO subject_fields (id, name) VALUES ('sf1', 'Test Field')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO governance_daos (id, name, scope_type, scope_id, status) \
                 VALUES ('dao1', 'Test DAO', 'subject_field', 'sf1', 'active')",
                [],
            )
            .unwrap();
    }

    fn make_message(announcement: &GovernanceAnnouncement) -> SignedGossipMessage {
        let payload = serde_json::to_vec(announcement).unwrap();
        SignedGossipMessage {
            topic: "/alexandria/governance/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: "stake_test1proposer".into(),
            timestamp: 1_700_000_000,
        }
    }

    #[test]
    fn handle_proposal_created_inserts() {
        let db = test_db();
        insert_test_dao(&db);

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ProposalCreated {
                proposal_id: "prop1".into(),
                title: "Add graph theory skills".into(),
                description: Some("New skills for graph algorithms".into()),
                category: "taxonomy_change".into(),
                proposer: "stake_test1proposer".into(),
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };

        let result = handle_governance_message(&db, &make_message(&ann));
        assert!(result.is_ok());

        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM governance_proposals WHERE id = 'prop1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "Add graph theory skills");
    }

    #[test]
    fn handle_proposal_resolved_updates() {
        let db = test_db();
        insert_test_dao(&db);

        // Create proposal first
        db.conn()
            .execute(
                "INSERT INTO governance_proposals (id, dao_id, title, category, proposer, status) \
                 VALUES ('prop1', 'dao1', 'Test', 'policy', 'stake_test1', 'published')",
                [],
            )
            .unwrap();

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ProposalResolved {
                proposal_id: "prop1".into(),
                status: "approved".into(),
                votes_for: 5,
                votes_against: 2,
                on_chain_tx: Some("tx_hash_123".into()),
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };

        handle_governance_message(&db, &make_message(&ann)).unwrap();

        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM governance_proposals WHERE id = 'prop1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "approved");
    }

    #[test]
    fn handle_committee_updated_replaces_members() {
        let db = test_db();
        insert_test_dao(&db);

        // Insert initial committee — the sender must be among them
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', 'stake_test1proposer', 'committee')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', 'stake_test1old', 'committee')",
                [],
            )
            .unwrap();

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::CommitteeUpdated {
                members: vec!["stake_test1new1".into(), "stake_test1new2".into()],
                on_chain_tx: None,
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };

        // make_message sets stake_address to "stake_test1proposer" (a committee member)
        handle_governance_message(&db, &make_message(&ann)).unwrap();

        // Old members should be gone
        let old_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM governance_dao_members WHERE stake_address = 'stake_test1old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_count, 0);

        // New members should be present
        let new_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM governance_dao_members WHERE dao_id = 'dao1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new_count, 2);
    }

    #[test]
    fn handle_committee_update_rejects_unauthorized_sender() {
        let db = test_db();
        insert_test_dao(&db);

        // Insert committee — sender is NOT a member
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', 'stake_test1chair', 'chair')",
                [],
            )
            .unwrap();

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::CommitteeUpdated {
                members: vec!["stake_test1attacker".into()],
                on_chain_tx: None,
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };

        // make_message sets stake_address to "stake_test1proposer" — NOT in committee
        let result = handle_governance_message(&db, &make_message(&ann));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unauthorized"));

        // Committee should be unchanged
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM governance_dao_members WHERE dao_id = 'dao1' AND stake_address = 'stake_test1chair'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "original committee should be unchanged");
    }

    #[test]
    fn handle_committee_update_allows_chair() {
        let db = test_db();
        insert_test_dao(&db);

        // Set the sender as chair
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', 'stake_test1proposer', 'chair')",
                [],
            )
            .unwrap();

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::CommitteeUpdated {
                members: vec!["stake_test1new1".into()],
                on_chain_tx: None,
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };

        // Should succeed — sender is the chair
        let result = handle_governance_message(&db, &make_message(&ann));
        assert!(result.is_ok());
    }

    #[test]
    fn handle_governance_rejects_empty_dao_id() {
        let db = test_db();
        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::CommitteeUpdated {
                members: vec![],
                on_chain_tx: None,
            },
            dao_id: String::new(),
            timestamp: 0,
        };

        assert!(handle_governance_message(&db, &make_message(&ann)).is_err());
    }

    #[test]
    fn handle_proposal_skips_unknown_dao() {
        let db = test_db();
        // Don't insert the DAO

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ProposalCreated {
                proposal_id: "prop1".into(),
                title: "Test".into(),
                description: None,
                category: "policy".into(),
                proposer: "stake_test1".into(),
            },
            dao_id: "unknown_dao".into(),
            timestamp: 1_700_000_000,
        };

        // Should succeed (graceful skip) but not insert the proposal
        let result = handle_governance_message(&db, &make_message(&ann));
        assert!(result.is_ok());

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM governance_proposals", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
