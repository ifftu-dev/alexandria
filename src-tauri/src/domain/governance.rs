//! Governance domain types for the DAO coordination protocol.
//!
//! DAOs mirror the knowledge taxonomy (Subject Field → top-level DAO,
//! Subject → sub-DAO). Governance announcements propagate via
//! `/alexandria/governance/1.0` gossip for proposal awareness and
//! coordination. Actual voting happens on-chain (Cardano smart contracts).
//!
//! Lifecycle states:
//!   Election: nomination → voting → finalized (or cancelled)
//!   Proposal: draft → published → approved|rejected (or cancelled)

use serde::{Deserialize, Serialize};

// ---- P2P Gossip Types ----

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

// ---- DAO Types ----

/// A DAO info record as stored in the local `governance_daos` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub scope_type: String,
    pub scope_id: String,
    pub status: String,
    pub committee_size: i64,
    pub election_interval_days: i64,
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

/// Parameters for creating a new DAO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDaoParams {
    pub name: String,
    pub description: Option<String>,
    pub scope_type: String,
    pub scope_id: String,
    pub committee_size: Option<i64>,
    pub election_interval_days: Option<i64>,
}

// ---- Election Types ----

/// Election phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElectionPhase {
    Nomination,
    Voting,
    Finalized,
    Cancelled,
}

impl ElectionPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            ElectionPhase::Nomination => "nomination",
            ElectionPhase::Voting => "voting",
            ElectionPhase::Finalized => "finalized",
            ElectionPhase::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<ElectionPhase> {
        match s {
            "nomination" => Some(ElectionPhase::Nomination),
            "voting" => Some(ElectionPhase::Voting),
            "finalized" => Some(ElectionPhase::Finalized),
            "cancelled" => Some(ElectionPhase::Cancelled),
            _ => None,
        }
    }
}

/// An election record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Election {
    pub id: String,
    pub dao_id: String,
    pub title: String,
    pub description: Option<String>,
    pub phase: String,
    pub seats: i64,
    pub nominee_min_proficiency: String,
    pub voter_min_proficiency: String,
    pub nomination_start: String,
    pub nomination_end: Option<String>,
    pub voting_end: Option<String>,
    pub on_chain_tx: Option<String>,
    pub created_at: String,
    pub finalized_at: Option<String>,
}

/// An election nominee.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionNominee {
    pub id: String,
    pub election_id: String,
    pub stake_address: String,
    pub accepted: bool,
    pub votes_received: i64,
    pub is_winner: bool,
    pub nominated_at: String,
}

/// An election vote record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionVote {
    pub id: String,
    pub election_id: String,
    pub voter: String,
    pub nominee_id: String,
    pub on_chain_tx: Option<String>,
    pub voted_at: String,
}

/// Parameters for opening a new election.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenElectionParams {
    pub dao_id: String,
    pub title: String,
    pub description: Option<String>,
    pub seats: Option<i64>,
    pub nominee_min_proficiency: Option<String>,
    pub voter_min_proficiency: Option<String>,
    pub nomination_end: Option<String>,
    pub voting_end: Option<String>,
}

// ---- Proposal Types ----

/// Proposal status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Published,
    Approved,
    Rejected,
    Cancelled,
}

impl ProposalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalStatus::Draft => "draft",
            ProposalStatus::Published => "published",
            ProposalStatus::Approved => "approved",
            ProposalStatus::Rejected => "rejected",
            ProposalStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<ProposalStatus> {
        match s {
            "draft" => Some(ProposalStatus::Draft),
            "published" => Some(ProposalStatus::Published),
            "approved" => Some(ProposalStatus::Approved),
            "rejected" => Some(ProposalStatus::Rejected),
            "cancelled" => Some(ProposalStatus::Cancelled),
            _ => None,
        }
    }
}

/// A proposal record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub dao_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub status: String,
    pub proposer: String,
    pub votes_for: i64,
    pub votes_against: i64,
    pub voting_deadline: Option<String>,
    pub min_vote_proficiency: String,
    pub on_chain_tx: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

/// A proposal vote record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalVote {
    pub id: String,
    pub proposal_id: String,
    pub voter: String,
    pub in_favor: bool,
    pub on_chain_tx: Option<String>,
    pub voted_at: String,
}

/// Parameters for submitting a new proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitProposalParams {
    pub dao_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub min_vote_proficiency: Option<String>,
}

// ---- Cardano Governance Types ----

/// Result of a governance transaction submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceTxResult {
    /// Transaction hash (64-char hex).
    pub tx_hash: String,
    /// Description of what was submitted.
    pub action: String,
}

/// On-chain DAO datum fields (matches Aiken DaoDatum).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoDatum {
    pub scope_type: String,
    pub scope_id: String,
    pub name: String,
    pub committee: Vec<String>,
    pub committee_size: i64,
    pub election_interval_days: i64,
}

/// On-chain election datum fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionDatum {
    pub dao_id: String,
    pub phase: String,
    pub seats: i64,
    pub nominees: Vec<String>,
    pub vote_receipt_policy: Option<String>,
}

/// On-chain proposal datum fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalDatum {
    pub dao_id: String,
    pub category: String,
    pub status: String,
    pub votes_for: i64,
    pub votes_against: i64,
    pub vote_receipt_policy: Option<String>,
}
