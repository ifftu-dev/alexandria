//! Cardano governance transaction builders.
//!
//! Builds Conway-era transactions for on-chain governance operations:
//!   - DAO registration (mint DAO state token + registry UTxO)
//!   - Election lifecycle (open, vote, finalize)
//!   - Proposal lifecycle (submit, vote, resolve)
//!   - Vote receipt minting (double-vote prevention)
//!
//! These transactions interact with the 7 Aiken/Plutus v3 validators
//! deployed via the v1 Cardano governance contracts. In v2, we build
//! transactions locally with pallas-txbuilder rather than via gRPC.

use pallas_codec::minicbor;
use pallas_codec::utils::KeyValuePairs;
use pallas_primitives::{Metadatum, MetadatumLabel};

use super::tx_builder::TxBuildError;

/// CIP-68 reference token label (100).
const _CIP68_REFERENCE_LABEL: u64 = 100;

/// CIP-68 user token label (222).
const _CIP68_USER_LABEL: u64 = 222;

/// Governance metadata label (matching v1 convention).
const GOVERNANCE_LABEL: MetadatumLabel = 1694;

/// Build CIP-25 metadata for a DAO registration transaction.
///
/// Records the DAO's scope, committee, and governance parameters on-chain.
pub fn build_dao_metadata(
    dao_id: &str,
    name: &str,
    scope_type: &str,
    scope_id: &str,
    committee: &[String],
    committee_size: i64,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let committee_list: Vec<Metadatum> = committee
        .iter()
        .map(|addr| Metadatum::Text(addr.clone()))
        .collect();

    let fields = vec![
        (
            Metadatum::Text("type".into()),
            Metadatum::Text("dao_registration".into()),
        ),
        (
            Metadatum::Text("dao_id".into()),
            Metadatum::Text(dao_id.into()),
        ),
        (Metadatum::Text("name".into()), Metadatum::Text(name.into())),
        (
            Metadatum::Text("scope_type".into()),
            Metadatum::Text(scope_type.into()),
        ),
        (
            Metadatum::Text("scope_id".into()),
            Metadatum::Text(scope_id.into()),
        ),
        (
            Metadatum::Text("committee".into()),
            Metadatum::Array(committee_list),
        ),
        (
            Metadatum::Text("committee_size".into()),
            Metadatum::Int(committee_size.into()),
        ),
    ];

    KeyValuePairs::from(vec![(
        GOVERNANCE_LABEL,
        Metadatum::Map(KeyValuePairs::from(fields)),
    )])
}

/// Build metadata for an election lifecycle transaction.
///
/// Records the election phase transition and relevant data on-chain.
pub fn build_election_metadata(
    election_id: &str,
    dao_id: &str,
    action: &str,
    phase: &str,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let fields = vec![
        (
            Metadatum::Text("type".into()),
            Metadatum::Text("election".into()),
        ),
        (
            Metadatum::Text("election_id".into()),
            Metadatum::Text(election_id.into()),
        ),
        (
            Metadatum::Text("dao_id".into()),
            Metadatum::Text(dao_id.into()),
        ),
        (
            Metadatum::Text("action".into()),
            Metadatum::Text(action.into()),
        ),
        (
            Metadatum::Text("phase".into()),
            Metadatum::Text(phase.into()),
        ),
    ];

    KeyValuePairs::from(vec![(
        GOVERNANCE_LABEL,
        Metadatum::Map(KeyValuePairs::from(fields)),
    )])
}

/// Build metadata for a proposal lifecycle transaction.
///
/// Records the proposal action (submit, approve, vote, resolve) on-chain.
pub fn build_proposal_metadata(
    proposal_id: &str,
    dao_id: &str,
    action: &str,
    status: &str,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let fields = vec![
        (
            Metadatum::Text("type".into()),
            Metadatum::Text("proposal".into()),
        ),
        (
            Metadatum::Text("proposal_id".into()),
            Metadatum::Text(proposal_id.into()),
        ),
        (
            Metadatum::Text("dao_id".into()),
            Metadatum::Text(dao_id.into()),
        ),
        (
            Metadatum::Text("action".into()),
            Metadatum::Text(action.into()),
        ),
        (
            Metadatum::Text("status".into()),
            Metadatum::Text(status.into()),
        ),
    ];

    KeyValuePairs::from(vec![(
        GOVERNANCE_LABEL,
        Metadatum::Map(KeyValuePairs::from(fields)),
    )])
}

