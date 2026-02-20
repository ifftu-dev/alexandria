//! Evidence challenge domain types.
//!
//! Types for the evidence challenge mechanism — allows any P2P observer
//! to dispute evidence or credentials. Challenger stakes ADA; DAO
//! committee reviews; outcome is either burn (upheld) or slash (rejected).

use serde::{Deserialize, Serialize};

/// Minimum stake in lovelace (5 ADA).
pub const MIN_STAKE_LOVELACE: u64 = 5_000_000;

/// Default challenge review deadline in days.
pub const CHALLENGE_DEADLINE_DAYS: i64 = 30;

/// Challenge target type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeTargetType {
    Evidence,
    SkillProof,
}

impl ChallengeTargetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChallengeTargetType::Evidence => "evidence",
            ChallengeTargetType::SkillProof => "skill_proof",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "evidence" => Some(Self::Evidence),
            "skill_proof" => Some(Self::SkillProof),
            _ => None,
        }
    }
}

/// Challenge lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeStatus {
    Pending,
    Reviewing,
    Upheld,
    Rejected,
    Expired,
}

impl ChallengeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChallengeStatus::Pending => "pending",
            ChallengeStatus::Reviewing => "reviewing",
            ChallengeStatus::Upheld => "upheld",
            ChallengeStatus::Rejected => "rejected",
            ChallengeStatus::Expired => "expired",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "reviewing" => Some(Self::Reviewing),
            "upheld" => Some(Self::Upheld),
            "rejected" => Some(Self::Rejected),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

/// An evidence challenge record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceChallenge {
    pub id: String,
    pub challenger: String,
    pub target_type: String,
    pub target_ids: Vec<String>,
    pub evidence_cids: Vec<String>,
    pub reason: String,
    pub stake_lovelace: u64,
    pub stake_tx_hash: Option<String>,
    pub status: String,
    pub dao_id: String,
    pub learner_address: String,
    pub reviewed_by: Vec<String>,
    pub resolution_tx: Option<String>,
    pub signature: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub expires_at: Option<String>,
}

/// A vote on a challenge by a DAO committee member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeVote {
    pub id: String,
    pub challenge_id: String,
    pub voter: String,
    pub upheld: bool,
    pub reason: Option<String>,
    pub voted_at: String,
}

/// Parameters for submitting a new challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitChallengeParams {
    pub target_type: String,
    pub target_ids: Vec<String>,
    pub evidence_cids: Vec<String>,
    pub reason: String,
    pub stake_lovelace: u64,
    pub dao_id: String,
    pub learner_address: String,
}

/// Result of resolving a challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResolution {
    pub challenge_id: String,
    pub status: String,
    pub votes_upheld: i64,
    pub votes_rejected: i64,
    pub proofs_invalidated: i64,
    pub reputation_zeroed: bool,
}
