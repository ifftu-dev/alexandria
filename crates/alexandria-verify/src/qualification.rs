//! Subject qualification policies.
//!
//! A qualification policy is the authority that turns authentic provenance
//! into a privilege. It is an immutable, reviewed document identified by the
//! SHA-256 digest of its exact canonical bytes, and a network configuration
//! pins the digests it accepts. Discovering a policy, a course author's
//! signature, an identity-provider login, or a mutable local row never makes a
//! policy applicable.
//!
//! Evaluation classifies the credential itself (see [`crate::trust`]) so a
//! caller cannot pair one credential with another credential's trust state.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::did::{parse_did_key, Did};
use crate::hash::sha256;
use crate::trust::{
    classify_credential, CourseEndorsementEvidence, CredentialTrust, TRUST_CALCULATION_VERSION,
};
use crate::vc::{
    SkillClaim, VerifiableCredential, VerificationPendingReason, VerificationPolicy,
    VerificationResult,
};

pub const QUALIFICATION_POLICY_FORMAT_VERSION: u32 = 1;
/// Version of the evaluation rules; cached decisions must be keyed by it.
pub const QUALIFICATION_CALCULATION_VERSION: u32 = 1;
pub const MAX_QUALIFICATION_POLICY_BYTES: usize = 64 * 1024;
pub const MAX_POLICY_SUBJECT_FIELDS: usize = 256;
pub const MAX_ACCEPTED_ISSUERS: usize = 64;
pub const MAX_PROFICIENCY_LEVEL: u8 = 5;

const MAX_IDENTIFIER_BYTES: usize = 128;

/// Privileged actions a qualification policy can govern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationAction {
    OpinionPosting,
}

/// How an authentic credential may satisfy a policy. Declared in the same
/// order as the serialized names so "uniquely sorted" reads naturally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRoute {
    /// A self-claim for an exact course version endorsed by accepted issuers.
    AcceptedCourseEndorsement,
    /// A credential signed directly by an accepted issuer.
    AcceptedIssuer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectQualificationPolicy {
    pub format_version: u32,
    pub policy_id: String,
    pub network_id: String,
    pub action: QualificationAction,
    /// Uniquely sorted subject-field identifiers this policy governs.
    pub subject_field_ids: Vec<String>,
    /// Minimum signed proficiency level (0 = remember … 5 = create).
    pub minimum_level: u8,
    /// Uniquely sorted `did:key` issuers approved for this action and scope.
    pub accepted_issuers: Vec<Did>,
    /// Uniquely sorted qualification routes this policy permits.
    pub routes: Vec<QualificationRoute>,
    /// Distinct accepted issuers that must be among a course endorsement's
    /// verified attestors for the course-endorsement route.
    pub minimum_accepted_attestors: u16,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QualificationPolicyError {
    #[error("qualification policy exceeds {MAX_QUALIFICATION_POLICY_BYTES} bytes")]
    TooLarge,
    #[error("qualification policy digest {0} is not pinned by the network profile")]
    NotPinned(String),
    #[error("pinned qualification policy {0} was not supplied")]
    MissingPinned(String),
    #[error("qualification policy {0} was supplied more than once")]
    Duplicate(String),
    #[error("invalid qualification policy JSON: {0}")]
    Json(String),
    #[error("qualification policy bytes are not canonical JSON")]
    NotCanonical,
    #[error("unsupported qualification policy format version: {0}")]
    UnsupportedVersion(u32),
    #[error("qualification policy is for network {actual}; expected {expected}")]
    WrongNetwork { expected: String, actual: String },
    #[error("invalid qualification policy: {0}")]
    Invalid(String),
    #[error("qualification policies overlap for {action:?} in subject field {subject_field_id}")]
    OverlappingScope {
        action: QualificationAction,
        subject_field_id: String,
    },
}

impl SubjectQualificationPolicy {
    pub fn validate(&self) -> Result<(), QualificationPolicyError> {
        if self.format_version != QUALIFICATION_POLICY_FORMAT_VERSION {
            return Err(QualificationPolicyError::UnsupportedVersion(
                self.format_version,
            ));
        }
        validate_identifier("policy_id", &self.policy_id)?;
        validate_identifier("network_id", &self.network_id)?;
        if self.subject_field_ids.is_empty()
            || self.subject_field_ids.len() > MAX_POLICY_SUBJECT_FIELDS
        {
            return invalid("subject_field_ids must contain between 1 and 256 entries");
        }
        if !self
            .subject_field_ids
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return invalid("subject_field_ids must be uniquely sorted");
        }
        for field in &self.subject_field_ids {
            validate_identifier("subject_field_id", field)?;
        }
        if self.minimum_level > MAX_PROFICIENCY_LEVEL {
            return invalid("minimum_level must be between 0 and 5");
        }
        if self.accepted_issuers.is_empty() || self.accepted_issuers.len() > MAX_ACCEPTED_ISSUERS {
            return invalid("accepted_issuers must contain between 1 and 64 entries");
        }
        if !self
            .accepted_issuers
            .windows(2)
            .all(|pair| pair[0].as_str() < pair[1].as_str())
        {
            return invalid("accepted_issuers must be uniquely sorted");
        }
        for issuer in &self.accepted_issuers {
            parse_did_key(issuer.as_str())
                .map_err(|_| QualificationPolicyError::Invalid("invalid accepted issuer".into()))?;
        }
        if self.routes.is_empty() || !self.routes.windows(2).all(|pair| pair[0] < pair[1]) {
            return invalid("routes must be a non-empty uniquely sorted list");
        }
        if self.minimum_accepted_attestors == 0
            || usize::from(self.minimum_accepted_attestors) > self.accepted_issuers.len()
        {
            return invalid("minimum_accepted_attestors must be within the accepted issuers");
        }
        Ok(())
    }

    fn governs(&self, action: QualificationAction, subject_field_id: &str) -> bool {
        self.action == action
            && self
                .subject_field_ids
                .binary_search_by(|field| field.as_str().cmp(subject_field_id))
                .is_ok()
    }
}

