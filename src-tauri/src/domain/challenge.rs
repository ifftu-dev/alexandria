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
    /// A posted opinion (Field Commentary video). Upheld challenges mark
    /// the opinion `withdrawn=1` and instruct well-behaved nodes to
    /// unpin the video blob.
    Opinion,
}

impl ChallengeTargetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChallengeTargetType::Evidence => "evidence",
            ChallengeTargetType::SkillProof => "skill_proof",
            ChallengeTargetType::Opinion => "opinion",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "evidence" => Some(Self::Evidence),
            "skill_proof" => Some(Self::SkillProof),
            "opinion" => Some(Self::Opinion),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_correct() {
        assert_eq!(MIN_STAKE_LOVELACE, 5_000_000); // 5 ADA
        assert_eq!(CHALLENGE_DEADLINE_DAYS, 30);
    }

    #[test]
    fn challenge_target_type_roundtrip() {
        for (variant, expected_str) in [
            (ChallengeTargetType::Evidence, "evidence"),
            (ChallengeTargetType::SkillProof, "skill_proof"),
        ] {
            assert_eq!(variant.as_str(), expected_str);
            let parsed = ChallengeTargetType::from_str(expected_str).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn challenge_target_type_from_str_invalid() {
        assert!(ChallengeTargetType::from_str("").is_none());
        assert!(ChallengeTargetType::from_str("Evidence").is_none());
        assert!(ChallengeTargetType::from_str("credential").is_none());
    }

    #[test]
    fn challenge_target_type_serde_roundtrip() {
        for variant in [
            ChallengeTargetType::Evidence,
            ChallengeTargetType::SkillProof,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: ChallengeTargetType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn challenge_status_roundtrip() {
        let all = [
            (ChallengeStatus::Pending, "pending"),
            (ChallengeStatus::Reviewing, "reviewing"),
            (ChallengeStatus::Upheld, "upheld"),
            (ChallengeStatus::Rejected, "rejected"),
            (ChallengeStatus::Expired, "expired"),
        ];
        for (variant, expected_str) in all {
            assert_eq!(variant.as_str(), expected_str);
            let parsed = ChallengeStatus::from_str(expected_str).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn challenge_status_from_str_invalid() {
        assert!(ChallengeStatus::from_str("").is_none());
        assert!(ChallengeStatus::from_str("PENDING").is_none());
        assert!(ChallengeStatus::from_str("resolved").is_none());
    }

    #[test]
    fn challenge_status_serde_roundtrip() {
        for variant in [
            ChallengeStatus::Pending,
            ChallengeStatus::Reviewing,
            ChallengeStatus::Upheld,
            ChallengeStatus::Rejected,
            ChallengeStatus::Expired,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: ChallengeStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn evidence_challenge_serde_roundtrip() {
        let challenge = EvidenceChallenge {
            id: "ch1".into(),
            challenger: "stake_test1uchallenger".into(),
            target_type: "evidence".into(),
            target_ids: vec!["ev1".into(), "ev2".into()],
            evidence_cids: vec!["cid1".into()],
            reason: "suspicious timing".into(),
            stake_lovelace: 5_000_000,
            stake_tx_hash: None,
            status: "pending".into(),
            dao_id: "dao1".into(),
            learner_address: "stake_test1ulearner".into(),
            reviewed_by: vec![],
            resolution_tx: None,
            signature: "sig123".into(),
            created_at: "2025-01-01".into(),
            resolved_at: None,
            expires_at: Some("2025-02-01".into()),
        };
        let json = serde_json::to_string(&challenge).unwrap();
        let parsed: EvidenceChallenge = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.target_ids.len(), 2);
        assert_eq!(parsed.stake_lovelace, 5_000_000);
        assert!(parsed.stake_tx_hash.is_none());
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
