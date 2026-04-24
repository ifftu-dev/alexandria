//! Credential challenge domain types (VC-first rebuild).
//!
//! A challenger stakes ADA to dispute a specific credential; a DAO
//! committee reviews and votes. If upheld, the credential is revoked
//! via its issuer's RevocationList2020 status list. If rejected, the
//! challenger's stake is slashed (off-chain bookkeeping — the actual
//! slash happens in a separate Cardano tx).

use serde::{Deserialize, Serialize};

/// Minimum stake in lovelace (5 ADA).
pub const MIN_STAKE_LOVELACE: u64 = 5_000_000;

/// Default challenge review deadline in days.
pub const CHALLENGE_DEADLINE_DAYS: i64 = 30;

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

    #[allow(clippy::should_implement_trait)]
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

/// A credential challenge record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialChallenge {
    pub id: String,
    pub challenger: String,
    pub credential_id: String,
    pub reason: String,
    pub stake_lovelace: i64,
    pub stake_tx_hash: Option<String>,
    pub status: String,
    pub dao_id: String,
    pub resolution_tx: Option<String>,
    pub signature: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub expires_at: Option<String>,
}

/// A single committee vote on a challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeVote {
    pub id: String,
    pub challenge_id: String,
    pub voter: String,
    pub upheld: bool,
    pub reason: Option<String>,
    pub voted_at: String,
}

/// Tallied outcome of a challenge resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResolution {
    pub challenge_id: String,
    pub status: String,
    pub votes_for_uphold: i64,
    pub votes_for_reject: i64,
    pub credential_revoked: bool,
}

/// IPC: submit a new challenge against a credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitCredentialChallengeParams {
    pub credential_id: String,
    pub reason: String,
    pub stake_lovelace: i64,
    pub dao_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        for variant in [
            ChallengeStatus::Pending,
            ChallengeStatus::Reviewing,
            ChallengeStatus::Upheld,
            ChallengeStatus::Rejected,
            ChallengeStatus::Expired,
        ] {
            let s = variant.as_str();
            assert_eq!(ChallengeStatus::from_str(s), Some(variant));
        }
    }

    #[test]
    fn min_stake_is_5_ada() {
        assert_eq!(MIN_STAKE_LOVELACE, 5_000_000);
    }
}