/// Lowercase hex SHA-256 of exact policy bytes.
pub fn qualification_policy_digest(bytes: &[u8]) -> String {
    hex::encode(sha256(bytes))
}

/// A policy whose exact canonical bytes match a digest the network pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedQualificationPolicy {
    digest: String,
    policy: SubjectQualificationPolicy,
}

impl PinnedQualificationPolicy {
    pub fn parse(
        bytes: &[u8],
        pinned_digests: &[String],
        network_id: &str,
    ) -> Result<Self, QualificationPolicyError> {
        if bytes.len() > MAX_QUALIFICATION_POLICY_BYTES {
            return Err(QualificationPolicyError::TooLarge);
        }
        let digest = qualification_policy_digest(bytes);
        if !pinned_digests.contains(&digest) {
            return Err(QualificationPolicyError::NotPinned(digest));
        }
        let policy: SubjectQualificationPolicy = serde_json::from_slice(bytes)
            .map_err(|error| QualificationPolicyError::Json(error.to_string()))?;
        let canonical = serde_json_canonicalizer::to_vec(&policy)
            .map_err(|error| QualificationPolicyError::Json(error.to_string()))?;
        if canonical != bytes {
            return Err(QualificationPolicyError::NotCanonical);
        }
        policy.validate()?;
        if policy.network_id != network_id {
            return Err(QualificationPolicyError::WrongNetwork {
                expected: network_id.to_owned(),
                actual: policy.network_id,
            });
        }
        Ok(Self { digest, policy })
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn policy(&self) -> &SubjectQualificationPolicy {
        &self.policy
    }
}

/// The complete set of policies a network pins. Every pinned digest must be
/// supplied exactly once, and at most one policy may govern an action in a
/// subject field, so the applicable policy is never a matter of ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationPolicySet {
    network_id: String,
    policies: Vec<PinnedQualificationPolicy>,
}

