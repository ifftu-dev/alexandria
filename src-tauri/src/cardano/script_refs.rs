//! Deployed validator script hashes and reference UTxO locations.
//!
//! After deploying the Aiken validators from `cardano/governance/plutus.json`
//! as reference scripts on preprod, populate these constants with the actual
//! script hashes and UTxO locations.
//!
//! Reference scripts (CIP-33) allow transactions to reference the validator
//! code without including it inline, saving ~14KB per transaction.

/// Script hash for the DAO registry spending validator.
pub const DAO_REGISTRY_SCRIPT_HASH: &str = "TODO_DEPLOY_AND_SET";

/// Script hash for the DAO state token minting policy.
pub const DAO_MINTING_SCRIPT_HASH: &str = "TODO_DEPLOY_AND_SET";

/// Script hash for the election spending validator.
pub const ELECTION_SCRIPT_HASH: &str = "TODO_DEPLOY_AND_SET";

/// Script hash for the proposal spending validator.
pub const PROPOSAL_SCRIPT_HASH: &str = "TODO_DEPLOY_AND_SET";

/// Script hash for the vote receipt minting policy.
pub const VOTE_MINTING_SCRIPT_HASH: &str = "TODO_DEPLOY_AND_SET";

/// Script hash for the CIP-68 reputation minting policy.
pub const REPUTATION_MINTING_SCRIPT_HASH: &str = "TODO_DEPLOY_AND_SET";

/// Script hash for the soulbound spending validator.
pub const SOULBOUND_SCRIPT_HASH: &str = "TODO_DEPLOY_AND_SET";

/// Reference UTxO for the DAO registry validator (tx_hash, output_index).
pub const DAO_REGISTRY_REF_UTXO: (&str, u64) = ("TODO_DEPLOY_AND_SET", 0);

/// Reference UTxO for the election validator.
pub const ELECTION_REF_UTXO: (&str, u64) = ("TODO_DEPLOY_AND_SET", 0);

/// Reference UTxO for the proposal validator.
pub const PROPOSAL_REF_UTXO: (&str, u64) = ("TODO_DEPLOY_AND_SET", 0);

/// Governance metadata label (CIP-68 / custom).
pub const GOVERNANCE_METADATA_LABEL: u64 = 1694;

/// Preprod shelley epoch start for slot conversion.
/// Slot = (posix_seconds - SHELLEY_EPOCH_START_POSIX)
pub const PREPROD_SHELLEY_EPOCH_START: i64 = 1_654_041_600;

/// Convert POSIX milliseconds to a Cardano slot number (preprod).
pub fn posix_ms_to_slot(posix_ms: i64) -> u64 {
    let posix_s = posix_ms / 1000;
    if posix_s > PREPROD_SHELLEY_EPOCH_START {
        (posix_s - PREPROD_SHELLEY_EPOCH_START) as u64
    } else {
        0
    }
}

/// Convert a Cardano slot number to POSIX milliseconds (preprod).
pub fn slot_to_posix_ms(slot: u64) -> i64 {
    (slot as i64 + PREPROD_SHELLEY_EPOCH_START) * 1000
}
