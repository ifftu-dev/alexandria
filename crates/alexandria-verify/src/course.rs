//! Course completion policy and endorsement verification.
//!
//! These types are I/O-free so the app, cloud service, CLI, and independent
//! verifiers can make the same decision over the same signed bytes.

use std::collections::HashSet;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::did::{did_from_verifying_key, parse_did_key, Did};
use crate::json::{decode_untrusted, JsonLimits, UntrustedJsonError};

pub const COMPLETION_POLICY_FORMAT_VERSION: u32 = 1;
pub const COMPLETION_ENDORSEMENT_FORMAT_VERSION: u32 = 1;
pub const MAX_AUTHORIZED_ATTESTORS: usize = 64;
pub const MAX_EVIDENCE_REQUIREMENTS: usize = 64;
pub const MAX_COMPLETION_EVIDENCE: usize = 64;
pub const MAX_COMPLETION_ENDORSEMENT_BYTES: usize = 128 * 1024;

/// Structural limits for an untrusted completion endorsement document. An
/// endorsement nests its binding's evidence four levels deep, carries at most
/// the evidence limit in one array, and holds identifiers, DIDs and hex.
pub const COMPLETION_ENDORSEMENT_JSON_LIMITS: JsonLimits = JsonLimits {
    max_bytes: MAX_COMPLETION_ENDORSEMENT_BYTES,
    max_depth: 8,
    max_array_len: MAX_COMPLETION_EVIDENCE,
    max_object_entries: 16,
    max_string_bytes: 1024,
};

/// Decode the exact bytes of an untrusted completion endorsement under
/// [`COMPLETION_ENDORSEMENT_JSON_LIMITS`]. The result is not yet verified.
pub fn decode_completion_endorsement(
    bytes: &[u8],
) -> Result<CourseCompletionEndorsement, UntrustedJsonError> {
    decode_untrusted(bytes, &COMPLETION_ENDORSEMENT_JSON_LIMITS)
}