impl QualificationPolicySet {
    pub fn from_pinned(
        documents: &[&[u8]],
        pinned_digests: &[String],
        network_id: &str,
    ) -> Result<Self, QualificationPolicyError> {
        let mut policies = Vec::with_capacity(documents.len());
        let mut supplied = HashSet::new();
        for bytes in documents {
            let pinned = PinnedQualificationPolicy::parse(bytes, pinned_digests, network_id)?;
            if !supplied.insert(pinned.digest.clone()) {
                return Err(QualificationPolicyError::Duplicate(pinned.digest));
            }
            policies.push(pinned);
        }
        if let Some(missing) = pinned_digests
            .iter()
            .find(|digest| !supplied.contains(*digest))
        {
            return Err(QualificationPolicyError::MissingPinned(missing.clone()));
        }
        let mut scopes = HashSet::new();
        for pinned in &policies {
            for field in &pinned.policy.subject_field_ids {
                if !scopes.insert((pinned.policy.action, field.as_str())) {
                    return Err(QualificationPolicyError::OverlappingScope {
                        action: pinned.policy.action,
                        subject_field_id: field.clone(),
                    });
                }
            }
        }
        Ok(Self {
            network_id: network_id.to_owned(),
            policies,
        })
    }

    pub fn network_id(&self) -> &str {
        &self.network_id
    }

    pub fn policies(&self) -> &[PinnedQualificationPolicy] {
        &self.policies
    }

    /// The single policy governing `action` in `subject_field_id`, if any.
    pub fn applicable(
        &self,
        action: QualificationAction,
        subject_field_id: &str,
    ) -> Option<&PinnedQualificationPolicy> {
        self.policies
            .iter()
            .find(|pinned| pinned.policy.governs(action, subject_field_id))
    }

    /// Decide whether `input.credential` qualifies `input.actor_did` for the
    /// action in the subject field.
    pub fn evaluate(&self, input: QualificationInput<'_>) -> QualificationDecision {
        let Some(pinned) = self.applicable(input.action, input.subject_field_id) else {
            return not_qualified(NotQualifiedReason::NoApplicablePolicy);
        };
        let trust = classify_credential(
            input.credential,
            input.verification,
            input.verification_policy,
            &self.network_id,
            input.endorsement,
        );
        match &trust {
            CredentialTrust::Invalid { .. } => {
                return not_qualified(NotQualifiedReason::InvalidCredential)
            }
            CredentialTrust::Pending { reasons } => {
                return QualificationDecision::Pending {
                    reasons: reasons.clone(),
                }
            }
            _ => {}
        }
        if input.credential.credential_subject.id != *input.actor_did {
            return not_qualified(NotQualifiedReason::ActorNotSubject);
        }
        let Some(claim) = SkillClaim::extract(&input.credential.credential_subject) else {
            return not_qualified(NotQualifiedReason::NotSkillClaim);
        };
        match input.skill_subject_field_id {
            None => return not_qualified(NotQualifiedReason::SkillNotInTaxonomy),
            Some(field) if field != input.subject_field_id => {
                return not_qualified(NotQualifiedReason::SkillOutsideScope)
            }
            Some(_) => {}
        }
        let policy = &pinned.policy;
        if claim.level < policy.minimum_level {
            return not_qualified(NotQualifiedReason::LevelBelowMinimum);
        }
        let accepted = |did: &Did| policy.accepted_issuers.iter().any(|issuer| issuer == did);
        let permits = |route| policy.routes.contains(&route);
        let (route, qualifying_issuers) = match trust {
            CredentialTrust::VerifiedIssuerSigned { issuer }
                if permits(QualificationRoute::AcceptedIssuer) && accepted(&issuer) =>
            {
                (QualificationRoute::AcceptedIssuer, vec![issuer])
            }
            CredentialTrust::VerifiedIssuerSigned { .. } => {
                return not_qualified(NotQualifiedReason::IssuerNotAccepted)
            }
            CredentialTrust::VerifiedCourseEndorsement { attestors, .. }
                if permits(QualificationRoute::AcceptedCourseEndorsement) =>
            {
                let qualifying = attestors
                    .into_iter()
                    .filter(|attestor| accepted(attestor))
                    .collect::<Vec<_>>();
                if qualifying.len() < usize::from(policy.minimum_accepted_attestors) {
                    return not_qualified(NotQualifiedReason::EndorsementNotAccepted);
                }
                (QualificationRoute::AcceptedCourseEndorsement, qualifying)
            }
            CredentialTrust::VerifiedCourseEndorsement { .. } => {
                return not_qualified(NotQualifiedReason::EndorsementNotAccepted)
            }
            CredentialTrust::VerifiedSelfClaim { .. } => {
                return not_qualified(NotQualifiedReason::SelfClaimNotAccepted)
            }
            CredentialTrust::Invalid { .. } | CredentialTrust::Pending { .. } => {
                unreachable!("invalid and pending trust returned above")
            }
        };
        QualificationDecision::Qualified(PolicyQualification {
            policy_id: policy.policy_id.clone(),
            policy_digest: pinned.digest.clone(),
            calculation_version: QUALIFICATION_CALCULATION_VERSION,
            trust_calculation_version: TRUST_CALCULATION_VERSION,
            action: input.action,
            subject_field_id: input.subject_field_id.to_owned(),
            credential_id: input.credential.id.clone(),
            skill_id: claim.skill_id,
            level: claim.level,
            route,
            qualifying_issuers,
        })
    }
}

