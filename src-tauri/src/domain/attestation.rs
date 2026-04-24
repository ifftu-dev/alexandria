//! Completion-attestation domain types (post-VC-first rebuild).
//!
//! Legacy evidence cosigning has been retired along with the
//! `evidence_records` + `evidence_attestations` tables. The new model
//! is simpler and VC-first:
//!
//! 1. A DAO declares **attestation requirements** on a specific
//!    *course* (rather than a skill/level pair): "before auto-issuing
//!    a VC for this course, require at least N assessor signatures on
//!    the learner's completion-witness tx."
//! 2. Assessors produce a **completion attestation** row: an Ed25519
//!    signature of the witness tx hash + their verification key.
//! 3. The auto-issuance pipeline checks the assessor count for a
//!    given witness tx before emitting a VC. If short, the observation
//!    stays pending and the UI nudges assessors.
//!
//! The implementation lives in `commands::attestation`; this module
//! defines serializable shapes for the IPC surface.

use serde::{Deserialize, Serialize};

/// A DAO-set attestation requirement keyed on a course id. When a
/// requirement is present, the observer will not auto-issue a VC for
/// that course until the required number of attestations have been
/// recorded against the learner's witness tx hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionAttestationRequirement {
    pub course_id: String,
    pub required_attestors: i64,
    pub dao_id: String,
    pub set_by_proposal: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A single attestor's signature over a completion-witness tx hash.
/// Stored once per (witness_tx_hash, attestor_did); duplicates are
/// ignored. Signatures are over the raw bytes of `witness_tx_hash`
/// hex-decoded to 32 bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionAttestation {
    pub id: String,
    pub witness_tx_hash: String,
    pub attestor_did: String,
    pub attestor_pubkey: String,
    pub signature: String,
    pub note: Option<String>,
    pub created_at: String,
}

/// Summary view for the UI — how many attestations does a given
/// witness tx hold, and is it gated?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionAttestationStatus {
    pub witness_tx_hash: String,
    pub course_id: Option<String>,
    pub required_attestors: i64,
    pub current_attestors: i64,
    pub is_satisfied: bool,
    pub attestations: Vec<CompletionAttestation>,
}

/// IPC: set an attestation requirement on a course.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetCompletionRequirementParams {
    pub course_id: String,
    pub required_attestors: i64,
    pub dao_id: String,
    pub set_by_proposal: Option<String>,
}

/// IPC: submit an attestation on a witness tx. The caller's signing
/// key is used to sign the 32-byte tx hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitCompletionAttestationParams {
    pub witness_tx_hash: String,
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_round_trip() {
        let req = CompletionAttestationRequirement {
            course_id: "course_abc".into(),
            required_attestors: 2,
            dao_id: "dao_cs".into(),
            set_by_proposal: Some("prop_1".into()),
            created_at: "2026-04-24T00:00:00Z".into(),
            updated_at: "2026-04-24T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: CompletionAttestationRequirement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.course_id, "course_abc");
        assert_eq!(back.required_attestors, 2);
    }

    #[test]
    fn status_round_trip() {
        let s = CompletionAttestationStatus {
            witness_tx_hash: "aa".repeat(32),
            course_id: Some("course_abc".into()),
            required_attestors: 2,
            current_attestors: 1,
            is_satisfied: false,
            attestations: vec![CompletionAttestation {
                id: "att_1".into(),
                witness_tx_hash: "aa".repeat(32),
                attestor_did: "did:key:z".into(),
                attestor_pubkey: "11".repeat(32),
                signature: "22".repeat(64),
                note: None,
                created_at: "2026-04-24T00:00:00Z".into(),
            }],
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: CompletionAttestationStatus = serde_json::from_str(&json).unwrap();
        assert!(!back.is_satisfied);
        assert_eq!(back.attestations.len(), 1);
    }
}
