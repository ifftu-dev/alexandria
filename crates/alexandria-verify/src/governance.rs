//! Verifiable quorum certificates for governance submission and close events.
//!
//! These types deliberately contain no Cardano transaction reference. Chain
//! anchoring can commit a verified certificate later, but it is not part of
//! deadline enforcement or finality. The ordered governance log and its
//! consensus implementation are separate concerns; this module only verifies
//! that a certificate is bound to one log commitment and authorized epoch.

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const GOVERNANCE_CERTIFICATE_VERSION: u16 = 1;
pub const GOVERNANCE_GENESIS_VERSION: u16 = 1;
pub const GOVERNANCE_COMMITTEE_SIZE: usize = 7;
pub const GOVERNANCE_QUORUM: usize = 5;
pub const MAX_GOVERNANCE_GENESIS_BYTES: usize = 256 * 1024;

const GENESIS_ACCEPTANCE_DOMAIN: &[u8] = b"alexandria/governance/genesis-acceptance/v1";
const GENESIS_ID_DOMAIN: &[u8] = b"alexandria/governance/genesis-id/v1";
const GENESIS_RULES_DOMAIN: &[u8] = b"alexandria/governance/genesis-rules/v1";
const RECEIPT_DOMAIN: &[u8] = b"alexandria/governance/receipt/v1";
const CLOSE_DOMAIN: &[u8] = b"alexandria/governance/close/v1";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GenesisError {
    #[error("governance genesis uses unsupported format version {0}")]
    UnsupportedVersion(u16),
    #[error("governance genesis contains an invalid scope, name, version, or activation value")]
    InvalidBinding,
    #[error("governance genesis must declare the approved seven-member, five-signature rules")]
    InvalidRules,
    #[error("governance genesis must contain seven canonically ordered independent members")]
    InvalidMembers,
    #[error("governance genesis contains a malformed or reused member key")]
    InvalidMemberKey,
    #[error("governance genesis qualification policy is empty, duplicated, or not canonical")]
    InvalidQualificationPolicy,
    #[error("governance genesis must contain one valid acceptance from every founding member")]
    InvalidAcceptances,
    #[error("claimed DAO id does not match the fully signed genesis envelope")]
    DaoIdMismatch,
    #[error("governance genesis bytes are not the canonical envelope encoding")]
    NonCanonicalEncoding,
    #[error("governance genesis exceeds the {MAX_GOVERNANCE_GENESIS_BYTES}-byte limit")]
    TooLarge,
    #[error("failed to encode canonical governance genesis: {0}")]
    Canonicalization(String),
    #[error("failed to decode governance genesis: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenesisScope {
    pub scope_type: String,
    pub scope_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenesisMember {
    pub member_id: String,
    pub identity_public_key_hex: String,
    pub consensus_public_key_hex: String,
    pub governance_public_key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenesisQualificationPolicy {
    pub policy_version: String,
    pub accepted_issuers: Vec<String>,
    pub accepted_assessment_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenesisRules {
    pub rules_version: String,
    pub committee_size: u8,
    pub receipt_threshold: u8,
    pub outcome_threshold: u8,
    pub proposal_approval_numerator: u32,
    pub proposal_approval_denominator: u32,
    pub minimum_turnout_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenesisActivation {
    pub cometbft_chain_id: String,
    pub initial_epoch: u64,
    pub initial_height: u64,
    pub activation_time_unix: i64,
}

/// Canonical genesis statement signed by all seven founding members.
///
/// It deliberately contains no DAO id or genesis hash. Those values are
/// derived only after the seven acceptances have been verified.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoundingGenesisCore {
    pub genesis_version: u16,
    pub protocol_version: u16,
    pub name: String,
    pub scope: GenesisScope,
    pub members: Vec<GenesisMember>,
    pub rules: GenesisRules,
    pub qualification_policy: GenesisQualificationPolicy,
    pub activation: GenesisActivation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoundingAcceptance {
    pub member_id: String,
    pub identity_signature_hex: String,
    pub consensus_signature_hex: String,
    pub governance_signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoundingGenesisEnvelope {
    pub core: FoundingGenesisCore,
    pub acceptances: Vec<FoundingAcceptance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedFoundingGenesis {
    pub dao_id: String,
    pub genesis_hash: String,
    pub rules_hash: String,
    pub committee: CommitteeEpoch,
}

impl FoundingGenesisCore {
    pub fn validate(&self) -> Result<(), GenesisError> {
        let encoded = serde_json_canonicalizer::to_vec(self)
            .map_err(|error| GenesisError::Canonicalization(error.to_string()))?;
        if encoded.len() > MAX_GOVERNANCE_GENESIS_BYTES {
            return Err(GenesisError::TooLarge);
        }
        if self.genesis_version != GOVERNANCE_GENESIS_VERSION {
            return Err(GenesisError::UnsupportedVersion(self.genesis_version));
        }
        if self.protocol_version != GOVERNANCE_CERTIFICATE_VERSION
            || !is_canonical_text(&self.name)
            || !is_canonical_text(&self.scope.scope_type)
            || !is_canonical_text(&self.scope.scope_id)
            || !is_canonical_text(&self.activation.cometbft_chain_id)
            || self.activation.initial_epoch != 0
            || self.activation.initial_height == 0
            || self.activation.activation_time_unix < 0
        {
            return Err(GenesisError::InvalidBinding);
        }
        if self.rules.committee_size as usize != GOVERNANCE_COMMITTEE_SIZE
            || self.rules.receipt_threshold as usize != GOVERNANCE_QUORUM
            || self.rules.outcome_threshold as usize != GOVERNANCE_QUORUM
            || self.rules.proposal_approval_numerator != 2
            || self.rules.proposal_approval_denominator != 3
            || self.rules.minimum_turnout_count == 0
            || !is_canonical_text(&self.rules.rules_version)
        {
            return Err(GenesisError::InvalidRules);
        }
        validate_sorted_unique_text(&self.qualification_policy.accepted_issuers)
            .and_then(|_| {
                validate_sorted_unique_text(&self.qualification_policy.accepted_assessment_evidence)
            })
            .map_err(|_| GenesisError::InvalidQualificationPolicy)?;
        if !is_canonical_text(&self.qualification_policy.policy_version) {
            return Err(GenesisError::InvalidQualificationPolicy);
        }
        if self.members.len() != GOVERNANCE_COMMITTEE_SIZE
            || !self
                .members
                .windows(2)
                .all(|members| members[0].member_id < members[1].member_id)
        {
            return Err(GenesisError::InvalidMembers);
        }

        let mut member_ids = BTreeSet::new();
        let mut all_keys = BTreeSet::new();
        for member in &self.members {
            if !is_canonical_text(&member.member_id) || !member_ids.insert(&member.member_id) {
                return Err(GenesisError::InvalidMembers);
            }
            for encoded in [
                &member.identity_public_key_hex,
                &member.consensus_public_key_hex,
                &member.governance_public_key_hex,
            ] {
                let key = decode_canonical_verifying_key(encoded)?;
                if !all_keys.insert(key.to_bytes()) {
                    return Err(GenesisError::InvalidMemberKey);
                }
            }
        }
        Ok(())
    }

    pub fn acceptance_signing_bytes(&self) -> Result<Vec<u8>, GenesisError> {
        self.validate()?;
        canonical_genesis_bytes(GENESIS_ACCEPTANCE_DOMAIN, self)
    }

    pub fn rules_hash(&self) -> Result<String, GenesisError> {
        #[derive(Serialize)]
        struct FrozenRules<'a> {
            rules: &'a GenesisRules,
            qualification_policy: &'a GenesisQualificationPolicy,
        }
        let bytes = canonical_genesis_bytes(
            GENESIS_RULES_DOMAIN,
            &FrozenRules {
                rules: &self.rules,
                qualification_policy: &self.qualification_policy,
            },
        )?;
        Ok(hex::encode(crate::hash::blake2b_256(&bytes)))
    }
}

impl FoundingGenesisEnvelope {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, GenesisError> {
        self.verify(None)?;
        serde_json_canonicalizer::to_vec(self)
            .map_err(|error| GenesisError::Canonicalization(error.to_string()))
    }

    pub fn verify(
        &self,
        expected_dao_id: Option<&str>,
    ) -> Result<VerifiedFoundingGenesis, GenesisError> {
        self.core.validate()?;
        if self.acceptances.len() != GOVERNANCE_COMMITTEE_SIZE
            || !self
                .acceptances
                .windows(2)
                .all(|acceptances| acceptances[0].member_id < acceptances[1].member_id)
        {
            return Err(GenesisError::InvalidAcceptances);
        }
        let signing_bytes = self.core.acceptance_signing_bytes()?;
        for (member, acceptance) in self.core.members.iter().zip(&self.acceptances) {
            if member.member_id != acceptance.member_id
                || !is_canonical_hex(&acceptance.identity_signature_hex, 64)
                || !is_canonical_hex(&acceptance.consensus_signature_hex, 64)
                || !is_canonical_hex(&acceptance.governance_signature_hex, 64)
            {
                return Err(GenesisError::InvalidAcceptances);
            }
            for (public_key, signature) in [
                (
                    &member.identity_public_key_hex,
                    &acceptance.identity_signature_hex,
                ),
                (
                    &member.consensus_public_key_hex,
                    &acceptance.consensus_signature_hex,
                ),
                (
                    &member.governance_public_key_hex,
                    &acceptance.governance_signature_hex,
                ),
            ] {
                let key = decode_canonical_verifying_key(public_key)?;
                let signature =
                    decode_signature(signature).map_err(|_| GenesisError::InvalidAcceptances)?;
                if key.verify_strict(&signing_bytes, &signature).is_err() {
                    return Err(GenesisError::InvalidAcceptances);
                }
            }
        }

        let encoded = serde_json_canonicalizer::to_vec(self)
            .map_err(|error| GenesisError::Canonicalization(error.to_string()))?;
        if encoded.len() > MAX_GOVERNANCE_GENESIS_BYTES {
            return Err(GenesisError::TooLarge);
        }
        let mut id_material = Vec::with_capacity(GENESIS_ID_DOMAIN.len() + 1 + encoded.len());
        id_material.extend_from_slice(GENESIS_ID_DOMAIN);
        id_material.push(0);
        id_material.extend_from_slice(&encoded);
        let genesis_hash = hex::encode(crate::hash::blake2b_256(&id_material));
        let dao_id = genesis_hash.clone();
        if expected_dao_id.is_some_and(|expected| expected != dao_id) {
            return Err(GenesisError::DaoIdMismatch);
        }
        let rules_hash = self.core.rules_hash()?;
        let committee = CommitteeEpoch {
            dao_id: dao_id.clone(),
            epoch: self.core.activation.initial_epoch,
            genesis_hash: genesis_hash.clone(),
            rules_hash: rules_hash.clone(),
            members: self
                .core
                .members
                .iter()
                .map(|member| CommitteeMemberKey {
                    member_id: member.member_id.clone(),
                    public_key_hex: member.governance_public_key_hex.clone(),
                })
                .collect(),
            threshold: self.core.rules.receipt_threshold,
        };
        committee
            .validate()
            .map_err(|_| GenesisError::InvalidMembers)?;
        Ok(VerifiedFoundingGenesis {
            dao_id,
            genesis_hash,
            rules_hash,
            committee,
        })
    }
}

pub fn decode_and_verify_genesis(
    bytes: &[u8],
    expected_dao_id: Option<&str>,
) -> Result<(FoundingGenesisEnvelope, VerifiedFoundingGenesis), GenesisError> {
    if bytes.len() > MAX_GOVERNANCE_GENESIS_BYTES {
        return Err(GenesisError::TooLarge);
    }
    let envelope: FoundingGenesisEnvelope =
        serde_json::from_slice(bytes).map_err(|error| GenesisError::Decode(error.to_string()))?;
    let canonical = serde_json_canonicalizer::to_vec(&envelope)
        .map_err(|error| GenesisError::Canonicalization(error.to_string()))?;
    if canonical != bytes {
        return Err(GenesisError::NonCanonicalEncoding);
    }
    let verified = envelope.verify(expected_dao_id)?;
    Ok((envelope, verified))
}

fn canonical_genesis_bytes<T: Serialize>(
    domain: &[u8],
    value: &T,
) -> Result<Vec<u8>, GenesisError> {
    let encoded = serde_json_canonicalizer::to_vec(value)
        .map_err(|error| GenesisError::Canonicalization(error.to_string()))?;
    let mut bytes = Vec::with_capacity(domain.len() + 1 + encoded.len());
    bytes.extend_from_slice(domain);
    bytes.push(0);
    bytes.extend_from_slice(&encoded);
    Ok(bytes)
}

fn decode_canonical_verifying_key(encoded: &str) -> Result<VerifyingKey, GenesisError> {
    if !is_canonical_hex(encoded, 32) {
        return Err(GenesisError::InvalidMemberKey);
    }
    decode_verifying_key(encoded).map_err(|_| GenesisError::InvalidMemberKey)
}

fn is_canonical_hex(encoded: &str, byte_length: usize) -> bool {
    encoded.len() == byte_length * 2
        && encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_canonical_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn validate_sorted_unique_text(values: &[String]) -> Result<(), ()> {
    if values.is_empty()
        || !values.iter().all(|value| is_canonical_text(value))
        || !values.windows(2).all(|values| values[0] < values[1])
    {
        return Err(());
    }
    Ok(())
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CertificateError {
    #[error("governance certificate uses unsupported protocol version {0}")]
    UnsupportedVersion(u16),
    #[error("committee epoch must contain exactly 7 independently identified members")]
    InvalidCommitteeSize,
    #[error("committee epoch must require exactly 5 signatures")]
    InvalidThreshold,
    #[error("committee member ids and public keys must both be unique")]
    DuplicateCommitteeMember,
    #[error("committee member id or public key is malformed")]
    MalformedCommitteeMember,
    #[error("certificate is not bound to the expected DAO, epoch, or rules")]
    WrongAuthorityContext,
    #[error("certificate binding contains an invalid value")]
    InvalidBinding,
    #[error("certificate does not contain 5 valid, distinct committee signatures")]
    QuorumNotReached,
    #[error("failed to encode canonical signing payload: {0}")]
    Canonicalization(String),
}

/// One independently controlled committee identity for an epoch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitteeMemberKey {
    pub member_id: String,
    pub public_key_hex: String,
}

/// Immutable authority context to which every certificate is bound.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitteeEpoch {
    pub dao_id: String,
    pub epoch: u64,
    pub genesis_hash: String,
    pub rules_hash: String,
    pub members: Vec<CommitteeMemberKey>,
    pub threshold: u8,
}

impl CommitteeEpoch {
    pub fn validate(&self) -> Result<(), CertificateError> {
        if self.members.len() != GOVERNANCE_COMMITTEE_SIZE {
            return Err(CertificateError::InvalidCommitteeSize);
        }
        if usize::from(self.threshold) != GOVERNANCE_QUORUM {
            return Err(CertificateError::InvalidThreshold);
        }
        if !is_canonical_digest(&self.dao_id)
            || self.dao_id != self.genesis_hash
            || !is_canonical_digest(&self.genesis_hash)
            || !is_canonical_digest(&self.rules_hash)
        {
            return Err(CertificateError::InvalidBinding);
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for member in &self.members {
            let key = decode_canonical_verifying_key(&member.public_key_hex)
                .map_err(|_| CertificateError::MalformedCommitteeMember)?;
            if !is_canonical_text(&member.member_id)
                || !ids.insert(member.member_id.as_str())
                || !keys.insert(key.to_bytes())
            {
                return Err(CertificateError::DuplicateCommitteeMember);
            }
        }
        if !self
            .members
            .windows(2)
            .all(|members| members[0].member_id < members[1].member_id)
        {
            return Err(CertificateError::MalformedCommitteeMember);
        }
        Ok(())
    }

    fn member_keys(&self) -> Result<BTreeMap<&str, VerifyingKey>, CertificateError> {
        self.validate()?;
        self.members
            .iter()
            .map(|member| {
                decode_canonical_verifying_key(&member.public_key_hex)
                    .map(|key| (member.member_id.as_str(), key))
                    .map_err(|_| CertificateError::MalformedCommitteeMember)
            })
            .collect()
    }
}

/// Common vote and log-position commitment signed by receipt attestors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteReceiptBinding {
    pub protocol_version: u16,
    pub dao_id: String,
    pub epoch: u64,
    pub genesis_hash: String,
    pub contest_id: String,
    pub vote_hash: String,
    pub log_root: String,
    pub log_position: u64,
    pub eligibility_evidence_hash: String,
    pub rules_hash: String,
    pub voting_cutoff: i64,
}

/// A committee member's independently timed signature over a receipt binding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiptAttestation {
    pub member_id: String,
    pub received_at: i64,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteReceiptCertificate {
    pub binding: VoteReceiptBinding,
    pub attestations: Vec<ReceiptAttestation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedVoteReceipt {
    /// Latest independently asserted receipt time among the five signatures
    /// needed for the certificate. This is deterministic metadata, not a new
    /// trusted clock.
    pub certified_at: i64,
    pub signer_ids: Vec<String>,
}

impl VoteReceiptCertificate {
    /// Verify a five-member pre-cutoff receipt against the immutable epoch.
    ///
    /// Invalid, unknown, duplicate, or post-cutoff attestations do not count.
    /// Every counted signer independently signs its own `received_at` value.
    pub fn verify(
        &self,
        committee: &CommitteeEpoch,
    ) -> Result<VerifiedVoteReceipt, CertificateError> {
        validate_receipt_binding(&self.binding, committee)?;
        let member_keys = committee.member_keys()?;
        let mut valid = BTreeMap::<&str, i64>::new();

        for attestation in &self.attestations {
            if attestation.received_at < 0 || attestation.received_at > self.binding.voting_cutoff {
                continue;
            }
            let Some(key) = member_keys.get(attestation.member_id.as_str()) else {
                continue;
            };
            let Ok(signature) = decode_signature(&attestation.signature_hex) else {
                continue;
            };
            let bytes = receipt_signing_bytes(
                &self.binding,
                &attestation.member_id,
                attestation.received_at,
            )?;
            if key.verify_strict(&bytes, &signature).is_ok() {
                valid
                    .entry(&attestation.member_id)
                    .and_modify(|time| *time = (*time).min(attestation.received_at))
                    .or_insert(attestation.received_at);
            }
        }

        let mut valid: Vec<(&str, i64)> = valid.into_iter().collect();
        if valid.len() < GOVERNANCE_QUORUM {
            return Err(CertificateError::QuorumNotReached);
        }
        valid.sort_by(|(left_id, left_time), (right_id, right_time)| {
            left_time
                .cmp(right_time)
                .then_with(|| left_id.cmp(right_id))
        });
        valid.truncate(GOVERNANCE_QUORUM);
        let certified_at = valid
            .iter()
            .map(|(_, received_at)| *received_at)
            .max()
            .ok_or(CertificateError::QuorumNotReached)?;
        Ok(VerifiedVoteReceipt {
            certified_at,
            signer_ids: valid
                .into_iter()
                .map(|(member_id, _)| member_id.to_owned())
                .collect(),
        })
    }
}

/// Closed-prefix and deterministic-result commitment signed by the committee.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloseCertificateBinding {
    pub protocol_version: u16,
    pub dao_id: String,
    pub epoch: u64,
    pub genesis_hash: String,
    pub contest_id: String,
    pub rules_hash: String,
    pub voting_cutoff: i64,
    pub log_root: String,
    pub log_length: u64,
    pub vote_set_hash: String,
    pub result_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloseAttestation {
    pub member_id: String,
    pub signed_at: i64,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloseCertificate {
    pub binding: CloseCertificateBinding,
    pub attestations: Vec<CloseAttestation>,
}

impl CloseCertificate {
    /// Verify that five distinct epoch members certified the same closed log
    /// prefix after the cutoff. Cardano anchoring is intentionally irrelevant.
    pub fn verify(&self, committee: &CommitteeEpoch) -> Result<Vec<String>, CertificateError> {
        validate_close_binding(&self.binding, committee)?;
        let member_keys = committee.member_keys()?;
        let mut valid = BTreeSet::<&str>::new();

        for attestation in &self.attestations {
            if attestation.signed_at < self.binding.voting_cutoff
                || valid.contains(attestation.member_id.as_str())
            {
                continue;
            }
            let Some(key) = member_keys.get(attestation.member_id.as_str()) else {
                continue;
            };
            let Ok(signature) = decode_signature(&attestation.signature_hex) else {
                continue;
            };
            let bytes =
                close_signing_bytes(&self.binding, &attestation.member_id, attestation.signed_at)?;
            if key.verify_strict(&bytes, &signature).is_ok() {
                valid.insert(&attestation.member_id);
            }
        }

        if valid.len() < GOVERNANCE_QUORUM {
            return Err(CertificateError::QuorumNotReached);
        }
        Ok(valid
            .into_iter()
            .take(GOVERNANCE_QUORUM)
            .map(str::to_owned)
            .collect())
    }
}

#[derive(Serialize)]
struct ReceiptSigningPayload<'a> {
    binding: &'a VoteReceiptBinding,
    member_id: &'a str,
    received_at: i64,
}

#[derive(Serialize)]
struct CloseSigningPayload<'a> {
    binding: &'a CloseCertificateBinding,
    member_id: &'a str,
    signed_at: i64,
}

pub fn receipt_signing_bytes(
    binding: &VoteReceiptBinding,
    member_id: &str,
    received_at: i64,
) -> Result<Vec<u8>, CertificateError> {
    canonical_signing_bytes(
        RECEIPT_DOMAIN,
        &ReceiptSigningPayload {
            binding,
            member_id,
            received_at,
        },
    )
}

pub fn close_signing_bytes(
    binding: &CloseCertificateBinding,
    member_id: &str,
    signed_at: i64,
) -> Result<Vec<u8>, CertificateError> {
    canonical_signing_bytes(
        CLOSE_DOMAIN,
        &CloseSigningPayload {
            binding,
            member_id,
            signed_at,
        },
    )
}

fn validate_receipt_binding(
    binding: &VoteReceiptBinding,
    committee: &CommitteeEpoch,
) -> Result<(), CertificateError> {
    validate_authority_context(
        binding.protocol_version,
        &binding.dao_id,
        binding.epoch,
        &binding.genesis_hash,
        &binding.rules_hash,
        committee,
    )?;
    if !is_canonical_text(&binding.contest_id)
        || !is_canonical_digest(&binding.vote_hash)
        || !is_canonical_digest(&binding.log_root)
        || !is_canonical_digest(&binding.eligibility_evidence_hash)
        || binding.voting_cutoff < 0
    {
        return Err(CertificateError::InvalidBinding);
    }
    Ok(())
}

fn validate_close_binding(
    binding: &CloseCertificateBinding,
    committee: &CommitteeEpoch,
) -> Result<(), CertificateError> {
    validate_authority_context(
        binding.protocol_version,
        &binding.dao_id,
        binding.epoch,
        &binding.genesis_hash,
        &binding.rules_hash,
        committee,
    )?;
    if !is_canonical_text(&binding.contest_id)
        || !is_canonical_digest(&binding.log_root)
        || !is_canonical_digest(&binding.vote_set_hash)
        || !is_canonical_digest(&binding.result_hash)
        || binding.voting_cutoff < 0
    {
        return Err(CertificateError::InvalidBinding);
    }
    Ok(())
}

fn validate_authority_context(
    protocol_version: u16,
    dao_id: &str,
    epoch: u64,
    genesis_hash: &str,
    rules_hash: &str,
    committee: &CommitteeEpoch,
) -> Result<(), CertificateError> {
    committee.validate()?;
    if protocol_version != GOVERNANCE_CERTIFICATE_VERSION {
        return Err(CertificateError::UnsupportedVersion(protocol_version));
    }
    if dao_id != committee.dao_id
        || epoch != committee.epoch
        || genesis_hash != committee.genesis_hash
        || rules_hash != committee.rules_hash
    {
        return Err(CertificateError::WrongAuthorityContext);
    }
    Ok(())
}

fn canonical_signing_bytes<T: Serialize>(
    domain: &[u8],
    payload: &T,
) -> Result<Vec<u8>, CertificateError> {
    let encoded = serde_json_canonicalizer::to_vec(payload)
        .map_err(|error| CertificateError::Canonicalization(error.to_string()))?;
    let mut bytes = Vec::with_capacity(domain.len() + 1 + encoded.len());
    bytes.extend_from_slice(domain);
    bytes.push(0);
    bytes.extend_from_slice(&encoded);
    Ok(bytes)
}

fn decode_verifying_key(encoded: &str) -> Result<VerifyingKey, ()> {
    let bytes = hex::decode(encoded).map_err(|_| ())?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| ())?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| ())
}

fn decode_signature(encoded: &str) -> Result<Signature, ()> {
    let bytes = hex::decode(encoded).map_err(|_| ())?;
    let bytes: [u8; 64] = bytes.try_into().map_err(|_| ())?;
    Ok(Signature::from_bytes(&bytes))
}

fn is_canonical_digest(encoded: &str) -> bool {
    is_canonical_hex(encoded, 32)
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn keys() -> Vec<SigningKey> {
        (1..=GOVERNANCE_COMMITTEE_SIZE)
            .map(|seed| SigningKey::from_bytes(&[seed as u8; 32]))
            .collect()
    }

    struct FounderKeys {
        identity: SigningKey,
        consensus: SigningKey,
        governance: SigningKey,
    }

    fn genesis() -> (FoundingGenesisEnvelope, Vec<FounderKeys>) {
        let founder_keys: Vec<_> = (0..GOVERNANCE_COMMITTEE_SIZE)
            .map(|index| FounderKeys {
                identity: SigningKey::from_bytes(&[20 + index as u8; 32]),
                consensus: SigningKey::from_bytes(&[40 + index as u8; 32]),
                governance: SigningKey::from_bytes(&[1 + index as u8; 32]),
            })
            .collect();
        let members = founder_keys
            .iter()
            .enumerate()
            .map(|(index, keys)| GenesisMember {
                member_id: format!("member-{index}"),
                identity_public_key_hex: hex::encode(keys.identity.verifying_key().to_bytes()),
                consensus_public_key_hex: hex::encode(keys.consensus.verifying_key().to_bytes()),
                governance_public_key_hex: hex::encode(keys.governance.verifying_key().to_bytes()),
            })
            .collect();
        let core = FoundingGenesisCore {
            genesis_version: GOVERNANCE_GENESIS_VERSION,
            protocol_version: GOVERNANCE_CERTIFICATE_VERSION,
            name: "Computing DAO".into(),
            scope: GenesisScope {
                scope_type: "subject".into(),
                scope_id: "computer-science".into(),
            },
            members,
            rules: GenesisRules {
                rules_version: "1".into(),
                committee_size: GOVERNANCE_COMMITTEE_SIZE as u8,
                receipt_threshold: GOVERNANCE_QUORUM as u8,
                outcome_threshold: GOVERNANCE_QUORUM as u8,
                proposal_approval_numerator: 2,
                proposal_approval_denominator: 3,
                minimum_turnout_count: 25,
            },
            qualification_policy: GenesisQualificationPolicy {
                policy_version: "1".into(),
                accepted_issuers: vec!["did:key:issuer-a".into(), "did:key:issuer-b".into()],
                accepted_assessment_evidence: vec![
                    "assessment-credential".into(),
                    "instructor-endorsement".into(),
                ],
            },
            activation: GenesisActivation {
                cometbft_chain_id: "alexandria-computing-1".into(),
                initial_epoch: 0,
                initial_height: 1,
                activation_time_unix: 1_800_000_000,
            },
        };
        let signing_bytes = core.acceptance_signing_bytes().unwrap();
        let acceptances = founder_keys
            .iter()
            .enumerate()
            .map(|(index, keys)| FoundingAcceptance {
                member_id: format!("member-{index}"),
                identity_signature_hex: hex::encode(keys.identity.sign(&signing_bytes).to_bytes()),
                consensus_signature_hex: hex::encode(
                    keys.consensus.sign(&signing_bytes).to_bytes(),
                ),
                governance_signature_hex: hex::encode(
                    keys.governance.sign(&signing_bytes).to_bytes(),
                ),
            })
            .collect();
        (FoundingGenesisEnvelope { core, acceptances }, founder_keys)
    }

    #[test]
    fn fully_accepted_canonical_genesis_derives_the_initial_authority() {
        let (genesis, _) = genesis();
        let verified = genesis.verify(None).unwrap();
        assert_eq!(verified.dao_id, verified.genesis_hash);
        assert_eq!(verified.dao_id.len(), 64);
        assert_eq!(verified.committee.dao_id, verified.dao_id);
        assert_eq!(verified.committee.epoch, 0);
        assert_eq!(verified.committee.threshold, GOVERNANCE_QUORUM as u8);
        assert_eq!(verified.committee.rules_hash, verified.rules_hash);

        let canonical = genesis.canonical_bytes().unwrap();
        let (decoded, decoded_verification) =
            decode_and_verify_genesis(&canonical, Some(&verified.dao_id)).unwrap();
        assert_eq!(decoded, genesis);
        assert_eq!(decoded_verification, verified);
    }

    #[test]
    fn five_founders_cannot_activate_a_nominal_seven_member_genesis() {
        let (mut genesis, _) = genesis();
        genesis.acceptances.truncate(GOVERNANCE_QUORUM);
        assert_eq!(genesis.verify(None), Err(GenesisError::InvalidAcceptances));
    }

    #[test]
    fn reordered_or_duplicate_genesis_members_and_acceptances_are_rejected() {
        let (mut reordered_members, _) = genesis();
        reordered_members.core.members.swap(0, 1);
        assert_eq!(
            reordered_members.verify(None),
            Err(GenesisError::InvalidMembers)
        );

        let (mut duplicate_key, _) = genesis();
        duplicate_key.core.members[1].consensus_public_key_hex = duplicate_key.core.members[0]
            .identity_public_key_hex
            .clone();
        assert_eq!(
            duplicate_key.verify(None),
            Err(GenesisError::InvalidMemberKey)
        );

        let (mut reordered_acceptances, _) = genesis();
        reordered_acceptances.acceptances.swap(0, 1);
        assert_eq!(
            reordered_acceptances.verify(None),
            Err(GenesisError::InvalidAcceptances)
        );
    }

    #[test]
    fn every_founder_must_prove_control_of_all_three_declared_keys() {
        let (genesis, _) = genesis();
        for field in 0..3 {
            let mut altered = genesis.clone();
            let replacement = match field {
                0 => altered.acceptances[1].identity_signature_hex.clone(),
                1 => altered.acceptances[1].consensus_signature_hex.clone(),
                _ => altered.acceptances[1].governance_signature_hex.clone(),
            };
            match field {
                0 => altered.acceptances[0].identity_signature_hex = replacement,
                1 => altered.acceptances[0].consensus_signature_hex = replacement,
                _ => altered.acceptances[0].governance_signature_hex = replacement,
            }
            assert_eq!(altered.verify(None), Err(GenesisError::InvalidAcceptances));
        }
    }

    #[test]
    fn oversized_constructed_and_encoded_genesis_are_rejected_before_use() {
        let (mut genesis, _) = genesis();
        genesis.core.qualification_policy.accepted_issuers =
            vec!["x".repeat(MAX_GOVERNANCE_GENESIS_BYTES)];
        assert_eq!(genesis.verify(None), Err(GenesisError::TooLarge));

        let oversized = vec![b' '; MAX_GOVERNANCE_GENESIS_BYTES + 1];
        assert_eq!(
            decode_and_verify_genesis(&oversized, None),
            Err(GenesisError::TooLarge)
        );
    }

    #[test]
    fn altered_genesis_terms_invalidate_every_existing_acceptance() {
        let (mut changed_rules, _) = genesis();
        changed_rules.core.rules.minimum_turnout_count += 1;
        assert_eq!(
            changed_rules.verify(None),
            Err(GenesisError::InvalidAcceptances)
        );

        let (mut changed_policy, _) = genesis();
        changed_policy
            .core
            .qualification_policy
            .accepted_issuers
            .push("did:key:issuer-c".into());
        assert_eq!(
            changed_policy.verify(None),
            Err(GenesisError::InvalidAcceptances)
        );
    }

    #[test]
    fn claimed_identity_and_noncanonical_transport_fail_closed() {
        let (genesis, _) = genesis();
        let verified = genesis.verify(None).unwrap();
        assert_eq!(
            genesis.verify(Some(&digest(99))),
            Err(GenesisError::DaoIdMismatch)
        );

        let pretty = serde_json::to_vec_pretty(&genesis).unwrap();
        assert_eq!(
            decode_and_verify_genesis(&pretty, Some(&verified.dao_id)),
            Err(GenesisError::NonCanonicalEncoding)
        );
    }

    #[test]
    fn competing_fully_signed_genesis_documents_are_distinct_daos() {
        let (first, _) = genesis();
        let (mut second, keys) = genesis();
        second.core.scope.scope_id = "data-science".into();
        let bytes = second.core.acceptance_signing_bytes().unwrap();
        for (acceptance, keys) in second.acceptances.iter_mut().zip(keys) {
            acceptance.identity_signature_hex = hex::encode(keys.identity.sign(&bytes).to_bytes());
            acceptance.consensus_signature_hex =
                hex::encode(keys.consensus.sign(&bytes).to_bytes());
            acceptance.governance_signature_hex =
                hex::encode(keys.governance.sign(&bytes).to_bytes());
        }
        assert_ne!(
            first.verify(None).unwrap().dao_id,
            second.verify(None).unwrap().dao_id
        );
    }

    fn committee(keys: &[SigningKey]) -> CommitteeEpoch {
        CommitteeEpoch {
            dao_id: digest(1),
            epoch: 1,
            genesis_hash: digest(1),
            rules_hash: digest(2),
            members: keys
                .iter()
                .enumerate()
                .map(|(index, key)| CommitteeMemberKey {
                    member_id: format!("member-{index}"),
                    public_key_hex: hex::encode(key.verifying_key().to_bytes()),
                })
                .collect(),
            threshold: GOVERNANCE_QUORUM as u8,
        }
    }

    fn receipt_binding() -> VoteReceiptBinding {
        VoteReceiptBinding {
            protocol_version: GOVERNANCE_CERTIFICATE_VERSION,
            dao_id: digest(1),
            epoch: 1,
            genesis_hash: digest(1),
            contest_id: "proposal-1".into(),
            vote_hash: digest(3),
            log_root: digest(4),
            log_position: 12,
            eligibility_evidence_hash: digest(5),
            rules_hash: digest(2),
            voting_cutoff: 2_000,
        }
    }

    fn signed_receipt(keys: &[SigningKey], count: usize) -> VoteReceiptCertificate {
        let binding = receipt_binding();
        let attestations = keys
            .iter()
            .take(count)
            .enumerate()
            .map(|(index, key)| {
                let member_id = format!("member-{index}");
                let received_at = 1_900 + index as i64;
                let bytes = receipt_signing_bytes(&binding, &member_id, received_at).unwrap();
                ReceiptAttestation {
                    member_id,
                    received_at,
                    signature_hex: hex::encode(key.sign(&bytes).to_bytes()),
                }
            })
            .collect();
        VoteReceiptCertificate {
            binding,
            attestations,
        }
    }

    fn close_binding() -> CloseCertificateBinding {
        CloseCertificateBinding {
            protocol_version: GOVERNANCE_CERTIFICATE_VERSION,
            dao_id: digest(1),
            epoch: 1,
            genesis_hash: digest(1),
            contest_id: "proposal-1".into(),
            rules_hash: digest(2),
            voting_cutoff: 2_000,
            log_root: digest(6),
            log_length: 42,
            vote_set_hash: digest(7),
            result_hash: digest(8),
        }
    }

    fn signed_close(keys: &[SigningKey], count: usize) -> CloseCertificate {
        let binding = close_binding();
        let attestations = keys
            .iter()
            .take(count)
            .enumerate()
            .map(|(index, key)| {
                let member_id = format!("member-{index}");
                let signed_at = 2_000 + index as i64;
                let bytes = close_signing_bytes(&binding, &member_id, signed_at).unwrap();
                CloseAttestation {
                    member_id,
                    signed_at,
                    signature_hex: hex::encode(key.sign(&bytes).to_bytes()),
                }
            })
            .collect();
        CloseCertificate {
            binding,
            attestations,
        }
    }

    #[test]
    fn receipt_requires_five_distinct_valid_pre_cutoff_signers() {
        let keys = keys();
        let committee = committee(&keys);
        let verified = signed_receipt(&keys, 5).verify(&committee).unwrap();
        assert_eq!(verified.signer_ids.len(), GOVERNANCE_QUORUM);
        assert_eq!(verified.certified_at, 1_904);
        assert_eq!(
            signed_receipt(&keys, 4).verify(&committee),
            Err(CertificateError::QuorumNotReached)
        );
    }

    #[test]
    fn duplicate_and_post_cutoff_receipt_attestations_do_not_count() {
        let keys = keys();
        let committee = committee(&keys);
        let mut receipt = signed_receipt(&keys, 5);
        receipt.attestations.push(receipt.attestations[0].clone());
        receipt.attestations[4].received_at = 2_001;
        assert_eq!(
            receipt.verify(&committee),
            Err(CertificateError::QuorumNotReached)
        );
    }

    #[test]
    fn certified_time_is_the_deterministic_fifth_earliest_valid_attestation() {
        let keys = keys();
        let committee = committee(&keys);
        let mut receipt = signed_receipt(&keys, 7);
        receipt.attestations.reverse();
        let verified = receipt.verify(&committee).unwrap();
        assert_eq!(verified.certified_at, 1_904);
        assert_eq!(verified.signer_ids.len(), GOVERNANCE_QUORUM);
    }

    #[test]
    fn receipt_tampering_and_wrong_epoch_fail_closed() {
        let keys = keys();
        let committee = committee(&keys);
        let mut receipt = signed_receipt(&keys, 5);
        receipt.binding.vote_hash = digest(9);
        assert_eq!(
            receipt.verify(&committee),
            Err(CertificateError::QuorumNotReached)
        );

        let mut receipt = signed_receipt(&keys, 5);
        receipt.binding.epoch = 2;
        assert_eq!(
            receipt.verify(&committee),
            Err(CertificateError::WrongAuthorityContext)
        );
    }

    #[test]
    fn close_certificate_requires_five_post_cutoff_signers() {
        let keys = keys();
        let committee = committee(&keys);
        assert_eq!(
            signed_close(&keys, 5).verify(&committee).unwrap().len(),
            GOVERNANCE_QUORUM
        );

        let mut close = signed_close(&keys, 5);
        close.attestations[4].signed_at = 1_999;
        assert_eq!(
            close.verify(&committee),
            Err(CertificateError::QuorumNotReached)
        );
    }

    #[test]
    fn committee_rejects_duplicate_identity_or_control_key() {
        let keys = keys();
        let mut duplicate_id = committee(&keys);
        duplicate_id.members[1].member_id = duplicate_id.members[0].member_id.clone();
        assert_eq!(
            duplicate_id.validate(),
            Err(CertificateError::DuplicateCommitteeMember)
        );

        let mut duplicate_key = committee(&keys);
        duplicate_key.members[1].public_key_hex = duplicate_key.members[0].public_key_hex.clone();
        assert_eq!(
            duplicate_key.validate(),
            Err(CertificateError::DuplicateCommitteeMember)
        );
    }

    #[test]
    fn committee_context_is_genesis_derived_and_canonical() {
        let keys = keys();

        let mut wrong_dao = committee(&keys);
        wrong_dao.dao_id = digest(9);
        assert_eq!(wrong_dao.validate(), Err(CertificateError::InvalidBinding));

        let mut uppercase_digest = committee(&keys);
        uppercase_digest.rules_hash = digest(0xab).to_ascii_uppercase();
        assert_eq!(
            uppercase_digest.validate(),
            Err(CertificateError::InvalidBinding)
        );

        let mut reordered = committee(&keys);
        reordered.members.swap(0, 1);
        assert_eq!(
            reordered.validate(),
            Err(CertificateError::MalformedCommitteeMember)
        );

        let mut uppercase_key = committee(&keys);
        uppercase_key.members[0]
            .public_key_hex
            .make_ascii_uppercase();
        assert_eq!(
            uppercase_key.validate(),
            Err(CertificateError::MalformedCommitteeMember)
        );
    }

    #[test]
    fn malformed_commitments_fail_before_signature_counting() {
        let keys = keys();
        let committee = committee(&keys);
        let mut receipt = signed_receipt(&keys, 5);
        receipt.binding.log_root = "not-a-blake2b-256-digest".into();
        assert_eq!(
            receipt.verify(&committee),
            Err(CertificateError::InvalidBinding)
        );
    }

    #[test]
    fn certificate_signing_is_domain_separated() {
        let receipt = receipt_signing_bytes(&receipt_binding(), "member-0", 1_900).unwrap();
        let close = close_signing_bytes(&close_binding(), "member-0", 2_000).unwrap();
        assert!(receipt.starts_with(RECEIPT_DOMAIN));
        assert!(close.starts_with(CLOSE_DOMAIN));
        assert_ne!(receipt, close);
    }

    fn digest(byte: u8) -> String {
        hex::encode([byte; 32])
    }
}