/// Everything evaluation needs. `skill_subject_field_id` is the subject field
/// the signed skill belongs to in the caller's trusted taxonomy, or `None`
/// when that skill is unknown locally.
#[derive(Debug, Clone, Copy)]
pub struct QualificationInput<'a> {
    pub action: QualificationAction,
    pub actor_did: &'a Did,
    pub subject_field_id: &'a str,
    pub credential: &'a VerifiableCredential,
    pub verification: &'a VerificationResult,
    pub verification_policy: &'a VerificationPolicy,
    pub endorsement: Option<CourseEndorsementEvidence<'a>>,
    pub skill_subject_field_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum QualificationDecision {
    Qualified(PolicyQualification),
    /// Verification evidence is missing or unavailable; not a refusal.
    Pending {
        reasons: Vec<VerificationPendingReason>,
    },
    NotQualified {
        reason: NotQualifiedReason,
    },
}

/// Evidence that a credential satisfied one pinned policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyQualification {
    pub policy_id: String,
    pub policy_digest: String,
    pub calculation_version: u32,
    pub trust_calculation_version: u32,
    pub action: QualificationAction,
    pub subject_field_id: String,
    pub credential_id: Option<String>,
    pub skill_id: String,
    pub level: u8,
    pub route: QualificationRoute,
    pub qualifying_issuers: Vec<Did>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotQualifiedReason {
    /// The network pins no policy for this action in this subject field.
    NoApplicablePolicy,
    InvalidCredential,
    /// The credential is about someone other than the acting identity.
    ActorNotSubject,
    NotSkillClaim,
    SkillNotInTaxonomy,
    SkillOutsideScope,
    LevelBelowMinimum,
    /// Authentic, but the subject vouched for themselves.
    SelfClaimNotAccepted,
    IssuerNotAccepted,
    EndorsementNotAccepted,
}

fn not_qualified(reason: NotQualifiedReason) -> QualificationDecision {
    QualificationDecision::NotQualified { reason }
}

