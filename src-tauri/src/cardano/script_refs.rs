//! Deployed validator script hashes and reference UTxO locations.
//!
//! Script hashes are computed from the Aiken-compiled UPLC bytecode in
//! `cardano/governance/plutus.json` (PlutusV3: blake2b-224 of 0x03 || compiled_code).
//!
//! Reference UTxO locations must be populated after the one-time deployment
//! of each validator as a reference script on preprod. Until deployed, the
//! `ref_utxos_deployed()` function returns false and governance tx builders
//! will queue actions without on-chain submission.

// ---- Script Hashes (computed from plutus.json) ----

/// Script hash for the DAO registry spending validator.
pub const DAO_REGISTRY_SCRIPT_HASH: &str =
    "2c825112bd560c62c0f6afcc463bf020571f5c53495d4e212c523a12";

/// Script hash for the DAO state token minting policy.
pub const DAO_MINTING_SCRIPT_HASH: &str =
    "4e1e127580699d82a90a919b0b9c875e9f4cacc3fef93ef3a6ed4594";

/// Script hash for the election spending validator.
pub const ELECTION_SCRIPT_HASH: &str = "b292f0e842766af40a800d0c53cbe7a7f9faab7b85f68802e6468d25";

/// Script hash for the proposal spending validator.
pub const PROPOSAL_SCRIPT_HASH: &str = "7888035da181d26498ebe1b6fbe4c515007155e120e30fc3fcdf2c0d";

/// Script hash for the vote receipt minting policy.
pub const VOTE_MINTING_SCRIPT_HASH: &str =
    "ad8badb5c65d7307c0977bfebbdd1a8389cf39d1314498de66d60701";

/// Script hash for the CIP-68 reputation minting policy.
pub const REPUTATION_MINTING_SCRIPT_HASH: &str =
    "315a6b7ca4d2df46af9956a24a3f31b7b9670c5115c76642776ac88f";

/// Script hash for the soulbound spending validator.
pub const SOULBOUND_SCRIPT_HASH: &str = "2700722e5fb56941388a7813f416a0d1e76ee251dbb3ea248d41890a";

/// Script hash for the completion-witness minting policy.
///
/// Compiled from `cardano/governance/validators/completion.ak` and
/// used as the policy id of `completion_minting`. Completion tokens
/// carry asset names of shape `learner_pkh (28) || course_tag (4)`.
pub const COMPLETION_MINTING_SCRIPT_HASH: &str =
    "6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0";

// ---- Reference UTxO Locations (populated after deployment) ----
// These are the UTxOs where each validator's compiled script is stored
// as a reference script (CIP-33). Transactions reference these instead
// of including the full ~14KB script inline.

/// Reference UTxO for the DAO registry validator (tx_hash, output_index).
pub const DAO_REGISTRY_REF_UTXO: (&str, u64) = ("DEPLOY_PENDING", 0);

/// Reference UTxO for the DAO minting policy.
pub const DAO_MINTING_REF_UTXO: (&str, u64) = ("DEPLOY_PENDING", 0);

/// Reference UTxO for the election validator.
pub const ELECTION_REF_UTXO: (&str, u64) = ("DEPLOY_PENDING", 0);

/// Reference UTxO for the proposal validator.
pub const PROPOSAL_REF_UTXO: (&str, u64) = ("DEPLOY_PENDING", 0);

/// Reference UTxO for the vote receipt minting policy.
pub const VOTE_MINTING_REF_UTXO: (&str, u64) = ("DEPLOY_PENDING", 0);

/// Reference UTxO for the reputation minting policy.
pub const REPUTATION_MINTING_REF_UTXO: (&str, u64) = ("DEPLOY_PENDING", 0);

/// Reference UTxO for the soulbound validator.
pub const SOULBOUND_REF_UTXO: (&str, u64) = ("DEPLOY_PENDING", 0);

/// Reference UTxO for the completion-witness minting policy.
pub const COMPLETION_MINTING_REF_UTXO: (&str, u64) = (
    "cb763b8336d0a0f2abba52bcc43e347e1fc2972c8ca7aabb871090358e0e6eea",
    0,
);

// ---- Utility ----

/// Governance metadata label (CIP-68 / custom).
pub const GOVERNANCE_METADATA_LABEL: u64 = 1694;

/// Custom metadata label for Alexandria credential integrity anchors
/// (spec §12.3). Sits one past `GOVERNANCE_METADATA_LABEL` to keep
/// Alexandria-internal labels grouped, and well clear of CIP-25 (721)
/// and CIP-68 reference / user-token labels (100, 222, 333, 444).
/// No registered CIP-X label exists for this purpose at the time of
/// writing — verifiers MUST look for label 1697 explicitly.
pub const ALEXANDRIA_ANCHOR_LABEL: u64 = 1697;

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

/// Check if the governance bundle reference UTxOs have been deployed.
/// Returns false while reference scripts are still pending deployment.
pub fn ref_utxos_deployed() -> bool {
    DAO_REGISTRY_REF_UTXO.0 != "DEPLOY_PENDING"
}

/// Check if the completion-witness validator has been deployed as a
/// reference script. Independent from the governance bundle so the
/// VC-first completion flow can run before the rest of the validators
/// land on-chain.
pub fn completion_ref_deployed() -> bool {
    COMPLETION_MINTING_REF_UTXO.0 != "DEPLOY_PENDING"
}
