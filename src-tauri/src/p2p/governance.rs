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

use crate::crypto::hash::entity_id;
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
        GovernanceEventType::ElectionVoteRecorded {
            election_id,
            voter,
            nominee_id,
        } => {
            handle_election_vote_recorded(
                db,
                election_id,
                voter,
                nominee_id,
                &message.stake_address,
                &signature_hex,
                &hex::encode(&message.public_key),
            )?;
        }
        GovernanceEventType::ProposalVoteRecorded {
            proposal_id,
            voter,
            in_favor,
        } => {
            handle_proposal_vote_recorded(
                db,
                proposal_id,
                voter,
                *in_favor,
                &message.stake_address,
                &signature_hex,
                &hex::encode(&message.public_key),
            )?;
        }
        GovernanceEventType::ElectionOpened {
            election_id,
            title,
            seats,
            nominee_min_proficiency,
            voter_min_proficiency,
            nomination_end,
            voting_end,
        } => {
            handle_election_opened(
                db,
                &announcement.dao_id,
                election_id,
                title,
                *seats,
                nominee_min_proficiency,
                voter_min_proficiency,
                nomination_end.as_deref(),
                voting_end.as_deref(),
                &message.stake_address,
            )?;
        }
        GovernanceEventType::NomineeSubmitted {
            election_id,
            nominee_id,
            nominee,
        } => {
            handle_nominee_submitted(db, election_id, nominee_id, nominee, &message.stake_address)?;
        }
        GovernanceEventType::NomineeAccepted {
            election_id,
            nominee_id,
        } => {
            handle_nominee_accepted(db, election_id, nominee_id, &message.stake_address)?;
        }
        GovernanceEventType::ElectionStarted { election_id } => {
            handle_election_phase(db, election_id, "voting", &[], &message.stake_address)?;
        }
        GovernanceEventType::ElectionFinalized {
            election_id,
            winner_nominee_ids,
        } => {
            handle_election_phase(
                db,
                election_id,
                "finalized",
                winner_nominee_ids,
                &message.stake_address,
            )?;
        }
    }

    // Record in sync_log
    let entity_id = match &announcement.event_type {
        GovernanceEventType::ProposalCreated { proposal_id, .. } => proposal_id.clone(),
        GovernanceEventType::ProposalResolved { proposal_id, .. } => proposal_id.clone(),
        GovernanceEventType::CommitteeUpdated { .. } => {
            format!("{}_committee", announcement.dao_id)
        }
        GovernanceEventType::ElectionVoteRecorded {
            election_id, voter, ..
        } => entity_id(&[election_id, voter]),
        GovernanceEventType::ProposalVoteRecorded {
            proposal_id, voter, ..
        } => entity_id(&[proposal_id, voter]),
        GovernanceEventType::ElectionOpened { election_id, .. }
        | GovernanceEventType::NomineeSubmitted { election_id, .. }
        | GovernanceEventType::NomineeAccepted { election_id, .. }
        | GovernanceEventType::ElectionStarted { election_id }
        | GovernanceEventType::ElectionFinalized { election_id, .. } => election_id.clone(),
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
    const ALLOWED_STATUSES: &[&str] = &["approved", "rejected", "expired", "withdrawn"];
    if !ALLOWED_STATUSES.contains(&status) {
        return Err(format!("invalid proposal status: '{status}'"));
    }

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

/// Handle a signed election vote gossiped by a peer.
///
/// Persists the vote (with the voter's signature + public key) and
/// increments the nominee tally — but only on the node that holds the
/// referenced election + nominee. Nominees aren't gossiped yet, so a
/// node lacking them skips gracefully (best-effort, like
/// `handle_proposal_created`); the authoritative tally is built by the
/// operator node (which holds the full election) and committed as a
/// Merkle root on-chain at finalize.
#[allow(clippy::too_many_arguments)]
fn handle_election_vote_recorded(
    db: &Database,
    election_id: &str,
    voter: &str,
    nominee_id: &str,
    sender_address: &str,
    signature_hex: &str,
    public_key_hex: &str,
) -> Result<(), String> {
    // A node may only cast its own vote: the gossip author must be the voter.
    if voter != sender_address {
        return Err(format!(
            "election vote voter '{voter}' does not match gossip sender '{sender_address}'"
        ));
    }

    // Best-effort: skip if the election or accepted nominee isn't local.
    let nominee_ok: bool = db
        .conn()
        .query_row(
            "SELECT accepted FROM governance_election_nominees \
             WHERE id = ?1 AND election_id = ?2",
            params![nominee_id, election_id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !nominee_ok {
        log::debug!(
            "Governance: election '{election_id}' / accepted nominee '{nominee_id}' not local — skipping gossiped vote",
        );
        return Ok(());
    }

    let vote_id = entity_id(&[election_id, voter]);
    let inserted = db
        .conn()
        .execute(
            "INSERT OR IGNORE INTO governance_election_votes \
             (id, election_id, voter, nominee_id, signature, public_key) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                vote_id,
                election_id,
                voter,
                nominee_id,
                signature_hex,
                public_key_hex
            ],
        )
        .map_err(|e| format!("failed to insert election vote: {e}"))?;

    if inserted > 0 {
        db.conn()
            .execute(
                "UPDATE governance_election_nominees \
                 SET votes_received = votes_received + 1 WHERE id = ?1",
                params![nominee_id],
            )
            .map_err(|e| format!("failed to increment nominee tally: {e}"))?;
        log::info!("Governance: recorded gossiped election vote for nominee '{nominee_id}'");
    }

    Ok(())
}

/// Handle a signed proposal vote gossiped by a peer. Mirrors
/// `handle_election_vote_recorded`; proposals ARE gossiped, so the
/// referenced proposal is usually present.
#[allow(clippy::too_many_arguments)]
fn handle_proposal_vote_recorded(
    db: &Database,
    proposal_id: &str,
    voter: &str,
    in_favor: bool,
    sender_address: &str,
    signature_hex: &str,
    public_key_hex: &str,
) -> Result<(), String> {
    if voter != sender_address {
        return Err(format!(
            "proposal vote voter '{voter}' does not match gossip sender '{sender_address}'"
        ));
    }

    // Best-effort: only count votes on a proposal that is locally Published.
    let is_published: bool = db
        .conn()
        .query_row(
            "SELECT status = 'published' FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !is_published {
        log::debug!(
            "Governance: proposal '{proposal_id}' not locally published — skipping gossiped vote",
        );
        return Ok(());
    }

    let vote_id = entity_id(&[proposal_id, voter]);
    let in_favor_int: i64 = if in_favor { 1 } else { 0 };
    let inserted = db
        .conn()
        .execute(
            "INSERT OR IGNORE INTO governance_proposal_votes \
             (id, proposal_id, voter, in_favor, signature, public_key) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                vote_id,
                proposal_id,
                voter,
                in_favor_int,
                signature_hex,
                public_key_hex
            ],
        )
        .map_err(|e| format!("failed to insert proposal vote: {e}"))?;

    if inserted > 0 {
        let col = if in_favor {
            "votes_for"
        } else {
            "votes_against"
        };
        db.conn()
            .execute(
                &format!("UPDATE governance_proposals SET {col} = {col} + 1 WHERE id = ?1"),
                params![proposal_id],
            )
            .map_err(|e| format!("failed to increment proposal tally: {e}"))?;
        log::info!("Governance: recorded gossiped proposal vote ({in_favor}) on '{proposal_id}'");
    }

    Ok(())
}

/// Handle a gossiped election open. Replicates the election locally so
/// the node can hold + tally it. Sender must be a committee member of
/// the DAO (mirrors the on-chain "committee opens elections" rule).
#[allow(clippy::too_many_arguments)]
fn handle_election_opened(
    db: &Database,
    dao_id: &str,
    election_id: &str,
    title: &str,
    seats: i64,
    nominee_min_proficiency: &str,
    voter_min_proficiency: &str,
    nomination_end: Option<&str>,
    voting_end: Option<&str>,
    sender_address: &str,
) -> Result<(), String> {
    let dao_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !dao_exists {
        log::debug!("Governance: DAO '{dao_id}' not local — skipping election '{election_id}'");
        return Ok(());
    }
    if !is_committee_authority(db, dao_id, sender_address) {
        return Err(format!(
            "unauthorized election open: '{sender_address}' is not committee of DAO '{dao_id}'"
        ));
    }

    db.conn()
        .execute(
            "INSERT OR IGNORE INTO governance_elections \
             (id, dao_id, title, phase, seats, nominee_min_proficiency, \
              voter_min_proficiency, nomination_end, voting_end) \
             VALUES (?1, ?2, ?3, 'nomination', ?4, ?5, ?6, ?7, ?8)",
            params![
                election_id,
                dao_id,
                title,
                seats,
                nominee_min_proficiency,
                voter_min_proficiency,
                nomination_end,
                voting_end
            ],
        )
        .map_err(|e| format!("failed to insert election: {e}"))?;
    log::info!("Governance: replicated election '{election_id}' in DAO '{dao_id}'");
    Ok(())
}

/// Handle a gossiped self-nomination. Sender must be the nominee.
fn handle_nominee_submitted(
    db: &Database,
    election_id: &str,
    nominee_id: &str,
    nominee: &str,
    sender_address: &str,
) -> Result<(), String> {
    if nominee != sender_address {
        return Err(format!(
            "nomination nominee '{nominee}' does not match gossip sender '{sender_address}'"
        ));
    }
    let election_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !election_exists {
        log::debug!("Governance: election '{election_id}' not local — skipping nominee");
        return Ok(());
    }
    db.conn()
        .execute(
            "INSERT OR IGNORE INTO governance_election_nominees \
             (id, election_id, stake_address, accepted) VALUES (?1, ?2, ?3, 0)",
            params![nominee_id, election_id, nominee],
        )
        .map_err(|e| format!("failed to insert nominee: {e}"))?;
    Ok(())
}

/// Handle a gossiped nomination acceptance. Sender must be the nominee
/// whose row is being accepted.
fn handle_nominee_accepted(
    db: &Database,
    election_id: &str,
    nominee_id: &str,
    sender_address: &str,
) -> Result<(), String> {
    let nominee_addr: Option<String> = db
        .conn()
        .query_row(
            "SELECT stake_address FROM governance_election_nominees \
             WHERE id = ?1 AND election_id = ?2",
            params![nominee_id, election_id],
            |row| row.get(0),
        )
        .ok();
    let Some(addr) = nominee_addr else {
        log::debug!("Governance: nominee '{nominee_id}' not local — skipping accept");
        return Ok(());
    };
    if addr != sender_address {
        return Err(format!(
            "nominee accept by '{sender_address}' but nominee belongs to '{addr}'"
        ));
    }
    db.conn()
        .execute(
            "UPDATE governance_election_nominees SET accepted = 1 WHERE id = ?1",
            params![nominee_id],
        )
        .map_err(|e| format!("failed to accept nominee: {e}"))?;
    Ok(())
}

/// Handle a gossiped election phase transition (voting / finalized).
/// Sender must be committee. Transitions are guarded (voting only from
/// nomination, finalized only from voting). For `finalized`, the winning
/// nominee rows are flagged.
fn handle_election_phase(
    db: &Database,
    election_id: &str,
    new_phase: &str,
    winner_nominee_ids: &[String],
    sender_address: &str,
) -> Result<(), String> {
    let dao_id: Option<String> = db
        .conn()
        .query_row(
            "SELECT dao_id FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| row.get(0),
        )
        .ok();
    let Some(dao_id) = dao_id else {
        log::debug!("Governance: election '{election_id}' not local — skipping phase change");
        return Ok(());
    };
    if !is_committee_authority(db, &dao_id, sender_address) {
        return Err(format!(
            "unauthorized election phase change by '{sender_address}' for DAO '{dao_id}'"
        ));
    }

    let from_phase = if new_phase == "voting" {
        "nomination"
    } else {
        "voting"
    };
    db.conn()
        .execute(
            "UPDATE governance_elections SET phase = ?1 WHERE id = ?2 AND phase = ?3",
            params![new_phase, election_id, from_phase],
        )
        .map_err(|e| format!("failed to update election phase: {e}"))?;

    if new_phase == "finalized" {
        for wid in winner_nominee_ids {
            db.conn()
                .execute(
                    "UPDATE governance_election_nominees SET is_winner = 1 \
                     WHERE id = ?1 AND election_id = ?2",
                    params![wid, election_id],
                )
                .map_err(|e| format!("failed to flag winner: {e}"))?;
        }
    }
    log::info!("Governance: election '{election_id}' → {new_phase}");
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

    // Wrap in a transaction so the committee is never left empty on partial failure
    let tx = db
        .conn()
        .unchecked_transaction()
        .map_err(|e| format!("failed to begin transaction: {e}"))?;

    // Remove existing committee members for this DAO
    tx.execute(
        "DELETE FROM governance_dao_members WHERE dao_id = ?1",
        params![dao_id],
    )
    .map_err(|e| format!("failed to clear committee: {e}"))?;

    // Insert new committee members
    for addr in members {
        tx.execute(
            "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
             VALUES (?1, ?2, 'committee')",
            params![dao_id, addr],
        )
        .map_err(|e| format!("failed to insert committee member: {e}"))?;
    }

    tx.commit()
        .map_err(|e| format!("failed to commit committee update: {e}"))?;

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
            encrypted: false,
            key_id: None,
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

    /// Insert an election in the voting phase with one accepted nominee.
    /// Returns (election_id, nominee_id).
    fn insert_voting_election(db: &Database) -> (String, String) {
        let elec_id = "elec1".to_string();
        db.conn()
            .execute(
                "INSERT INTO governance_elections \
                 (id, dao_id, title, phase, seats) VALUES (?1, 'dao1', 'E', 'voting', 1)",
                params![elec_id],
            )
            .unwrap();
        let nom_id = "nom1".to_string();
        db.conn()
            .execute(
                "INSERT INTO governance_election_nominees \
                 (id, election_id, stake_address, accepted) VALUES (?1, ?2, 'stake_test1nom', 1)",
                params![nom_id, elec_id],
            )
            .unwrap();
        (elec_id, nom_id)
    }

    fn election_vote_ann(
        election_id: &str,
        nominee_id: &str,
        voter: &str,
    ) -> GovernanceAnnouncement {
        GovernanceAnnouncement {
            event_type: GovernanceEventType::ElectionVoteRecorded {
                election_id: election_id.into(),
                voter: voter.into(),
                nominee_id: nominee_id.into(),
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        }
    }

    #[test]
    fn election_vote_inserts_and_tallies() {
        let db = test_db();
        insert_test_dao(&db);
        let (elec, nom) = insert_voting_election(&db);

        // make_message signs as "stake_test1proposer" — voter must match.
        let ann = election_vote_ann(&elec, &nom, "stake_test1proposer");
        handle_governance_message(&db, &make_message(&ann)).unwrap();

        let votes: i64 = db
            .conn()
            .query_row(
                "SELECT votes_received FROM governance_election_nominees WHERE id = ?1",
                params![nom],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(votes, 1);

        // Signature + public key persisted on the vote row.
        let has_sig: bool = db
            .conn()
            .query_row(
                "SELECT signature IS NOT NULL AND public_key IS NOT NULL \
                 FROM governance_election_votes WHERE election_id = ?1",
                params![elec],
                |r| r.get(0),
            )
            .unwrap();
        assert!(has_sig);
    }

    #[test]
    fn election_vote_rejects_voter_mismatch() {
        let db = test_db();
        insert_test_dao(&db);
        let (elec, nom) = insert_voting_election(&db);

        // voter ("stake_test1someoneelse") != gossip sender ("stake_test1proposer")
        let ann = election_vote_ann(&elec, &nom, "stake_test1someoneelse");
        let result = handle_governance_message(&db, &make_message(&ann));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not match gossip sender"));
    }

    #[test]
    fn election_vote_skips_unknown_nominee() {
        let db = test_db();
        insert_test_dao(&db);
        insert_voting_election(&db);

        let ann = election_vote_ann("elec1", "nonexistent_nominee", "stake_test1proposer");
        // Graceful skip — no error, no vote row.
        assert!(handle_governance_message(&db, &make_message(&ann)).is_ok());
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM governance_election_votes", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn election_vote_dedup_counts_once() {
        let db = test_db();
        insert_test_dao(&db);
        let (elec, nom) = insert_voting_election(&db);

        let ann = election_vote_ann(&elec, &nom, "stake_test1proposer");
        handle_governance_message(&db, &make_message(&ann)).unwrap();
        // Same voter, same election — INSERT OR IGNORE, tally stays 1.
        handle_governance_message(&db, &make_message(&ann)).unwrap();

        let votes: i64 = db
            .conn()
            .query_row(
                "SELECT votes_received FROM governance_election_nominees WHERE id = ?1",
                params![nom],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(votes, 1, "duplicate gossiped vote must not double-count");
    }

    #[test]
    fn proposal_vote_inserts_and_tallies() {
        let db = test_db();
        insert_test_dao(&db);
        db.conn()
            .execute(
                "INSERT INTO governance_proposals \
                 (id, dao_id, title, category, proposer, status) \
                 VALUES ('prop1', 'dao1', 'P', 'policy', 'stake_test1', 'published')",
                [],
            )
            .unwrap();

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ProposalVoteRecorded {
                proposal_id: "prop1".into(),
                voter: "stake_test1proposer".into(),
                in_favor: true,
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        handle_governance_message(&db, &make_message(&ann)).unwrap();

        let (vf, va): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT votes_for, votes_against FROM governance_proposals WHERE id = 'prop1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((vf, va), (1, 0));
    }

    #[test]
    fn proposal_vote_skips_unpublished() {
        let db = test_db();
        insert_test_dao(&db);
        db.conn()
            .execute(
                "INSERT INTO governance_proposals \
                 (id, dao_id, title, category, proposer, status) \
                 VALUES ('prop1', 'dao1', 'P', 'policy', 'stake_test1', 'draft')",
                [],
            )
            .unwrap();

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ProposalVoteRecorded {
                proposal_id: "prop1".into(),
                voter: "stake_test1proposer".into(),
                in_favor: true,
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        // Draft proposal — vote skipped gracefully.
        assert!(handle_governance_message(&db, &make_message(&ann)).is_ok());
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM governance_proposal_votes", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    /// Make the gossip sender ("stake_test1proposer") a committee member.
    fn make_sender_committee(db: &Database) {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', 'stake_test1proposer', 'committee')",
                [],
            )
            .unwrap();
    }

    #[test]
    fn election_opened_replicates_for_committee_sender() {
        let db = test_db();
        insert_test_dao(&db);
        make_sender_committee(&db);

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ElectionOpened {
                election_id: "e1".into(),
                title: "T".into(),
                seats: 1,
                nominee_min_proficiency: "remember".into(),
                voter_min_proficiency: "remember".into(),
                nomination_end: None,
                voting_end: None,
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        handle_governance_message(&db, &make_message(&ann)).unwrap();

        let phase: String = db
            .conn()
            .query_row(
                "SELECT phase FROM governance_elections WHERE id = 'e1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(phase, "nomination");
    }

    #[test]
    fn election_opened_rejects_non_committee_sender() {
        let db = test_db();
        insert_test_dao(&db);
        // sender NOT committee
        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ElectionOpened {
                election_id: "e1".into(),
                title: "T".into(),
                seats: 1,
                nominee_min_proficiency: "remember".into(),
                voter_min_proficiency: "remember".into(),
                nomination_end: None,
                voting_end: None,
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        let r = handle_governance_message(&db, &make_message(&ann));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("unauthorized"));
    }

    #[test]
    fn nominee_submitted_then_accepted_flow() {
        let db = test_db();
        insert_test_dao(&db);
        make_sender_committee(&db);
        // Election present (committee opened it).
        db.conn()
            .execute(
                "INSERT INTO governance_elections (id, dao_id, title, phase, seats) \
                 VALUES ('e1', 'dao1', 'T', 'nomination', 1)",
                [],
            )
            .unwrap();

        // Self-nominate: sender == nominee.
        let nom_id = "nomX".to_string();
        let sub = GovernanceAnnouncement {
            event_type: GovernanceEventType::NomineeSubmitted {
                election_id: "e1".into(),
                nominee_id: nom_id.clone(),
                nominee: "stake_test1proposer".into(),
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        handle_governance_message(&db, &make_message(&sub)).unwrap();

        let accepted: bool = db
            .conn()
            .query_row(
                "SELECT accepted FROM governance_election_nominees WHERE id = ?1",
                params![nom_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!accepted);

        // Accept (sender is the nominee).
        let acc = GovernanceAnnouncement {
            event_type: GovernanceEventType::NomineeAccepted {
                election_id: "e1".into(),
                nominee_id: nom_id.clone(),
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        handle_governance_message(&db, &make_message(&acc)).unwrap();

        let accepted: bool = db
            .conn()
            .query_row(
                "SELECT accepted FROM governance_election_nominees WHERE id = ?1",
                params![nom_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(accepted);
    }

    #[test]
    fn nominee_submitted_rejects_sender_mismatch() {
        let db = test_db();
        insert_test_dao(&db);
        db.conn()
            .execute(
                "INSERT INTO governance_elections (id, dao_id, title, phase, seats) \
                 VALUES ('e1', 'dao1', 'T', 'nomination', 1)",
                [],
            )
            .unwrap();
        let sub = GovernanceAnnouncement {
            event_type: GovernanceEventType::NomineeSubmitted {
                election_id: "e1".into(),
                nominee_id: "nomX".into(),
                nominee: "stake_test1someoneelse".into(),
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        assert!(handle_governance_message(&db, &make_message(&sub)).is_err());
    }

    #[test]
    fn election_phase_transition_requires_committee_and_guards() {
        let db = test_db();
        insert_test_dao(&db);
        make_sender_committee(&db);
        db.conn()
            .execute(
                "INSERT INTO governance_elections (id, dao_id, title, phase, seats) \
                 VALUES ('e1', 'dao1', 'T', 'nomination', 1)",
                [],
            )
            .unwrap();

        let started = GovernanceAnnouncement {
            event_type: GovernanceEventType::ElectionStarted {
                election_id: "e1".into(),
            },
            dao_id: "dao1".into(),
            timestamp: 1_700_000_000,
        };
        handle_governance_message(&db, &make_message(&started)).unwrap();
        let phase: String = db
            .conn()
            .query_row(
                "SELECT phase FROM governance_elections WHERE id = 'e1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(phase, "voting");
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

    /// End-to-end of the signed-vote gossip RECEIVE path: a real
    /// Ed25519-signed vote envelope from a registered identity passes the
    /// full `MessageValidator` pipeline (signature → stake↔pubkey
    /// registry identity-binding → freshness → dedup → schema →
    /// authority) and is then tallied by `handle_governance_message`.
    /// This exercises the actual crypto + registry checks, not the stub
    /// signatures used by the other handler tests.
    #[test]
    fn signed_vote_e2e_validate_then_tally() {
        use crate::p2p::signing::sign_gossip_message;
        use crate::p2p::types::TOPIC_GOVERNANCE;
        use crate::p2p::validation::MessageValidator;
        use ed25519_dalek::SigningKey;
        use std::sync::{Arc, Mutex};

        let db = test_db();
        insert_test_dao(&db);
        let (elec, nom) = insert_voting_election(&db);

        // A real signing key + its registered stake↔pubkey binding.
        let key = SigningKey::generate(&mut rand::thread_rng());
        let pubkey_hex = hex::encode(key.verifying_key().to_bytes());
        let voter = "stake_test1uevoter".to_string();
        db.conn()
            .execute(
                "INSERT INTO stake_pubkey_registry \
                 (stake_address, public_key_hex, valid_from, valid_until, source) \
                 VALUES (?1, ?2, 0, NULL, 'snapshot')",
                params![voter, pubkey_hex],
            )
            .unwrap();

        // Build + sign the vote envelope exactly as the cast path does.
        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ElectionVoteRecorded {
                election_id: elec.clone(),
                voter: voter.clone(),
                nominee_id: nom.clone(),
            },
            dao_id: "dao1".into(),
            timestamp: 0,
        };
        let payload = serde_json::to_vec(&ann).unwrap();
        let signed = sign_gossip_message(TOPIC_GOVERNANCE, payload, &key, &voter);

        // Full validation pipeline (with DB so the registry check runs).
        let arc = Arc::new(Mutex::new(Some(db)));
        MessageValidator::with_db(arc.clone())
            .validate(&signed)
            .expect("vote must pass the full validation pipeline");

        // Then handle + assert the tally moved.
        {
            let guard = arc.lock().unwrap();
            let dbref = guard.as_ref().unwrap();
            handle_governance_message(dbref, &signed).expect("handle");

            let votes: i64 = dbref
                .conn()
                .query_row(
                    "SELECT votes_received FROM governance_election_nominees WHERE id = ?1",
                    params![nom],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(votes, 1);

            // The voter's signature + pubkey were persisted on the row.
            let (sig, pk): (Option<String>, Option<String>) = dbref
                .conn()
                .query_row(
                    "SELECT signature, public_key FROM governance_election_votes \
                     WHERE election_id = ?1",
                    params![elec],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert!(sig.is_some() && pk.is_some());
            assert_eq!(pk.unwrap(), pubkey_hex);
        }
    }

    /// A vote from an UNREGISTERED identity (no stake↔pubkey binding) must
    /// be rejected by the validation pipeline's identity-binding step —
    /// registration is required to participate.
    #[test]
    fn signed_vote_rejected_when_unregistered() {
        use crate::p2p::signing::sign_gossip_message;
        use crate::p2p::types::TOPIC_GOVERNANCE;
        use crate::p2p::validation::MessageValidator;
        use ed25519_dalek::SigningKey;
        use std::sync::{Arc, Mutex};

        let db = test_db();
        insert_test_dao(&db);
        let (elec, nom) = insert_voting_election(&db);

        let key = SigningKey::generate(&mut rand::thread_rng());
        let voter = "stake_test1uunregistered".to_string();
        // NOTE: no stake_pubkey_registry row inserted.

        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::ElectionVoteRecorded {
                election_id: elec,
                voter: voter.clone(),
                nominee_id: nom,
            },
            dao_id: "dao1".into(),
            timestamp: 0,
        };
        let payload = serde_json::to_vec(&ann).unwrap();
        let signed = sign_gossip_message(TOPIC_GOVERNANCE, payload, &key, &voter);

        let arc = Arc::new(Mutex::new(Some(db)));
        let result = MessageValidator::with_db(arc).validate(&signed);
        assert!(
            result.is_err(),
            "unregistered identity must fail the registry identity-binding check"
        );
    }
}
