//! Credential trust classification.
//!
//! A valid signature proves who signed a credential. It does not show that the
//! signer is approved for any privilege. This module turns the verifier's
//! result, the signed credential, and optional exact course-completion
//! endorsement evidence into a typed trust state with stable reason codes.
//!
//! Policy qualification — an approved issuer for a specific action and scope —
//! is a separate step layered on these states. Nothing here grants privilege,
//! and no state is derived from a database flag or a caller-supplied label.

use serde::{Deserialize, Serialize};

use crate::course::{
    evaluate_completion_endorsements, CourseCompletionBinding, CourseCompletionEndorsement,
    CourseCompletionPolicy,
};
use crate::did::Did;
use crate::vc::verify::type_allowed;
use crate::vc::{
    AcceptanceDecision, SkillClaim, VerifiableCredential, VerificationPendingReason,
    VerificationPolicy, VerificationResult,
};

/// Version of the classification rules. Cached trust states must be keyed by
/// this value so a rule change cannot leave a stale classification in place.
pub const TRUST_CALCULATION_VERSION: u32 = 1;

const COURSE_DOCUMENT_EVIDENCE_PREFIX: &str = "course-document:blake3:";
const COMPLETION_ROOT_EVIDENCE_PREFIX: &str = "completion-root:";

/// Signed evidence reference naming the exact course document a completion
/// self-claim was earned against.
pub fn course_document_evidence_ref(
    course_document_cid: &str,
    course_document_version: u32,
) -> String {
    format!("{COURSE_DOCUMENT_EVIDENCE_PREFIX}{course_document_cid}:v{course_document_version}")
}

