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
    pub icon_emoji: Option<String>,
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

    #[allow(clippy::should_implement_trait)]
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

    #[allow(clippy::should_implement_trait)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn election_phase_roundtrip() {
        for (variant, expected) in [
            (ElectionPhase::Nomination, "nomination"),
            (ElectionPhase::Voting, "voting"),
            (ElectionPhase::Finalized, "finalized"),
            (ElectionPhase::Cancelled, "cancelled"),
        ] {
            assert_eq!(variant.as_str(), expected);
            let parsed = ElectionPhase::from_str(expected).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn election_phase_from_str_invalid() {
        assert!(ElectionPhase::from_str("").is_none());
        assert!(ElectionPhase::from_str("VOTING").is_none());
        assert!(ElectionPhase::from_str("completed").is_none());
    }

    #[test]
    fn election_phase_serde_roundtrip() {
        for variant in [
            ElectionPhase::Nomination,
            ElectionPhase::Voting,
            ElectionPhase::Finalized,
            ElectionPhase::Cancelled,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: ElectionPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn proposal_status_roundtrip() {
        for (variant, expected) in [
            (ProposalStatus::Draft, "draft"),
            (ProposalStatus::Published, "published"),
            (ProposalStatus::Approved, "approved"),
            (ProposalStatus::Rejected, "rejected"),
            (ProposalStatus::Cancelled, "cancelled"),
        ] {
            assert_eq!(variant.as_str(), expected);
            let parsed = ProposalStatus::from_str(expected).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn proposal_status_from_str_invalid() {
        assert!(ProposalStatus::from_str("").is_none());
        assert!(ProposalStatus::from_str("Draft").is_none());
        assert!(ProposalStatus::from_str("accepted").is_none());
    }

    #[test]
    fn governance_event_type_serde_proposal_created() {
        let event = GovernanceEventType::ProposalCreated {
            proposal_id: "prop1".into(),
            title: "Add new skill".into(),
            description: Some("Description".into()),
            category: "taxonomy".into(),
            proposer: "stake_test1u123".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"ProposalCreated\""));
        let parsed: GovernanceEventType = serde_json::from_str(&json).unwrap();
        if let GovernanceEventType::ProposalCreated { proposal_id, .. } = parsed {
            assert_eq!(proposal_id, "prop1");
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn governance_event_type_serde_proposal_resolved() {
        let event = GovernanceEventType::ProposalResolved {
            proposal_id: "prop1".into(),
            status: "approved".into(),
            votes_for: 10,
            votes_against: 2,
            on_chain_tx: Some("txhash".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: GovernanceEventType = serde_json::from_str(&json).unwrap();
        if let GovernanceEventType::ProposalResolved { votes_for, .. } = parsed {
            assert_eq!(votes_for, 10);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn governance_event_type_serde_committee_updated() {
        let event = GovernanceEventType::CommitteeUpdated {
            members: vec!["addr1".into(), "addr2".into()],
            on_chain_tx: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: GovernanceEventType = serde_json::from_str(&json).unwrap();
        if let GovernanceEventType::CommitteeUpdated { members, .. } = parsed {
            assert_eq!(members.len(), 2);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn governance_announcement_serde_roundtrip() {
        let ann = GovernanceAnnouncement {
            event_type: GovernanceEventType::CommitteeUpdated {
                members: vec!["addr1".into()],
                on_chain_tx: None,
            },
            dao_id: "dao1".into(),
            timestamp: 1700000000,
        };
        let json = serde_json::to_string(&ann).unwrap();
        let parsed: GovernanceAnnouncement = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.dao_id, "dao1");
        assert_eq!(parsed.timestamp, 1700000000);
    }
}