fn validate_identifier(field: &str, value: &str) -> Result<(), QualificationPolicyError> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
    {
        return invalid(format!(
            "{field} must be 1-128 printable ASCII bytes without whitespace"
        ));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, QualificationPolicyError> {
    Err(QualificationPolicyError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::course::{
        sign_completion_endorsement, AuthorizedAttestor, CompletionEvidence,
        CourseCompletionBinding, CourseCompletionPolicy, EvidenceRequirement,
        COMPLETION_ENDORSEMENT_FORMAT_VERSION, COMPLETION_POLICY_FORMAT_VERSION,
    };
    use crate::did::{derive_did_key, VerificationMethodRef};
    use crate::trust::{completion_root_evidence_ref, course_document_evidence_ref};
    use crate::vc::sign::{sign_credential, UnsignedCredential};
    use crate::vc::verify::verify_credential;
    use crate::vc::{Claim, CredentialStatus, Proof};
    use crate::NullStore;

    const NOW: &str = "2026-09-15T00:00:00Z";
    const NETWORK: &str = "preprod";
    const FIELD: &str = "field-a";
    const OTHER_FIELD: &str = "field-b";
    const DOCUMENT_CID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const COMPLETION_ROOT: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn sorted_dids(keys: &[&SigningKey]) -> Vec<Did> {
        let mut dids = keys
            .iter()
            .map(|key| derive_did_key(key))
            .collect::<Vec<_>>();
        dids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        dids
    }

    fn skill_credential(
        issuer: &SigningKey,
        subject: &Did,
        level: u8,
        evidence_refs: Vec<String>,
    ) -> VerifiableCredential {
        let issuer_did = derive_did_key(issuer);
        let class = if issuer_did == *subject {
            "SelfAssertion"
        } else {
            "AssessmentCredential"
        };
        let claim = Claim::Skill(SkillClaim {
            skill_id: "skill".into(),
            level,
            score: 0.8,
            evidence_refs,
            rubric_version: None,
            assessment_method: None,
            provenance: None,
        });
        let unsigned = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some("urn:uuid:qualification-test".into()),
            type_: vec!["VerifiableCredential".into(), class.into()],
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

    fn policy(
        accepted: &[&SigningKey],
        routes: Vec<QualificationRoute>,
    ) -> SubjectQualificationPolicy {
        SubjectQualificationPolicy {
            format_version: QUALIFICATION_POLICY_FORMAT_VERSION,
            policy_id: "field-a-opinions".into(),
            network_id: NETWORK.into(),
            action: QualificationAction::OpinionPosting,
            subject_field_ids: vec![FIELD.into()],
            minimum_level: 2,
            accepted_issuers: sorted_dids(accepted),
            routes,
            minimum_accepted_attestors: 1,
        }
    }

    fn canonical(policy: &SubjectQualificationPolicy) -> Vec<u8> {
        serde_json_canonicalizer::to_vec(policy).unwrap()
    }

    fn policy_set(policies: &[SubjectQualificationPolicy]) -> QualificationPolicySet {
        let documents = policies.iter().map(canonical).collect::<Vec<_>>();
        let digests = documents
            .iter()
            .map(|bytes| qualification_policy_digest(bytes))
            .collect::<Vec<_>>();
        let slices = documents.iter().map(Vec::as_slice).collect::<Vec<_>>();
        QualificationPolicySet::from_pinned(&slices, &digests, NETWORK).expect("policy set")
    }

    fn evaluate(
        set: &QualificationPolicySet,
        actor: &Did,
        field: &str,
        credential: &VerifiableCredential,
        endorsement: Option<CourseEndorsementEvidence<'_>>,
        skill_field: Option<&str>,
    ) -> QualificationDecision {
        let verification_policy = VerificationPolicy::default();
        let verification = verify_credential(&NullStore, credential, NOW, &verification_policy);
        set.evaluate(QualificationInput {
            action: QualificationAction::OpinionPosting,
            actor_did: actor,
            subject_field_id: field,
            credential,
            verification: &verification,
            verification_policy: &verification_policy,
            endorsement,
            skill_subject_field_id: skill_field,
        })
    }

    fn refusal(decision: QualificationDecision) -> NotQualifiedReason {
        match decision {
            QualificationDecision::NotQualified { reason } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn completion_policy(attestors: &[&SigningKey], required: u16) -> CourseCompletionPolicy {
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

    fn completion_binding(subject: &Did) -> CourseCompletionBinding {
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

    fn completion_refs() -> Vec<String> {
        vec![
            course_document_evidence_ref(DOCUMENT_CID, 2),
            completion_root_evidence_ref(COMPLETION_ROOT),
        ]
    }

    #[test]
    fn only_exact_pinned_canonical_policy_bytes_parse() {
        let instructor = key(3);
        let document = policy(&[&instructor], vec![QualificationRoute::AcceptedIssuer]);
        let bytes = canonical(&document);
        let digest = qualification_policy_digest(&bytes);

        let pinned =
            PinnedQualificationPolicy::parse(&bytes, std::slice::from_ref(&digest), NETWORK)
                .unwrap();
        assert_eq!(pinned.digest(), digest);
        assert_eq!(pinned.policy(), &document);

        assert_eq!(
            PinnedQualificationPolicy::parse(&bytes, &[], NETWORK),
            Err(QualificationPolicyError::NotPinned(digest.clone()))
        );

        let pretty = serde_json::to_vec_pretty(&document).unwrap();
        let pretty_digest = qualification_policy_digest(&pretty);
        assert_eq!(
            PinnedQualificationPolicy::parse(&pretty, &[pretty_digest], NETWORK),
            Err(QualificationPolicyError::NotCanonical)
        );

        assert!(matches!(
            PinnedQualificationPolicy::parse(&bytes, &[digest], "mainnet"),
            Err(QualificationPolicyError::WrongNetwork { .. })
        ));

        let oversized = vec![b' '; MAX_QUALIFICATION_POLICY_BYTES + 1];
        let oversized_digest = qualification_policy_digest(&oversized);
        assert_eq!(
            PinnedQualificationPolicy::parse(&oversized, &[oversized_digest], NETWORK),
            Err(QualificationPolicyError::TooLarge)
        );
    }

    #[test]
    fn malformed_policies_fail_validation() {
        let (first, second) = (key(3), key(4));
        let valid = policy(&[&first, &second], vec![QualificationRoute::AcceptedIssuer]);
        assert!(valid.validate().is_ok());

        let mut unsorted = valid.clone();
        unsorted.accepted_issuers.reverse();
        let mut duplicate = valid.clone();
        duplicate.accepted_issuers = vec![derive_did_key(&first), derive_did_key(&first)];
        let mut malformed_issuer = valid.clone();
        malformed_issuer.accepted_issuers = vec![Did("did:web:example.com".into())];
        let mut no_routes = valid.clone();
        no_routes.routes.clear();
        let mut too_many_attestors = valid.clone();
        too_many_attestors.minimum_accepted_attestors = 3;
        let mut level = valid.clone();
        level.minimum_level = 6;
        let mut unsorted_fields = valid.clone();
        unsorted_fields.subject_field_ids = vec![OTHER_FIELD.into(), FIELD.into()];
        let mut version = valid;
        version.format_version = 2;

        for policy in [
            unsorted,
            duplicate,
            malformed_issuer,
            no_routes,
            too_many_attestors,
            level,
            unsorted_fields,
        ] {
            assert!(matches!(
                policy.validate(),
                Err(QualificationPolicyError::Invalid(_))
            ));
        }
        assert_eq!(
            version.validate(),
            Err(QualificationPolicyError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn policy_set_requires_every_pinned_policy_once_and_no_overlap() {
        let instructor = key(3);
        let first = policy(&[&instructor], vec![QualificationRoute::AcceptedIssuer]);
        let first_bytes = canonical(&first);
        let first_digest = qualification_policy_digest(&first_bytes);

        assert_eq!(
            QualificationPolicySet::from_pinned(&[], std::slice::from_ref(&first_digest), NETWORK),
            Err(QualificationPolicyError::MissingPinned(
                first_digest.clone()
            ))
        );
        assert_eq!(
            QualificationPolicySet::from_pinned(
                &[&first_bytes, &first_bytes],
                std::slice::from_ref(&first_digest),
                NETWORK
            ),
            Err(QualificationPolicyError::Duplicate(first_digest.clone()))
        );

        let mut overlapping = first.clone();
        overlapping.policy_id = "another-field-a-policy".into();
        let overlapping_bytes = canonical(&overlapping);
        let overlapping_digest = qualification_policy_digest(&overlapping_bytes);
        assert!(matches!(
            QualificationPolicySet::from_pinned(
                &[&first_bytes, &overlapping_bytes],
                &[first_digest, overlapping_digest],
                NETWORK
            ),
            Err(QualificationPolicyError::OverlappingScope { .. })
        ));

        let empty = QualificationPolicySet::from_pinned(&[], &[], NETWORK).unwrap();
        assert!(empty.policies().is_empty());
    }

    #[test]
    fn accepted_issuer_credential_qualifies_only_in_its_policy_scope() {
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        let set = policy_set(&[policy(
            &[&instructor],
            vec![QualificationRoute::AcceptedIssuer],
        )]);
        let credential = skill_credential(&instructor, &learner, 3, vec![]);

        match evaluate(&set, &learner, FIELD, &credential, None, Some(FIELD)) {
            QualificationDecision::Qualified(qualification) => {
                assert_eq!(qualification.route, QualificationRoute::AcceptedIssuer);
                assert_eq!(
                    qualification.qualifying_issuers,
                    vec![derive_did_key(&instructor)]
                );
                assert_eq!(qualification.policy_digest, set.policies()[0].digest());
                assert_eq!(qualification.level, 3);
            }
            other => panic!("expected qualification, got {other:?}"),
        }

        // No cross-scope privilege: the same authentic credential in a field
        // no pinned policy governs grants nothing.
        assert_eq!(
            refusal(evaluate(
                &set,
                &learner,
                OTHER_FIELD,
                &credential,
                None,
                Some(OTHER_FIELD)
            )),
            NotQualifiedReason::NoApplicablePolicy
        );
        assert_eq!(
            refusal(evaluate(
                &QualificationPolicySet::from_pinned(&[], &[], NETWORK).unwrap(),
                &learner,
                FIELD,
                &credential,
                None,
                Some(FIELD)
            )),
            NotQualifiedReason::NoApplicablePolicy
        );
    }

    #[test]
    fn self_claims_and_unaccepted_issuers_do_not_qualify() {
        let instructor = key(3);
        let learner_key = key(1);
        let learner = derive_did_key(&learner_key);
        let set = policy_set(&[policy(
            &[&instructor],
            vec![
                QualificationRoute::AcceptedCourseEndorsement,
                QualificationRoute::AcceptedIssuer,
            ],
        )]);

        let self_claim = skill_credential(&learner_key, &learner, 5, vec![]);
        assert_eq!(
            refusal(evaluate(
                &set,
                &learner,
                FIELD,
                &self_claim,
                None,
                Some(FIELD)
            )),
            NotQualifiedReason::SelfClaimNotAccepted
        );

        // Being an accepted issuer does not let that identity vouch for itself.
        let instructor_did = derive_did_key(&instructor);
        let instructor_self_claim = skill_credential(&instructor, &instructor_did, 5, vec![]);
        assert_eq!(
            refusal(evaluate(
                &set,
                &instructor_did,
                FIELD,
                &instructor_self_claim,
                None,
                Some(FIELD)
            )),
            NotQualifiedReason::SelfClaimNotAccepted
        );

        // A second identity the learner controls is authentic but unapproved.
        let sock_puppet = key(2);
        let puppet_credential = skill_credential(&sock_puppet, &learner, 5, vec![]);
        assert_eq!(
            refusal(evaluate(
                &set,
                &learner,
                FIELD,
                &puppet_credential,
                None,
                Some(FIELD)
            )),
            NotQualifiedReason::IssuerNotAccepted
        );
    }

    #[test]
    fn course_endorsement_route_requires_accepted_attestors() {
        let instructor = key(3);
        let learner_key = key(1);
        let learner = derive_did_key(&learner_key);
        let set = policy_set(&[policy(
            &[&instructor],
            vec![QualificationRoute::AcceptedCourseEndorsement],
        )]);
        let credential = skill_credential(&learner_key, &learner, 3, completion_refs());
        let binding = completion_binding(&learner);

        let accepted_policy = completion_policy(&[&instructor], 1);
        let accepted_endorsements =
            [
                sign_completion_endorsement(&accepted_policy, binding.clone(), &instructor)
                    .unwrap(),
            ];
        match evaluate(
            &set,
            &learner,
            FIELD,
            &credential,
            Some(CourseEndorsementEvidence {
                policy: &accepted_policy,
                binding: &binding,
                endorsements: &accepted_endorsements,
            }),
            Some(FIELD),
        ) {
            QualificationDecision::Qualified(qualification) => {
                assert_eq!(
                    qualification.route,
                    QualificationRoute::AcceptedCourseEndorsement
                );
                assert_eq!(
                    qualification.qualifying_issuers,
                    vec![derive_did_key(&instructor)]
                );
            }
            other => panic!("expected qualification, got {other:?}"),
        }

        // Two controlled identities plus an arbitrary course: the attacker
        // authors a completion policy naming their own attestor and endorses
        // themselves. The endorsement is authentic but its attestor is not an
        // accepted issuer, so the policy is not bypassed.
        let accomplice = key(2);
        let arbitrary_policy = completion_policy(&[&accomplice], 1);
        let arbitrary_endorsements =
            [
                sign_completion_endorsement(&arbitrary_policy, binding.clone(), &accomplice)
                    .unwrap(),
            ];
        assert_eq!(
            refusal(evaluate(
                &set,
                &learner,
                FIELD,
                &credential,
                Some(CourseEndorsementEvidence {
                    policy: &arbitrary_policy,
                    binding: &binding,
                    endorsements: &arbitrary_endorsements,
                }),
                Some(FIELD)
            )),
            NotQualifiedReason::EndorsementNotAccepted
        );

        // A policy that does not permit the route refuses even accepted
        // attestors.
        let issuer_only = policy_set(&[policy(
            &[&instructor],
            vec![QualificationRoute::AcceptedIssuer],
        )]);
        assert_eq!(
            refusal(evaluate(
                &issuer_only,
                &learner,
                FIELD,
                &credential,
                Some(CourseEndorsementEvidence {
                    policy: &accepted_policy,
                    binding: &binding,
                    endorsements: &accepted_endorsements,
                }),
                Some(FIELD)
            )),
            NotQualifiedReason::EndorsementNotAccepted
        );
    }

    #[test]
    fn actor_taxonomy_and_level_are_checked_against_signed_content() {
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        let set = policy_set(&[policy(
            &[&instructor],
            vec![QualificationRoute::AcceptedIssuer],
        )]);
        let credential = skill_credential(&instructor, &learner, 3, vec![]);

        let other_actor = derive_did_key(&key(9));
        assert_eq!(
            refusal(evaluate(
                &set,
                &other_actor,
                FIELD,
                &credential,
                None,
                Some(FIELD)
            )),
            NotQualifiedReason::ActorNotSubject
        );
        assert_eq!(
            refusal(evaluate(&set, &learner, FIELD, &credential, None, None)),
            NotQualifiedReason::SkillNotInTaxonomy
        );
        assert_eq!(
            refusal(evaluate(
                &set,
                &learner,
                FIELD,
                &credential,
                None,
                Some(OTHER_FIELD)
            )),
            NotQualifiedReason::SkillOutsideScope
        );
        let low = skill_credential(&instructor, &learner, 1, vec![]);
        assert_eq!(
            refusal(evaluate(&set, &learner, FIELD, &low, None, Some(FIELD))),
            NotQualifiedReason::LevelBelowMinimum
        );
    }

    #[test]
    fn invalid_or_pending_credentials_never_qualify() {
        let instructor = key(3);
        let learner = derive_did_key(&key(1));
        let set = policy_set(&[policy(
            &[&instructor],
            vec![QualificationRoute::AcceptedIssuer],
        )]);

        let mut tampered = skill_credential(&instructor, &learner, 3, vec![]);
        tampered
            .credential_subject
            .properties
            .insert("level".into(), serde_json::json!(5));
        assert_eq!(
            refusal(evaluate(
                &set,
                &learner,
                FIELD,
                &tampered,
                None,
                Some(FIELD)
            )),
            NotQualifiedReason::InvalidCredential
        );

        let mut with_status = skill_credential(&instructor, &learner, 3, vec![]);
        with_status.credential_status = Some(CredentialStatus {
            id: "urn:status:1#0".into(),
            type_: "BitstringStatusListEntry".into(),
            status_purpose: "revocation".into(),
            status_list_index: "0".into(),
            status_list_credential: "urn:status:1".into(),
        });
        let issuer = derive_did_key(&instructor);
        let with_status = sign_credential(
            UnsignedCredential {
                credential: with_status,
            },
            &instructor,
            &issuer,
        )
        .unwrap();
        assert_eq!(
            evaluate(&set, &learner, FIELD, &with_status, None, Some(FIELD)),
            QualificationDecision::Pending {
                reasons: vec![VerificationPendingReason::StatusListMissing]
            }
        );
    }
}