/// Build metadata for a vote transaction.
///
/// Records the vote on-chain (for both election and proposal votes).
pub fn build_vote_metadata(
    target_type: &str,
    target_id: &str,
    voter: &str,
    choice: &str,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let fields = vec![
        (
            Metadatum::Text("type".into()),
            Metadatum::Text("vote".into()),
        ),
        (
            Metadatum::Text("target_type".into()),
            Metadatum::Text(target_type.into()),
        ),
        (
            Metadatum::Text("target_id".into()),
            Metadatum::Text(target_id.into()),
        ),
        (
            Metadatum::Text("voter".into()),
            Metadatum::Text(voter.into()),
        ),
        (
            Metadatum::Text("choice".into()),
            Metadatum::Text(choice.into()),
        ),
    ];

    KeyValuePairs::from(vec![(
        GOVERNANCE_LABEL,
        Metadatum::Map(KeyValuePairs::from(fields)),
    )])
}

/// Serialize governance metadata to CBOR bytes (for inclusion in transactions).
///
/// Uses the same injection pattern as the existing tx_builder: build the
/// metadata, encode to CBOR, then inject into the transaction's auxiliary data.
pub fn metadata_to_cbor(
    metadata: &KeyValuePairs<MetadatumLabel, Metadatum>,
) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    minicbor::encode(metadata, &mut buf).map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(buf)
}

/// Build a vote receipt asset name for double-vote prevention.
///
/// Format: voter_key_hash(28 bytes) + target_type(1 byte) + target_id(4 bytes)
/// This matches the v1 Cardano vote_minting validator's expected asset name.
///
/// target_type: 0x01 = election, 0x02 = proposal
pub fn build_vote_receipt_name(
    voter_key_hash_hex: &str,
    target_type: u8,
    target_id: &str,
) -> Result<Vec<u8>, TxBuildError> {
    let key_hash = hex::decode(voter_key_hash_hex)
        .map_err(|e| TxBuildError::Builder(format!("invalid voter key hash hex: {e}")))?;

    if key_hash.len() != 28 {
        return Err(TxBuildError::Builder(format!(
            "voter key hash must be 28 bytes, got {}",
            key_hash.len()
        )));
    }

    // Take first 4 bytes of target_id hash for compactness
    let target_bytes = &crate::crypto::hash::blake2b_256(target_id.as_bytes())[..4];

    let mut asset_name = Vec::with_capacity(33);
    asset_name.extend_from_slice(&key_hash);
    asset_name.push(target_type);
    asset_name.extend_from_slice(target_bytes);

    Ok(asset_name)
}

/// Vote receipt target type constants.
pub const VOTE_TARGET_ELECTION: u8 = 0x01;
pub const VOTE_TARGET_PROPOSAL: u8 = 0x02;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dao_metadata_structure() {
        let meta = build_dao_metadata(
            "dao_123",
            "CS DAO",
            "subject_field",
            "sf_cs",
            &["stake_addr1".to_string(), "stake_addr2".to_string()],
            5,
        );
        // Should have exactly 1 entry under GOVERNANCE_LABEL
        assert_eq!(meta.len(), 1);
        let (label, _) = &meta[0];
        assert_eq!(*label, GOVERNANCE_LABEL);
    }

    #[test]
    fn election_metadata_structure() {
        let meta = build_election_metadata("elec_1", "dao_1", "open", "nomination");
        assert_eq!(meta.len(), 1);
        let (label, _) = &meta[0];
        assert_eq!(*label, GOVERNANCE_LABEL);
    }

    #[test]
    fn proposal_metadata_structure() {
        let meta = build_proposal_metadata("prop_1", "dao_1", "submit", "draft");
        assert_eq!(meta.len(), 1);
        let (label, _) = &meta[0];
        assert_eq!(*label, GOVERNANCE_LABEL);
    }

    #[test]
    fn vote_metadata_structure() {
        let meta = build_vote_metadata("election", "elec_1", "stake_test1u123", "nominee_1");
        assert_eq!(meta.len(), 1);
    }

    #[test]
    fn vote_receipt_name_format() {
        // 28-byte key hash (56 hex chars)
        let key_hash = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef01";
        let name = build_vote_receipt_name(key_hash, VOTE_TARGET_ELECTION, "elec_123").unwrap();

        // 28 (key hash) + 1 (type) + 4 (target id hash prefix) = 33 bytes
        assert_eq!(name.len(), 33);
        assert_eq!(name[28], VOTE_TARGET_ELECTION);
    }

    #[test]
    fn vote_receipt_rejects_invalid_key_hash() {
        let result = build_vote_receipt_name("abc123", VOTE_TARGET_PROPOSAL, "prop_1");
        assert!(result.is_err());
    }

    #[test]
    fn metadata_serializes_to_cbor() {
        let meta = build_dao_metadata("dao_1", "Test", "subject_field", "sf1", &[], 3);
        let cbor = metadata_to_cbor(&meta).unwrap();
        assert!(!cbor.is_empty());
    }
}
