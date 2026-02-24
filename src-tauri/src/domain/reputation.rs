//! Reputation domain types.
//!
//! Types for the full whitepaper reputation engine (§2.3–2.8, §8.2).
//! These support:
//!   - Scoped reputation: `(subject, role, skill, proficiency_level, time_window)`
//!   - Distribution metrics: median, percentiles, variance, learner count
//!   - Instructor rankings per skill scope
//!   - Deterministic recomputation from evidence chains

use serde::{Deserialize, Serialize};

/// Reputation role — defines the capacity in which an actor earned reputation.
///
/// Per whitepaper §2.4: instructor, assessor, author, mentor, learner.
/// Only instructor and learner have active computation pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReputationRole {
    Instructor,
    Learner,
    Assessor,
    Author,
    Mentor,
}

impl ReputationRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReputationRole::Instructor => "instructor",
            ReputationRole::Learner => "learner",
            ReputationRole::Assessor => "assessor",
            ReputationRole::Author => "author",
            ReputationRole::Mentor => "mentor",
        }
    }

    pub fn from_str(s: &str) -> Option<ReputationRole> {
        match s {
            "instructor" => Some(ReputationRole::Instructor),
            "learner" => Some(ReputationRole::Learner),
            "assessor" => Some(ReputationRole::Assessor),
            "author" => Some(ReputationRole::Author),
            "mentor" => Some(ReputationRole::Mentor),
            _ => None,
        }
    }
}

/// Full reputation scope — the 5-tuple that uniquely identifies a
/// reputation assertion per whitepaper §2.3.
///
/// > "Implementations MUST NOT produce or consume reputation values
/// >  that omit any element of this tuple."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationScope {
    pub actor_address: String,
    pub role: ReputationRole,
    pub skill_id: String,
    pub proficiency_level: String,
    /// Time window start (ISO 8601). `None` = unbounded.
    pub window_start: Option<String>,
    /// Time window end (ISO 8601). `None` = unbounded (up to now).
    pub window_end: Option<String>,
}

/// Distribution metrics for an instructor's impact on a skill scope.
///
/// Per whitepaper §2.8: "Reputation MUST be exposed as a distribution,
/// not as a single scalar."
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DistributionMetrics {
    /// Median of all per-learner impact deltas.
    pub median_impact: f64,
    /// 25th percentile of impact distribution.
    pub impact_p25: f64,
    /// 75th percentile of impact distribution.
    pub impact_p75: f64,
    /// Number of distinct learners contributing evidence.
    pub learner_count: i64,
    /// Variance of the impact distribution.
    pub impact_variance: f64,
}

/// A single per-learner impact delta, stored for distribution computation.
///
/// When an instructor's evidence is attributed from a learner's proof
/// update, the delta is recorded here so we can later compute
/// median, percentiles, and variance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactDelta {
    pub id: String,
    /// The reputation assertion this delta contributes to.
    pub assertion_id: String,
    /// The learner whose proof update generated this delta.
    pub learner_address: String,
    /// The confidence change × attribution weight.
    pub delta: f64,
    /// The attribution weight for this instructor from this learner's evidence.
    pub attribution: f64,
    /// When this delta was recorded.
    pub created_at: String,
}

/// A link between a reputation assertion and a skill proof that
/// contributed to it, with the delta and attribution weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationEvidence {
    pub assertion_id: String,
    pub proof_id: String,
    pub delta_confidence: f64,
    pub attribution_weight: f64,
}

/// Full reputation assertion with distribution metrics.
///
/// Extends the base `ReputationAssertion` from `domain::evidence` with
/// the distribution fields required by the whitepaper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullReputationAssertion {
    pub id: String,
    pub actor_address: String,
    pub role: String,
    pub skill_id: Option<String>,
    pub proficiency_level: Option<String>,
    /// Cumulative impact score (sum of attribution-weighted deltas).
    pub score: f64,
    /// Statistical confidence (smoothed for instructors, direct for learners).
    pub confidence: f64,
    pub evidence_count: i64,
    /// Distribution metrics (populated for instructor role).
    pub distribution: Option<DistributionMetrics>,
    pub computation_spec: String,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub updated_at: String,
}

/// Query parameters for reputation lookups.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReputationQuery {
    /// Filter by actor address.
    pub actor_address: Option<String>,
    /// Filter by role.
    pub role: Option<String>,
    /// Filter by skill ID.
    pub skill_id: Option<String>,
    /// Filter by proficiency level.
    pub proficiency_level: Option<String>,
    /// Maximum number of results.
    pub limit: Option<i64>,
}