/// Signed evidence reference naming the completion root of a self-claim.
pub fn completion_root_evidence_ref(completion_root: &str) -> String {
    format!("{COMPLETION_ROOT_EVIDENCE_PREFIX}{completion_root}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CredentialTrust {
    /// The credential, or the verifier result supplied for it, is not acceptable.
    Invalid { reasons: Vec<TrustInvalidReason> },
    /// Verification needs evidence that is missing or could not be read.
    Pending {
        reasons: Vec<VerificationPendingReason>,
    },
    /// An authentic claim the subject made about themselves.
    VerifiedSelfClaim { endorsement: EndorsementOutcome },
    /// An authentic credential signed by an issuer other than the subject.
    /// Issuer inequality alone is neither independence nor policy approval.
    VerifiedIssuerSigned { issuer: Did },
    /// An authentic self-claim for an exact course document whose signed
    /// completion policy threshold is met by distinct authorized attestors.
    VerifiedCourseEndorsement {
        course_id: String,
        course_document_cid: String,
        course_document_version: u32,
        attestors: Vec<Did>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustInvalidReason {
    /// The verifier result does not describe this credential or contradicts
    /// its own checks.
    InconsistentVerificationResult,
    IssuerUnresolved,
    InvalidSignature,
    SubjectNotBound,
    InvalidStatusReference,
    Revoked,
    Expired,
    Suspended,
    Superseded,
    IntegrityAnchorMissing,
    TypeNotAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum EndorsementOutcome {
    /// No completion policy evidence was supplied for this credential.
    NotSupplied,
    /// The supplied completion evidence does not describe this credential.
    NotApplicable { reason: EndorsementMismatch },
    /// The supplied completion policy, binding, or endorsements are malformed.
    InvalidEvidence,
    /// Distinct valid authorized endorsements are below the policy threshold.
    ThresholdUnmet {
        required_attestors: u16,
        valid_attestors: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndorsementMismatch {
    WrongNetwork,
    SubjectMismatch,
    NotSkillClaim,
    CourseDocumentNotClaimed,
    CompletionRootNotClaimed,
}

/// Exact completion policy, binding, and stored endorsements for one claim.
#[derive(Debug, Clone, Copy)]
pub struct CourseEndorsementEvidence<'a> {
    pub policy: &'a CourseCompletionPolicy,
    pub binding: &'a CourseCompletionBinding,
    pub endorsements: &'a [CourseCompletionEndorsement],
}

/// Classify a credential from its verifier result.
///
/// The verifier result is rechecked against the credential and policy rather
/// than trusted: a result for another credential, or an `accept` that
/// contradicts its own flags, is invalid.
pub fn classify_credential(
    credential: &VerifiableCredential,
    verification: &VerificationResult,
    policy: &VerificationPolicy,
    expected_network_id: &str,
    endorsement: Option<CourseEndorsementEvidence<'_>>,
) -> CredentialTrust {
    let reasons = invalid_reasons(credential, verification, policy);
    if !reasons.is_empty() {
        return CredentialTrust::Invalid { reasons };
    }
    match verification.acceptance_decision {
        AcceptanceDecision::Pending if !verification.pending_reasons.is_empty() => {
            CredentialTrust::Pending {
                reasons: verification.pending_reasons.clone(),
            }
        }
        AcceptanceDecision::Accept
            if verification.pending_reasons.is_empty()
                && verification.issuer_resolved
                && verification.valid_signature =>
        {
            classify_accepted(credential, expected_network_id, endorsement)
        }
        _ => inconsistent(),
    }
}

fn inconsistent() -> CredentialTrust {
    CredentialTrust::Invalid {
        reasons: vec![TrustInvalidReason::InconsistentVerificationResult],
    }
}

fn invalid_reasons(
    credential: &VerifiableCredential,
    verification: &VerificationResult,
    policy: &VerificationPolicy,
) -> Vec<TrustInvalidReason> {
    if verification.credential_id != credential.id.as_deref().unwrap_or_default() {
        return vec![TrustInvalidReason::InconsistentVerificationResult];
    }
    let issuer_evidence_pending = verification.pending_reasons.iter().any(|reason| {
        matches!(
            reason,
            VerificationPendingReason::IssuerKeyMissing
                | VerificationPendingReason::IssuerKeyUnavailable
        )
    });
    let mut reasons = Vec::new();
    if !verification.issuer_resolved {
        if !issuer_evidence_pending {
            // Nothing was verified; a signature failure would be misleading.
            return vec![TrustInvalidReason::IssuerUnresolved];
        }
    } else if !verification.valid_signature {
        reasons.push(TrustInvalidReason::InvalidSignature);
    }
    let checks = [
        (
            !verification.subject_bound,
            TrustInvalidReason::SubjectNotBound,
        ),
        (
            !verification.status_valid,
            TrustInvalidReason::InvalidStatusReference,
        ),
        (verification.revoked, TrustInvalidReason::Revoked),
        (
            policy.reject_expired && verification.expired,
            TrustInvalidReason::Expired,
        ),
        (
            policy.reject_suspended && verification.suspended,
            TrustInvalidReason::Suspended,
        ),
        (
            policy.reject_superseded && verification.superseded,
            TrustInvalidReason::Superseded,
        ),
        (
            policy.require_integrity_anchor && !verification.integrity_anchored,
            TrustInvalidReason::IntegrityAnchorMissing,
        ),
        (
            !type_allowed(credential, policy),
            TrustInvalidReason::TypeNotAllowed,
        ),
    ];
    reasons.extend(
        checks
            .into_iter()
            .filter_map(|(failed, reason)| failed.then_some(reason)),
    );
    reasons
}

fn classify_accepted(
    credential: &VerifiableCredential,
    expected_network_id: &str,
    endorsement: Option<CourseEndorsementEvidence<'_>>,
) -> CredentialTrust {
    if credential.issuer != credential.credential_subject.id {
        return CredentialTrust::VerifiedIssuerSigned {
            issuer: credential.issuer.clone(),
        };
    }
    let Some(evidence) = endorsement else {
        return CredentialTrust::VerifiedSelfClaim {
            endorsement: EndorsementOutcome::NotSupplied,
        };
    };
    match endorsed_attestors(credential, expected_network_id, evidence) {
        Ok(attestors) => CredentialTrust::VerifiedCourseEndorsement {
            course_id: evidence.binding.course_id.clone(),
            course_document_cid: evidence.binding.course_document_cid.clone(),
            course_document_version: evidence.binding.course_document_version,
            attestors,
        },
        Err(endorsement) => CredentialTrust::VerifiedSelfClaim { endorsement },
    }
}

fn endorsed_attestors(
    credential: &VerifiableCredential,
    expected_network_id: &str,
    evidence: CourseEndorsementEvidence<'_>,
) -> Result<Vec<Did>, EndorsementOutcome> {
    let binding = evidence.binding;
    binding
        .validate(evidence.policy)
        .map_err(|_| EndorsementOutcome::InvalidEvidence)?;
    let not_applicable = |reason| Err(EndorsementOutcome::NotApplicable { reason });
    if binding.network_id != expected_network_id {
        return not_applicable(EndorsementMismatch::WrongNetwork);
    }
    if binding.subject_did != credential.credential_subject.id {
        return not_applicable(EndorsementMismatch::SubjectMismatch);
    }
    let Some(claim) = SkillClaim::extract(&credential.credential_subject) else {
        return not_applicable(EndorsementMismatch::NotSkillClaim);
    };
    let claims = |expected: String| claim.evidence_refs.contains(&expected);
    if !claims(course_document_evidence_ref(
        &binding.course_document_cid,
        binding.course_document_version,
    )) {
        return not_applicable(EndorsementMismatch::CourseDocumentNotClaimed);
    }
    if !claims(completion_root_evidence_ref(&binding.completion_root)) {
        return not_applicable(EndorsementMismatch::CompletionRootNotClaimed);
    }
    let threshold =
        evaluate_completion_endorsements(evidence.policy, binding, evidence.endorsements)
            .map_err(|_| EndorsementOutcome::InvalidEvidence)?;
    if !threshold.satisfied {
        return Err(EndorsementOutcome::ThresholdUnmet {
            required_attestors: threshold.required_attestors,
            valid_attestors: threshold.valid_attestors.len(),
        });
    }
    Ok(threshold.valid_attestors)
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::course::{
        sign_completion_endorsement, AuthorizedAttestor, CompletionEvidence, EvidenceRequirement,
        COMPLETION_ENDORSEMENT_FORMAT_VERSION, COMPLETION_POLICY_FORMAT_VERSION,
    };
    use crate::did::{derive_did_key, VerificationMethodRef};
    use crate::vc::sign::{sign_credential, UnsignedCredential};
    use crate::vc::verify::verify_credential;
    use crate::vc::{Claim, CredentialStatus, Proof};
    use crate::NullStore;

    const NOW: &str = "2026-09-15T00:00:00Z";
    const NETWORK: &str = "preprod";
    const DOCUMENT_CID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const COMPLETION_ROOT: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn credential(
        issuer: &SigningKey,
        subject: &Did,
        evidence_refs: Vec<String>,
    ) -> VerifiableCredential {
        let issuer_did = derive_did_key(issuer);
        let claim = Claim::Skill(SkillClaim {
            skill_id: "skill".into(),
            level: 3,
            score: 0.8,
            evidence_refs,
            rubric_version: None,
            assessment_method: Some("course_completion".into()),
            provenance: None,
        });
        let unsigned = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some("urn:uuid:trust-test".into()),
            type_: vec!["VerifiableCredential".into(), "SelfAssertion".into()],
            issuer: issuer_did.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: None,
            credential_subject: claim.into_subject(subject.clone()),
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: "2026-01-01T00:00:00Z".into(),
                verification_method: VerificationMethodRef(format!(
                    "{}#key-1",
                    issuer_did.as_str()
                )),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(),
            },
        };
        sign_credential(
            UnsignedCredential {
                credential: unsigned,
            },
            issuer,
            &issuer_did,
        )
        .expect("sign credential")
    }

    fn completion_refs() -> Vec<String> {
        vec![
            course_document_evidence_ref(DOCUMENT_CID, 2),
            completion_root_evidence_ref(COMPLETION_ROOT),
        ]
    }

    fn policy(attestors: &[&SigningKey], required: u16) -> CourseCompletionPolicy {
        let mut authorized_attestors = attestors
            .iter()
            .map(|attestor| AuthorizedAttestor {
                did: derive_did_key(attestor),
                public_key_hex: hex::encode(attestor.verifying_key().as_bytes()),
            })
            .collect::<Vec<_>>();
        authorized_attestors.sort_by(|left, right| left.did.as_str().cmp(right.did.as_str()));
        CourseCompletionPolicy {
            format_version: COMPLETION_POLICY_FORMAT_VERSION,
            required_attestors: required,
            authorized_attestors,
            evidence_requirements: vec![EvidenceRequirement {
                kind: "graded-submissions".into(),
                format_version: 1,
            }],
        }
    }

    fn binding(subject: &Did) -> CourseCompletionBinding {
        CourseCompletionBinding {
            format_version: COMPLETION_ENDORSEMENT_FORMAT_VERSION,
            network_id: NETWORK.into(),
            subject_did: subject.clone(),
            course_id: "course-1".into(),
            course_document_cid: DOCUMENT_CID.into(),
            course_document_version: 2,
            completion_root: COMPLETION_ROOT.into(),
            evidence: vec![CompletionEvidence {
                kind: "graded-submissions".into(),
                format_version: 1,
                id: "attempt-1".into(),
                digest: "33".repeat(32),
            }],
            witness_tx_hash: None,
        }
    }

    fn classify(
        credential: &VerifiableCredential,
        evidence: Option<CourseEndorsementEvidence<'_>>,
    ) -> CredentialTrust {
        let policy = VerificationPolicy::default();
        let result = verify_credential(&NullStore, credential, NOW, &policy);
        classify_credential(credential, &result, &policy, NETWORK, evidence)
    }

    #[test]
    fn authentic_self_claim_without_completion_evidence_is_only_a_self_claim() {
        let learner = key(1);
        let vc = credential(&learner, &derive_did_key(&learner), completion_refs());
        assert_eq!(
            classify(&vc, None),
            CredentialTrust::VerifiedSelfClaim {
                endorsement: EndorsementOutcome::NotSupplied
            }
        );
    }

    #[test]
    fn satisfied_exact_endorsement_classifies_as_course_endorsement() {
        let learner = key(1);
        let subject = derive_did_key(&learner);
        let (first, second) = (key(7), key(8));
        let policy = policy(&[&first, &second], 2);
        let binding = binding(&subject);
        let endorsements = [
            sign_completion_endorsement(&policy, binding.clone(), &first).unwrap(),
            sign_completion_endorsement(&policy, binding.clone(), &second).unwrap(),
        ];
        let vc = credential(&learner, &subject, completion_refs());
        let mut attestors = vec![derive_did_key(&first), derive_did_key(&second)];
        attestors.sort_by(|left, right| left.as_str().cmp(right.as_str()));

        assert_eq!(
            classify(
                &vc,
                Some(CourseEndorsementEvidence {
                    policy: &policy,
                    binding: &binding,
                    endorsements: &endorsements,
                })
            ),
            CredentialTrust::VerifiedCourseEndorsement {
                course_id: "course-1".into(),
                course_document_cid: DOCUMENT_CID.into(),
                course_document_version: 2,
                attestors,
            }
        );
    }

    #[test]
    fn repeated_or_unlisted_endorsements_leave_the_threshold_unmet() {
        let learner = key(1);
        let subject = derive_did_key(&learner);
        let (listed, other, outsider) = (key(7), key(8), key(9));
        let policy = policy(&[&listed, &other], 2);
        let binding = binding(&subject);
        let genuine = sign_completion_endorsement(&policy, binding.clone(), &listed).unwrap();
        let outsider_policy = self::policy(&[&outsider], 1);
        let unlisted =
            sign_completion_endorsement(&outsider_policy, binding.clone(), &outsider).unwrap();
        let endorsements = [genuine.clone(), genuine, unlisted];
        let vc = credential(&learner, &subject, completion_refs());

        assert_eq!(
            classify(
                &vc,
                Some(CourseEndorsementEvidence {
                    policy: &policy,
                    binding: &binding,
                    endorsements: &endorsements,
                })
            ),
            CredentialTrust::VerifiedSelfClaim {
                endorsement: EndorsementOutcome::ThresholdUnmet {
                    required_attestors: 2,
                    valid_attestors: 1,
                }
            }
        );
    }

    #[test]
    fn endorsement_for_another_document_subject_or_network_does_not_apply() {
        let learner = key(1);
        let subject = derive_did_key(&learner);
        let attestor = key(7);
        let policy = policy(&[&attestor], 1);
        let exact = binding(&subject);
        let endorsements =
            [sign_completion_endorsement(&policy, exact.clone(), &attestor).unwrap()];
        let evidence = |binding| CourseEndorsementEvidence {
            policy: &policy,
            binding,
            endorsements: &endorsements,
        };
        let outcome = |trust| match trust {
            CredentialTrust::VerifiedSelfClaim { endorsement } => endorsement,
            other => panic!("expected a self-claim, got {other:?}"),
        };

        let other_document = credential(
            &learner,
            &subject,
            vec![
                course_document_evidence_ref(DOCUMENT_CID, 3),
                completion_root_evidence_ref(COMPLETION_ROOT),
            ],
        );
        assert_eq!(
            outcome(classify(&other_document, Some(evidence(&exact)))),
            EndorsementOutcome::NotApplicable {
                reason: EndorsementMismatch::CourseDocumentNotClaimed
            }
        );

        let other_root = credential(
            &learner,
            &subject,
            vec![course_document_evidence_ref(DOCUMENT_CID, 2)],
        );
        assert_eq!(
            outcome(classify(&other_root, Some(evidence(&exact)))),
            EndorsementOutcome::NotApplicable {
                reason: EndorsementMismatch::CompletionRootNotClaimed
            }
        );

        let vc = credential(&learner, &subject, completion_refs());
        let mut wrong_network = exact.clone();
        wrong_network.network_id = "mainnet".into();
        assert_eq!(
            outcome(classify(&vc, Some(evidence(&wrong_network)))),
            EndorsementOutcome::NotApplicable {
                reason: EndorsementMismatch::WrongNetwork
            }
        );

        let other_subject = binding(&derive_did_key(&key(2)));
        assert_eq!(
            outcome(classify(&vc, Some(evidence(&other_subject)))),
            EndorsementOutcome::NotApplicable {
                reason: EndorsementMismatch::SubjectMismatch
            }
        );
    }

    #[test]
    fn third_party_issuer_is_issuer_signed_even_with_completion_evidence() {
        let learner = derive_did_key(&key(1));
        let issuer = key(5);
        let attestor = key(7);
        let policy = policy(&[&attestor], 1);
        let binding = binding(&learner);
        let endorsements =
            [sign_completion_endorsement(&policy, binding.clone(), &attestor).unwrap()];
        let vc = credential(&issuer, &learner, completion_refs());

        assert_eq!(
            classify(
                &vc,
                Some(CourseEndorsementEvidence {
                    policy: &policy,
                    binding: &binding,
                    endorsements: &endorsements,
                })
            ),
            CredentialTrust::VerifiedIssuerSigned {
                issuer: derive_did_key(&issuer)
            }
        );
    }

    #[test]
    fn tampered_credential_is_invalid() {
        let learner = key(1);
        let mut vc = credential(&learner, &derive_did_key(&learner), completion_refs());
        vc.credential_subject
            .properties
            .insert("level".into(), serde_json::json!(5));
        assert_eq!(
            classify(&vc, None),
            CredentialTrust::Invalid {
                reasons: vec![TrustInvalidReason::InvalidSignature]
            }
        );
    }

    #[test]
    fn supplied_results_that_contradict_the_credential_are_invalid() {
        let learner = key(1);
        let vc = credential(&learner, &derive_did_key(&learner), completion_refs());
        let policy = VerificationPolicy::default();
        let accepted = verify_credential(&NullStore, &vc, NOW, &policy);
        let inconsistent = CredentialTrust::Invalid {
            reasons: vec![TrustInvalidReason::InconsistentVerificationResult],
        };

        let mut other_credential = accepted.clone();
        other_credential.credential_id = "urn:uuid:another".into();
        assert_eq!(
            classify_credential(&vc, &other_credential, &policy, NETWORK, None),
            inconsistent
        );

        let mut rejected_without_reason = accepted.clone();
        rejected_without_reason.acceptance_decision = AcceptanceDecision::Reject;
        assert_eq!(
            classify_credential(&vc, &rejected_without_reason, &policy, NETWORK, None),
            inconsistent
        );

        let mut accepted_but_revoked = accepted;
        accepted_but_revoked.revoked = true;
        assert_eq!(
            classify_credential(&vc, &accepted_but_revoked, &policy, NETWORK, None),
            CredentialTrust::Invalid {
                reasons: vec![TrustInvalidReason::Revoked]
            }
        );
    }

    #[test]
    fn missing_status_evidence_is_pending_not_verified() {
        let learner = key(1);
        let subject = derive_did_key(&learner);
        let mut vc = credential(&learner, &subject, completion_refs());
        vc.credential_status = Some(CredentialStatus {
            id: "urn:status:1#0".into(),
            type_: "BitstringStatusListEntry".into(),
            status_purpose: "revocation".into(),
            status_list_index: "0".into(),
            status_list_credential: "urn:status:1".into(),
        });
        let issuer_did = derive_did_key(&learner);
        let vc = sign_credential(UnsignedCredential { credential: vc }, &learner, &issuer_did)
            .expect("re-sign with status");

        assert_eq!(
            classify(&vc, None),
            CredentialTrust::Pending {
                reasons: vec![VerificationPendingReason::StatusListMissing]
            }
        );
    }

    #[test]
    fn trust_state_wire_shape_is_tagged_snake_case() {
        let value = serde_json::to_value(CredentialTrust::VerifiedSelfClaim {
            endorsement: EndorsementOutcome::ThresholdUnmet {
                required_attestors: 2,
                valid_attestors: 1,
            },
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "state": "verified_self_claim",
                "endorsement": {
                    "outcome": "threshold_unmet",
                    "required_attestors": 2,
                    "valid_attestors": 1
                }
            })
        );
    }
}
