//! Answer custody on a credential, and the evidence entry naming the hosted
//! attempt a role credential was issued from.
//!
//! An integrity assertion states who held the answers
//! ([`AnswerCustody`]): the candidate's `device`, or the `issuer` when the
//! answers never left it. Only an issuer other than the candidate can hold
//! answers away from the candidate, so a self-issued credential stating
//! `issuer` is invalid.
//!
//! When an organisation's hosted service draws, delivers, times and grades an
//! attempt itself, there is no candidate credential to name, so the role
//! credential it signs carries one [`HostedAttemptEvidence`] entry instead of
//! an [`AssessedCredentialEvidence`](crate::assessed::AssessedCredentialEvidence):
//! the attempt's identifier, that the issuer held the answers and graded, the
//! probe outcome, and the attestation outcome and verified assurance level
//! the issuer found. The issuer's signature is what vouches for it. The
//! entry is valid only on an issuer-signed credential whose assertion states
//! `issuer` custody.
//!
//! [`custody_defects`] finds what is wrong with a credential's custody from
//! its own content, with no trust inputs; verification rejects a credential
//! with any ([`crate::vc::VerificationResult::custody_defects`]). The
//! issuance policy rule judges a role credential from a hosted attempt with
//! [`crate::policy::evaluate_hosted`].

use serde::{Deserialize, Serialize};

pub use crate::integrity::AnswerCustody;

use crate::attestation::{AttestationOutcome, VerifiedAssuranceLevel};
use crate::vc::{EvidenceEntry, ProbeSummary, VerifiableCredential};

/// The `type` of a [`HostedAttemptEvidence`] entry.
pub const HOSTED_ATTEMPT_EVIDENCE_TYPE: &str = "HostedAttemptEvidence";

/// Who computed an attempt's score.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grader {
    /// The issuer graded, ignoring any score the device sent.
    Issuer,
    /// The candidate's device graded and reported its score.
    Device,
}

/// What an issuer established about a hosted attempt. On the wire, one
/// member of the credential's `evidence`:
///
/// ```json
/// {
///   "type": "HostedAttemptEvidence",
///   "attemptId": "…",
///   "answerCustody": "issuer",
///   "gradedBy": "issuer",
///   "probe": { "outcome": "satisfied", "policies": ["authored"], "satisfied": 1, "failed": 0, "skipped": 0 },
///   "attestation": { "outcome": "verified_credential_bound", "platform": "ios" },
///   "verifiedAssuranceLevel": "device_attested"
/// }
/// ```
///
/// `probe` is omitted when no probe step ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedAttemptEvidence {
    /// Always [`HOSTED_ATTEMPT_EVIDENCE_TYPE`].
    #[serde(rename = "type")]
    pub type_: String,
    /// The issuer's identifier for the attempt.
    pub attempt_id: String,
    /// Always `issuer`: the answers never left the issuer.
    pub answer_custody: AnswerCustody,
    /// Always `issuer`: the issuer computed the score.
    pub graded_by: Grader,
    /// The probe step the issuer ran, timed and graded; `None` when none ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<ProbeSummary>,
    /// What the issuer's verification established about the platform
    /// evidence the candidate's device sent for the attempt.
    pub attestation: AttestationOutcome,
    /// The verified assurance level that outcome establishes.
    pub verified_assurance_level: VerifiedAssuranceLevel,
}

/// Why a credential's [`HostedAttemptEvidence`] cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostedAttemptEvidenceError {
    #[error("more than one HostedAttemptEvidence entry")]
    Duplicate,
    #[error("the HostedAttemptEvidence entry does not parse: {0}")]
    Malformed(String),
}

impl HostedAttemptEvidence {
    /// The entry for hosted attempt `attempt_id`: issuer custody, graded by
    /// the issuer, with the issuer's `probe` outcome and the `attestation`
    /// outcome it established and the level that outcome verifies.
    pub fn new(
        attempt_id: impl Into<String>,
        probe: Option<ProbeSummary>,
        attestation: AttestationOutcome,
    ) -> Self {
        Self {
            type_: HOSTED_ATTEMPT_EVIDENCE_TYPE.into(),
            attempt_id: attempt_id.into(),
            answer_custody: AnswerCustody::Issuer,
            graded_by: Grader::Issuer,
            probe,
            verified_assurance_level: attestation.verified_assurance_level(),
            attestation,
        }
    }

    /// Whether the entry says what a hosted attempt is: the issuer held the
    /// answers and graded.
    pub fn states_a_hosted_attempt(&self) -> bool {
        self.answer_custody == AnswerCustody::Issuer && self.graded_by == Grader::Issuer
    }

    /// The level a policy judges: the entry's stated level when its own
    /// outcome verifies it, else `local` (the rule
    /// [`AssessedCredentialEvidence::assurance_level`](crate::assessed::AssessedCredentialEvidence::assurance_level)
    /// applies).
    pub fn assurance_level(&self) -> VerifiedAssuranceLevel {
        if self.verified_assurance_level == self.attestation.verified_assurance_level() {
            self.verified_assurance_level
        } else {
            VerifiedAssuranceLevel::Local
        }
    }