/// An instructor's ranking entry for a specific skill scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructorRanking {
    pub actor_address: String,
    pub skill_id: String,
    pub proficiency_level: String,
    /// Cumulative impact score.
    pub impact_score: f64,
    /// Statistical confidence (smoothed).
    pub confidence: f64,
    /// Number of distinct learners.
    pub learner_count: i64,
    /// Median per-learner impact.
    pub median_impact: f64,
    /// Rank within this skill scope (1 = best).
    pub rank: i64,
}

/// Result of a full reputation recomputation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecomputeResult {
    /// Number of assertions updated.
    pub assertions_updated: i64,
    /// Number of impact deltas recomputed.
    pub deltas_recomputed: i64,
    /// Total time taken (milliseconds).
    pub duration_ms: i64,
}

/// Result of reputation verification — checks if a reputation claim
/// can be independently reproduced from the evidence chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the claimed score matches recomputed score.
    pub score_matches: bool,
    /// Whether the claimed confidence matches recomputed confidence.
    pub confidence_matches: bool,
    /// Recomputed score from evidence.
    pub recomputed_score: f64,
    /// Recomputed confidence from evidence.
    pub recomputed_confidence: f64,
    /// Claimed score.
    pub claimed_score: f64,
    /// Claimed confidence.
    pub claimed_confidence: f64,
    /// Maximum absolute difference (tolerance = 0.001).
    pub max_diff: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reputation_role_roundtrip() {
        let all = [
            (ReputationRole::Instructor, "instructor"),
            (ReputationRole::Learner, "learner"),
            (ReputationRole::Assessor, "assessor"),
            (ReputationRole::Author, "author"),
            (ReputationRole::Mentor, "mentor"),
        ];
        for (variant, expected_str) in all {
            assert_eq!(variant.as_str(), expected_str);
            let parsed = ReputationRole::from_str(expected_str).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn reputation_role_from_str_invalid() {
        assert!(ReputationRole::from_str("").is_none());
        assert!(ReputationRole::from_str("Instructor").is_none());
        assert!(ReputationRole::from_str("student").is_none());
    }

    #[test]
    fn reputation_role_serde_roundtrip() {
        for variant in [
            ReputationRole::Instructor,
            ReputationRole::Learner,
            ReputationRole::Assessor,
            ReputationRole::Author,
            ReputationRole::Mentor,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: ReputationRole = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn snapshot_status_roundtrip() {
        let all = [
            (SnapshotStatus::Pending, "pending"),
            (SnapshotStatus::Building, "building"),
            (SnapshotStatus::Submitted, "submitted"),
            (SnapshotStatus::Confirmed, "confirmed"),
            (SnapshotStatus::Failed, "failed"),
        ];
        for (variant, expected_str) in all {
            assert_eq!(variant.as_str(), expected_str);
            let parsed = SnapshotStatus::from_str(expected_str).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn snapshot_status_from_str_invalid() {
        assert!(SnapshotStatus::from_str("").is_none());
        assert!(SnapshotStatus::from_str("PENDING").is_none());
        assert!(SnapshotStatus::from_str("completed").is_none());
    }

    #[test]
    fn distribution_metrics_default() {
        let metrics = DistributionMetrics::default();
        assert_eq!(metrics.median_impact, 0.0);
        assert_eq!(metrics.impact_p25, 0.0);
        assert_eq!(metrics.impact_p75, 0.0);
        assert_eq!(metrics.learner_count, 0);
        assert_eq!(metrics.impact_variance, 0.0);
    }

    #[test]
    fn reputation_query_default() {
        let query = ReputationQuery::default();
        assert!(query.actor_address.is_none());
        assert!(query.role.is_none());
        assert!(query.skill_id.is_none());
        assert!(query.proficiency_level.is_none());
        assert!(query.limit.is_none());
    }

    #[test]
    fn cip68_constants() {
        assert_eq!(cip68::REFERENCE_LABEL_PREFIX, [0x00, 0x06, 0x43, 0xb0]);
        assert_eq!(cip68::USER_LABEL_PREFIX, [0x00, 0x0d, 0xe1, 0x40]);
        assert_eq!(cip68::ROLE_INSTRUCTOR, 0x01);
        assert_eq!(cip68::ROLE_LEARNER, 0x02);
        assert_eq!(cip68::ROLE_ASSESSOR, 0x03);
        assert_eq!(cip68::ROLE_AUTHOR, 0x04);
        assert_eq!(cip68::ROLE_MENTOR, 0x05);
        assert_eq!(cip68::IMPACT_SCALE, 1_000_000);
        assert_eq!(cip68::CONFIDENCE_SCALE, 10_000);
    }

    #[test]
    fn full_reputation_assertion_serde_roundtrip() {
        let assertion = FullReputationAssertion {
            id: "ra1".into(),
            actor_address: "stake_test1u123".into(),
            role: "instructor".into(),
            skill_id: Some("sk1".into()),
            proficiency_level: Some("apply".into()),
            score: 0.85,
            confidence: 0.625,
            evidence_count: 5,
            distribution: Some(DistributionMetrics {
                median_impact: 0.1,
                impact_p25: 0.05,
                impact_p75: 0.15,
                learner_count: 3,
                impact_variance: 0.002,
            }),
            computation_spec: "v2".into(),
            window_start: None,
            window_end: None,
            updated_at: "2025-01-01".into(),
        };
        let json = serde_json::to_string(&assertion).unwrap();
        let parsed: FullReputationAssertion = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.score, 0.85);
        assert!(parsed.distribution.is_some());
        assert_eq!(parsed.distribution.unwrap().learner_count, 3);
    }
}

/// CIP-68 label prefixes for soulbound reputation tokens.
pub mod cip68 {
    /// Reference NFT label (100) — 4-byte prefix.
    pub const REFERENCE_LABEL_PREFIX: [u8; 4] = [0x00, 0x06, 0x43, 0xb0];
    /// User token label (222) — 4-byte prefix.
    pub const USER_LABEL_PREFIX: [u8; 4] = [0x00, 0x0d, 0xe1, 0x40];

    /// Role byte encoding for asset name.
    pub const ROLE_INSTRUCTOR: u8 = 0x01;
    pub const ROLE_LEARNER: u8 = 0x02;
    pub const ROLE_ASSESSOR: u8 = 0x03;
    pub const ROLE_AUTHOR: u8 = 0x04;
    pub const ROLE_MENTOR: u8 = 0x05;

    /// Scale factor for on-chain impact scores (10^6).
    pub const IMPACT_SCALE: i64 = 1_000_000;
    /// Scale factor for on-chain confidence values (10^4).
    pub const CONFIDENCE_SCALE: i64 = 10_000;
}

/// Snapshot status for on-chain reputation anchoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    Pending,
    Building,
    Submitted,
    Confirmed,
    Failed,
}

impl SnapshotStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SnapshotStatus::Pending => "pending",
            SnapshotStatus::Building => "building",
            SnapshotStatus::Submitted => "submitted",
            SnapshotStatus::Confirmed => "confirmed",
            SnapshotStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<SnapshotStatus> {
        match s {
            "pending" => Some(SnapshotStatus::Pending),
            "building" => Some(SnapshotStatus::Building),
            "submitted" => Some(SnapshotStatus::Submitted),
            "confirmed" => Some(SnapshotStatus::Confirmed),
            "failed" => Some(SnapshotStatus::Failed),
            _ => None,
        }
    }
}

/// A reputation snapshot record — tracks on-chain anchoring of reputation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub id: String,
    pub actor_address: String,
    pub subject_id: String,
    pub role: String,
    pub skill_count: i64,
    pub tx_status: String,
    pub tx_hash: Option<String>,
    pub policy_id: Option<String>,
    pub ref_asset_name: Option<String>,
    pub user_asset_name: Option<String>,
    pub error_message: Option<String>,
    pub snapshot_at: String,
    pub confirmed_at: Option<String>,
}

/// On-chain skill score (part of ReputationDatum).
///
/// Stored as Plutus Data integers with fixed-point scaling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnChainSkillScore {
    /// First 16 bytes of skill_id (hex-encoded for serialization).
    pub skill_id_bytes: String,
    /// Bloom's proficiency level as enum index (0-5).
    pub proficiency: u8,
    /// Impact score scaled by 10^6.
    pub impact_score: i64,
    /// Confidence scaled by 10^4.
    pub confidence: i64,
    /// Number of evidence records.
    pub evidence_count: i64,
}

/// Parameters for creating a reputation snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSnapshotParams {
    /// Subject ID to snapshot reputation for.
    pub subject_id: String,
    /// Role to snapshot (instructor/learner).
    pub role: String,
}
