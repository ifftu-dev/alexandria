//! Governance domain types for the DAO coordination protocol.
//!
//! DAOs mirror the knowledge taxonomy (Subject Field → top-level DAO,
//! Subject → sub-DAO). Governance announcements propagate via
//! `/alexandria/governance/1.0` gossip for proposal awareness and
//! coordination. Actual voting happens on-chain (Cardano smart contracts).

use serde::{Deserialize, Serialize};

/// A governance announcement broadcast on `/alexandria/governance/1.0`.
///
/// Used for three purposes:
/// 1. **Proposal awareness**: Notify peers about new proposals
/// 2. **Vote results**: Announce when a proposal is resolved
/// 3. **Committee changes**: Announce DAO committee membership updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceAnnouncement {
    /// Type of governance event.
    pub event_type: GovernanceEventType,
    /// DAO ID this event belongs to.
    pub dao_id: String,
    /// Unix timestamp of the event.
    pub timestamp: i64,
}

/// The type of governance event being announced.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum GovernanceEventType {
    /// A new proposal has been published.
    ProposalCreated {
        proposal_id: String,
        title: String,
        description: Option<String>,
        category: String,
        proposer: String,
    },
    /// A proposal has been resolved (approved or rejected).
    ProposalResolved {
        proposal_id: String,
        status: String,
        votes_for: i64,
        votes_against: i64,
        on_chain_tx: Option<String>,
    },
    /// DAO committee membership has changed.
    CommitteeUpdated {
        /// Stake addresses of current committee members.
        members: Vec<String>,
        /// On-chain transaction that finalized the election.
        on_chain_tx: Option<String>,
    },
}

/// A DAO info record as stored in the local `governance_daos` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub scope_type: String,
    pub scope_id: String,
    pub status: String,
    pub on_chain_tx: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A DAO member record from the `governance_dao_members` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoMember {
    pub dao_id: String,
    pub stake_address: String,
    pub role: String,
    pub joined_at: String,
}