    /// This entry as a member of a credential's `evidence`.
    pub fn into_entry(self) -> EvidenceEntry {
        match serde_json::to_value(self).expect("hosted attempt evidence serializes") {
            serde_json::Value::Object(map) => EvidenceEntry(map),
            _ => unreachable!("hosted attempt evidence serializes to an object"),
        }
    }

    /// The credential's entry: `Ok(None)` when it has none.
    pub fn find(
        credential: &VerifiableCredential,
    ) -> Result<Option<Self>, HostedAttemptEvidenceError> {
        let mut entries = credential
            .evidence
            .iter()
            .filter(|e| e.has_type(HOSTED_ATTEMPT_EVIDENCE_TYPE));
        let Some(entry) = entries.next() else {
            return Ok(None);
        };
        if entries.next().is_some() {
            return Err(HostedAttemptEvidenceError::Duplicate);
        }
        serde_json::from_value(serde_json::Value::Object(entry.0.clone()))
            .map(Some)
            .map_err(|e| HostedAttemptEvidenceError::Malformed(e.to_string()))
    }
}

/// What is wrong with what a credential says about who held the answers.
/// Each one rejects the credential.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyDefect {
    /// A self-issued credential states `issuer` answer custody.
    IssuerCustodySelfIssued,
    /// A self-issued credential carries a hosted attempt entry.
    HostedAttemptSelfIssued,
    /// The hosted attempt entry is duplicated, does not parse, or does not
    /// say that the issuer held the answers and graded.
    HostedAttemptInvalid,
    /// A hosted attempt entry beside an assertion that does not state
    /// `issuer` custody, or beside no assertion at all.
    HostedAttemptCustodyMismatch,
}

impl CustodyDefect {
    /// Every defect, in the order [`custody_defects`] reports them.
    pub const ALL: [Self; 4] = [
        Self::IssuerCustodySelfIssued,
        Self::HostedAttemptSelfIssued,
        Self::HostedAttemptInvalid,
        Self::HostedAttemptCustodyMismatch,
    ];

    /// The stable snake_case code.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IssuerCustodySelfIssued => "issuer_custody_self_issued",
            Self::HostedAttemptSelfIssued => "hosted_attempt_self_issued",
            Self::HostedAttemptInvalid => "hosted_attempt_invalid",
            Self::HostedAttemptCustodyMismatch => "hosted_attempt_custody_mismatch",
        }
    }
}

/// Every [`CustodyDefect`] of `credential`, in [`CustodyDefect::ALL`] order;
/// empty when its custody is consistent. Judged on the credential's content
/// alone: a credential is self-issued when its issuer is its subject.
pub fn custody_defects(credential: &VerifiableCredential) -> Vec<CustodyDefect> {
    let self_issued = credential.issuer == credential.credential_subject.id;
    let custody = credential.integrity.as_ref().map(|a| a.answer_custody());
    let has_entry = credential
        .evidence
        .iter()
        .any(|e| e.has_type(HOSTED_ATTEMPT_EVIDENCE_TYPE));
    let entry_valid = matches!(
        HostedAttemptEvidence::find(credential),
        Ok(Some(entry)) if entry.states_a_hosted_attempt()
    );
    [
        (
            self_issued && custody == Some(AnswerCustody::Issuer),
            CustodyDefect::IssuerCustodySelfIssued,
        ),
        (
            self_issued && has_entry,
            CustodyDefect::HostedAttemptSelfIssued,
        ),
        (
            has_entry && !entry_valid,
            CustodyDefect::HostedAttemptInvalid,
        ),
        (
            has_entry && custody != Some(AnswerCustody::Issuer),
            CustodyDefect::HostedAttemptCustodyMismatch,
        ),
    ]
    .into_iter()
    .filter_map(|(found, defect)| found.then_some(defect))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::Platform;

    /// The wire tokens are the stable codes.
    #[test]
    fn defects_serialize_as_their_codes() {
        for defect in CustodyDefect::ALL {
            assert_eq!(
                serde_json::to_value(defect).unwrap(),
                serde_json::Value::String(defect.as_str().into())
            );
        }
    }

    /// A companion's credential-bound outcome verifies `companion_attested`
    /// and nothing higher, as on an assessed credential entry.
    #[test]
    fn a_level_its_outcome_does_not_verify_is_local() {
        let mut entry = HostedAttemptEvidence::new(
            "hatt",
            None,
            AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios,
                companion: true,
            },
        );
        assert_eq!(
            entry.assurance_level(),
            VerifiedAssuranceLevel::CompanionAttested
        );
        entry.verified_assurance_level = VerifiedAssuranceLevel::DeviceAttested;
        assert_eq!(entry.assurance_level(), VerifiedAssuranceLevel::Local);
    }
}