const MAX_IDENTIFIER_BYTES: usize = 256;
const ENDORSEMENT_DOMAIN: &[u8] = b"alexandria/course-completion-endorsement/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseCompletionPolicy {
    pub format_version: u32,
    pub required_attestors: u16,
    pub authorized_attestors: Vec<AuthorizedAttestor>,
    pub evidence_requirements: Vec<EvidenceRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedAttestor {
    pub did: Did,
    pub public_key_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirement {
    pub kind: String,
    pub format_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionEvidence {
    pub kind: String,
    pub format_version: u32,
    pub id: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseCompletionBinding {
    pub format_version: u32,
    pub network_id: String,
    pub subject_did: Did,
    pub course_id: String,
    pub course_document_cid: String,
    pub course_document_version: u32,
    pub completion_root: String,
    pub evidence: Vec<CompletionEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_tx_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseCompletionEndorsement {
    pub binding: CourseCompletionBinding,
    pub attestor_did: Did,
    pub attestor_public_key_hex: String,
    pub signature_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndorsementThresholdResult {
    pub required_attestors: u16,
    pub valid_attestors: Vec<Did>,
    pub rejected_endorsements: usize,
    pub satisfied: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CourseTrustError {
    #[error("unsupported course completion policy format version: {0}")]
    UnsupportedPolicyVersion(u32),
    #[error("unsupported course completion endorsement format version: {0}")]
    UnsupportedEndorsementVersion(u32),
    #[error("invalid course completion policy: {0}")]
    InvalidPolicy(String),
    #[error("invalid course completion binding: {0}")]
    InvalidBinding(String),
    #[error("attestor is not authorized by the signed course policy")]
    UnauthorizedAttestor,
    #[error("attestor DID does not match its Ed25519 public key")]
    AttestorIdentityMismatch,
    #[error("invalid Ed25519 public key")]
    InvalidPublicKey,
    #[error("invalid Ed25519 signature")]
    InvalidSignature,
    #[error("completion endorsement exceeds {MAX_COMPLETION_ENDORSEMENT_BYTES} bytes")]
    TooLarge,
    #[error("failed to canonicalize completion endorsement: {0}")]
    Canonicalization(String),
}

impl CourseCompletionPolicy {
    pub fn validate(&self) -> Result<(), CourseTrustError> {
        if self.format_version != COMPLETION_POLICY_FORMAT_VERSION {
            return Err(CourseTrustError::UnsupportedPolicyVersion(
                self.format_version,
            ));
        }
        if self.authorized_attestors.is_empty()
            || self.authorized_attestors.len() > MAX_AUTHORIZED_ATTESTORS
        {
            return invalid_policy("authorized_attestors must contain between 1 and 64 entries");
        }
        if self.required_attestors == 0
            || usize::from(self.required_attestors) > self.authorized_attestors.len()
        {
            return invalid_policy("required_attestors must be within the authorized set");
        }
        if self.evidence_requirements.is_empty()
            || self.evidence_requirements.len() > MAX_EVIDENCE_REQUIREMENTS
        {
            return invalid_policy("evidence_requirements must contain between 1 and 64 entries");
        }

        let mut previous_did: Option<&str> = None;
        let mut public_keys = HashSet::new();
        for attestor in &self.authorized_attestors {
            parse_did_key(attestor.did.as_str())
                .map_err(|_| CourseTrustError::InvalidPolicy("invalid attestor DID".into()))?;
            if previous_did.is_some_and(|previous| previous >= attestor.did.as_str()) {
                return invalid_policy("authorized_attestors must be uniquely sorted by DID");
            }
            previous_did = Some(attestor.did.as_str());

            let key = decode_verifying_key(&attestor.public_key_hex)?;
            if did_from_verifying_key(&key) != attestor.did {
                return Err(CourseTrustError::AttestorIdentityMismatch);
            }
            if !public_keys.insert(attestor.public_key_hex.as_str()) {
                return invalid_policy("authorized attestors must use distinct public keys");
            }
        }

        if !self
            .evidence_requirements
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return invalid_policy("evidence_requirements must be uniquely sorted");
        }
        for requirement in &self.evidence_requirements {
            validate_identifier(&requirement.kind).map_err(CourseTrustError::InvalidPolicy)?;
            if requirement.format_version == 0 {
                return invalid_policy("evidence format_version must be positive");
            }
        }
        Ok(())
    }

    fn authorized_key(&self, did: &Did) -> Option<&str> {
        self.authorized_attestors
            .iter()
            .find(|attestor| &attestor.did == did)
            .map(|attestor| attestor.public_key_hex.as_str())
    }
}

impl CourseCompletionBinding {
    pub fn validate(&self, policy: &CourseCompletionPolicy) -> Result<(), CourseTrustError> {
        policy.validate()?;
        if self.format_version != COMPLETION_ENDORSEMENT_FORMAT_VERSION {
            return Err(CourseTrustError::UnsupportedEndorsementVersion(
                self.format_version,
            ));
        }
        validate_identifier(&self.network_id).map_err(CourseTrustError::InvalidBinding)?;
        parse_did_key(self.subject_did.as_str())
            .map_err(|_| CourseTrustError::InvalidBinding("invalid subject DID".into()))?;
        validate_identifier(&self.course_id).map_err(CourseTrustError::InvalidBinding)?;
        if self.course_document_version == 0 {
            return invalid_binding("course_document_version must be positive");
        }
        validate_digest("course_document_cid", &self.course_document_cid)?;
        validate_digest("completion_root", &self.completion_root)?;
        if let Some(hash) = &self.witness_tx_hash {
            validate_digest("witness_tx_hash", hash)?;
        }
        if self.evidence.is_empty() || self.evidence.len() > MAX_COMPLETION_EVIDENCE {
            return invalid_binding("evidence must contain between 1 and 64 entries");
        }
        if !self.evidence.windows(2).all(|pair| pair[0] < pair[1]) {
            return invalid_binding("evidence must be uniquely sorted");
        }
        for evidence in &self.evidence {
            validate_identifier(&evidence.kind).map_err(CourseTrustError::InvalidBinding)?;
            validate_identifier(&evidence.id).map_err(CourseTrustError::InvalidBinding)?;
            validate_digest("evidence digest", &evidence.digest)?;
            if evidence.format_version == 0 {
                return invalid_binding("evidence format_version must be positive");
            }
        }
        for requirement in &policy.evidence_requirements {
            if !self.evidence.iter().any(|evidence| {
                evidence.kind == requirement.kind
                    && evidence.format_version == requirement.format_version
            }) {
                return invalid_binding(format!(
                    "missing required evidence {} version {}",
                    requirement.kind, requirement.format_version
                ));
            }
        }
        Ok(())
    }
}

pub fn sign_completion_endorsement(
    policy: &CourseCompletionPolicy,
    binding: CourseCompletionBinding,
    key: &SigningKey,
) -> Result<CourseCompletionEndorsement, CourseTrustError> {
    binding.validate(policy)?;
    let attestor_did = did_from_verifying_key(&key.verifying_key());
    let public_key_hex = hex::encode(key.verifying_key().as_bytes());
    if policy.authorized_key(&attestor_did) != Some(public_key_hex.as_str()) {
        return Err(CourseTrustError::UnauthorizedAttestor);
    }
    let signature = key.sign(&completion_endorsement_signing_bytes(&binding)?);
    Ok(CourseCompletionEndorsement {
        binding,
        attestor_did,
        attestor_public_key_hex: public_key_hex,
        signature_hex: hex::encode(signature.to_bytes()),
    })
}

pub fn verify_completion_endorsement(
    policy: &CourseCompletionPolicy,
    expected_binding: &CourseCompletionBinding,
    endorsement: &CourseCompletionEndorsement,
) -> Result<(), CourseTrustError> {
    expected_binding.validate(policy)?;
    if endorsement.binding != *expected_binding {
        return invalid_binding("endorsement does not match the expected completion binding");
    }
    let encoded = serde_json_canonicalizer::to_vec(endorsement)
        .map_err(|error| CourseTrustError::Canonicalization(error.to_string()))?;
    if encoded.len() > MAX_COMPLETION_ENDORSEMENT_BYTES {
        return Err(CourseTrustError::TooLarge);
    }

    let authorized_key = policy
        .authorized_key(&endorsement.attestor_did)
        .ok_or(CourseTrustError::UnauthorizedAttestor)?;
    if authorized_key != endorsement.attestor_public_key_hex {
        return Err(CourseTrustError::UnauthorizedAttestor);
    }
    let key = decode_verifying_key(&endorsement.attestor_public_key_hex)?;
    if did_from_verifying_key(&key) != endorsement.attestor_did {
        return Err(CourseTrustError::AttestorIdentityMismatch);
    }
    let signature = decode_signature(&endorsement.signature_hex)?;
    key.verify(
        &completion_endorsement_signing_bytes(expected_binding)?,
        &signature,
    )
    .map_err(|_| CourseTrustError::InvalidSignature)
}

pub fn evaluate_completion_endorsements(
    policy: &CourseCompletionPolicy,
    expected_binding: &CourseCompletionBinding,
    endorsements: &[CourseCompletionEndorsement],
) -> Result<EndorsementThresholdResult, CourseTrustError> {
    expected_binding.validate(policy)?;
    let mut valid = Vec::new();
    let mut seen = HashSet::new();
    let mut rejected = 0usize;
    for endorsement in endorsements {
        if verify_completion_endorsement(policy, expected_binding, endorsement).is_ok()
            && seen.insert(endorsement.attestor_did.clone())
        {
            valid.push(endorsement.attestor_did.clone());
        } else {
            rejected += 1;
        }
    }
    valid.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    Ok(EndorsementThresholdResult {
        required_attestors: policy.required_attestors,
        satisfied: valid.len() >= usize::from(policy.required_attestors),
        valid_attestors: valid,
        rejected_endorsements: rejected,
    })
}

fn completion_endorsement_signing_bytes(
    binding: &CourseCompletionBinding,
) -> Result<Vec<u8>, CourseTrustError> {
    let canonical = serde_json_canonicalizer::to_vec(binding)
        .map_err(|error| CourseTrustError::Canonicalization(error.to_string()))?;
    if canonical.len() > MAX_COMPLETION_ENDORSEMENT_BYTES {
        return Err(CourseTrustError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(ENDORSEMENT_DOMAIN.len() + 1 + canonical.len());
    bytes.extend_from_slice(ENDORSEMENT_DOMAIN);
    bytes.push(0);
    bytes.extend_from_slice(&canonical);
    Ok(bytes)
}

fn decode_verifying_key(encoded: &str) -> Result<VerifyingKey, CourseTrustError> {
    let bytes = decode_canonical_hex(encoded, 32).ok_or(CourseTrustError::InvalidPublicKey)?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CourseTrustError::InvalidPublicKey)?;
    VerifyingKey::from_bytes(&array).map_err(|_| CourseTrustError::InvalidPublicKey)
}

fn decode_signature(encoded: &str) -> Result<Signature, CourseTrustError> {
    let bytes = decode_canonical_hex(encoded, 64).ok_or(CourseTrustError::InvalidSignature)?;
    let array: [u8; 64] = bytes
        .try_into()
        .map_err(|_| CourseTrustError::InvalidSignature)?;
    Ok(Signature::from_bytes(&array))
}

fn decode_canonical_hex(encoded: &str, bytes: usize) -> Option<Vec<u8>> {
    if encoded.len() != bytes * 2
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    hex::decode(encoded).ok()
}

fn validate_digest(label: &str, encoded: &str) -> Result<(), CourseTrustError> {
    if decode_canonical_hex(encoded, 32).is_none() {
        return invalid_binding(format!("{label} must be canonical lowercase 32-byte hex"));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
    {
        return Err("identifier must be 1-256 printable ASCII bytes without whitespace".into());
    }
    Ok(())
}

fn invalid_policy<T>(message: impl Into<String>) -> Result<T, CourseTrustError> {
    Err(CourseTrustError::InvalidPolicy(message.into()))
}

fn invalid_binding<T>(message: impl Into<String>) -> Result<T, CourseTrustError> {
    Err(CourseTrustError::InvalidBinding(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn attestor(key: &SigningKey) -> AuthorizedAttestor {
        AuthorizedAttestor {
            did: did_from_verifying_key(&key.verifying_key()),
            public_key_hex: hex::encode(key.verifying_key().as_bytes()),
        }
    }

    fn policy(keys: &[&SigningKey], required: u16) -> CourseCompletionPolicy {
        let mut authorized_attestors = keys.iter().map(|key| attestor(key)).collect::<Vec<_>>();
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

    fn binding(subject: &SigningKey) -> CourseCompletionBinding {
        CourseCompletionBinding {
            format_version: COMPLETION_ENDORSEMENT_FORMAT_VERSION,
            network_id: "preprod".into(),
            subject_did: did_from_verifying_key(&subject.verifying_key()),
            course_id: "course-1".into(),
            course_document_cid: "11".repeat(32),
            course_document_version: 2,
            completion_root: "22".repeat(32),
            evidence: vec![CompletionEvidence {
                kind: "graded-submissions".into(),
                format_version: 1,
                id: "attempt-1".into(),
                digest: "33".repeat(32),
            }],
            witness_tx_hash: None,
        }
    }

    #[test]
    fn exact_binding_and_distinct_authorized_attestors_satisfy_threshold() {
        let first = key(1);
        let second = key(2);
        let subject = key(9);
        let policy = policy(&[&first, &second], 2);
        let binding = binding(&subject);
        let endorsements = vec![
            sign_completion_endorsement(&policy, binding.clone(), &first).unwrap(),
            sign_completion_endorsement(&policy, binding.clone(), &second).unwrap(),
        ];

        let result = evaluate_completion_endorsements(&policy, &binding, &endorsements).unwrap();
        assert!(result.satisfied);
        assert_eq!(result.valid_attestors.len(), 2);
        assert_eq!(result.rejected_endorsements, 0);
    }

    #[test]
    fn duplicate_signature_counts_once() {
        let first = key(1);
        let second = key(2);
        let subject = key(9);
        let policy = policy(&[&first, &second], 2);
        let binding = binding(&subject);
        let endorsement = sign_completion_endorsement(&policy, binding.clone(), &first).unwrap();

        let result = evaluate_completion_endorsements(
            &policy,
            &binding,
            &[endorsement.clone(), endorsement],
        )
        .unwrap();
        assert!(!result.satisfied);
        assert_eq!(result.valid_attestors.len(), 1);
        assert_eq!(result.rejected_endorsements, 1);
    }

    #[test]
    fn unlisted_attestor_and_changed_binding_are_rejected() {
        let first = key(1);
        let outsider = key(3);
        let subject = key(9);
        let policy = policy(&[&first], 1);
        let binding = binding(&subject);

        assert_eq!(
            sign_completion_endorsement(&policy, binding.clone(), &outsider),
            Err(CourseTrustError::UnauthorizedAttestor)
        );

        let endorsement = sign_completion_endorsement(&policy, binding.clone(), &first).unwrap();
        let mut changed = binding;
        changed.network_id = "other".into();
        assert!(matches!(
            verify_completion_endorsement(&policy, &changed, &endorsement),
            Err(CourseTrustError::InvalidBinding(_))
        ));
    }

    #[test]
    fn policy_rejects_duplicates_bad_thresholds_and_missing_evidence() {
        let first = key(1);
        let subject = key(9);
        let mut duplicate_policy = policy(&[&first], 1);
        duplicate_policy.authorized_attestors.push(attestor(&first));
        assert!(matches!(
            duplicate_policy.validate(),
            Err(CourseTrustError::InvalidPolicy(_))
        ));

        let invalid_threshold_policy = policy(&[&first], 2);
        assert!(matches!(
            invalid_threshold_policy.validate(),
            Err(CourseTrustError::InvalidPolicy(_))
        ));

        let policy = policy(&[&first], 1);
        let mut binding = binding(&subject);
        binding.evidence[0].kind = "different".into();
        assert!(matches!(
            binding.validate(&policy),
            Err(CourseTrustError::InvalidBinding(_))
        ));
    }

    #[test]
    fn identity_or_signature_substitution_is_rejected() {
        let first = key(1);
        let second = key(2);
        let subject = key(9);
        let policy = policy(&[&first, &second], 1);
        let binding = binding(&subject);
        let mut endorsement =
            sign_completion_endorsement(&policy, binding.clone(), &first).unwrap();
        endorsement.attestor_did = did_from_verifying_key(&second.verifying_key());
        endorsement.attestor_public_key_hex = hex::encode(second.verifying_key().as_bytes());
        assert_eq!(
            verify_completion_endorsement(&policy, &binding, &endorsement),
            Err(CourseTrustError::InvalidSignature)
        );
    }
}
