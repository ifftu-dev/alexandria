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
    "06b2872e2d4afc1b1a5c78f314923c23550ceddb94d7eb13ae268840";

/// Script hash for the DAO state token minting policy.
pub const DAO_MINTING_SCRIPT_HASH: &str =
    "e275a9e8418282f84e5baa39aa57627e72d5f25550780c4cb28c4db8";

/// Script hash for the election spending validator.
pub const ELECTION_SCRIPT_HASH: &str = "b292f0e842766af40a800d0c53cbe7a7f9faab7b85f68802e6468d25";

/// Script hash for the proposal spending validator.
pub const PROPOSAL_SCRIPT_HASH: &str = "7888035da181d26498ebe1b6fbe4c515007155e120e30fc3fcdf2c0d";

/// Script hash for the vote receipt minting policy.
pub const VOTE_MINTING_SCRIPT_HASH: &str =
    "4ab759540562e73715a879a4bef188da5b0ef216592a0f04f8c50ba6";

/// Script hash for the CIP-68 reputation minting policy.
pub const REPUTATION_MINTING_SCRIPT_HASH: &str =
    "9499c3f500ef8c98b667b579c7fbb4546868f9909c19165e7a4ab155";

/// Script hash for the soulbound spending validator.
pub const SOULBOUND_SCRIPT_HASH: &str = "9c823cf7b9d72f459ef68b7091606992654dad5098c74a3558b96bee";

/// Script hash for the completion-witness minting policy.
///
/// Compiled from `cardano/governance/validators/completion.ak` and
/// used as the policy id of `completion_minting`. Completion tokens
/// carry asset names of shape `learner_pkh (28) || course_tag (4)`.
pub const COMPLETION_MINTING_SCRIPT_HASH: &str =
    "6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0";

/// Script hash for the challenge-stake escrow spending validator.
///
/// Compiled from `cardano/governance/validators/challenge_escrow.ak`.
/// A challenger locks their stake at this script address; the DAO
/// authority later settles it to the challenger (`Refund`, challenge
/// upheld) or the DAO treasury (`Forfeit`, challenge rejected).
pub const CHALLENGE_ESCROW_SCRIPT_HASH: &str =
    "ead373d24790d337c0d94324988b11f760563ec3f09ff1ef48d1e519";

// ---- Reference UTxO Locations (populated after deployment) ----
// These are the UTxOs where each validator's compiled script is stored
// as a reference script (CIP-33). Transactions reference these instead
// of including the full ~14KB script inline.

// Deployed to preprod 2026-05-22 (block 4736927) via
// `cardano/governance/deploy_blockfrost.py`. Batch A tx
// 448db85c…974551 carries proposal/election/dao_registry; batch B tx
// 12daa5f2…1cdae0 carries the remaining six. Each output's
// reference_script_hash was verified to match the SCRIPT_HASH above.

/// Reference UTxO for the DAO registry validator (tx_hash, output_index).
pub const DAO_REGISTRY_REF_UTXO: (&str, u64) = (
    "bcc9ea10ab2e5fd23ca7d94a3cd16c275e03e565d9c375381bfec440770f1194",
    0,
);

/// Reference UTxO for the DAO minting policy.
pub const DAO_MINTING_REF_UTXO: (&str, u64) = (
    "bcc9ea10ab2e5fd23ca7d94a3cd16c275e03e565d9c375381bfec440770f1194",
    1,
);

/// Reference UTxO for the election validator.
pub const ELECTION_REF_UTXO: (&str, u64) = (
    "448db85c1fa30e3159ad2aad341a84fb34f71c7966cd1a6392fb186c7c974551",
    1,
);

/// Reference UTxO for the proposal validator.
pub const PROPOSAL_REF_UTXO: (&str, u64) = (
    "448db85c1fa30e3159ad2aad341a84fb34f71c7966cd1a6392fb186c7c974551",
    0,
);

/// Reference UTxO for the vote receipt minting policy.
pub const VOTE_MINTING_REF_UTXO: (&str, u64) = (
    "bcc9ea10ab2e5fd23ca7d94a3cd16c275e03e565d9c375381bfec440770f1194",
    4,
);

/// Reference UTxO for the reputation minting policy.
pub const REPUTATION_MINTING_REF_UTXO: (&str, u64) = (
    "bcc9ea10ab2e5fd23ca7d94a3cd16c275e03e565d9c375381bfec440770f1194",
    3,
);

/// Reference UTxO for the soulbound validator.
pub const SOULBOUND_REF_UTXO: (&str, u64) = (
    "bcc9ea10ab2e5fd23ca7d94a3cd16c275e03e565d9c375381bfec440770f1194",
    2,
);

/// Reference UTxO for the completion-witness minting policy.
pub const COMPLETION_MINTING_REF_UTXO: (&str, u64) = (
    "12daa5f20a61f768a4d8c436e3a693b338ff275fde73149b1832faa4b61cdae0",
    1,
);

/// Reference UTxO for the challenge-escrow spending validator.
pub const CHALLENGE_ESCROW_REF_UTXO: (&str, u64) = (
    "12daa5f20a61f768a4d8c436e3a693b338ff275fde73149b1832faa4b61cdae0",
    5,
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

/// Check if the challenge-escrow validator has been deployed as a
/// reference script. The lock tx (paying the script) works without a
/// reference script, but settlement (spending the script) needs it.
pub fn challenge_escrow_deployed() -> bool {
    CHALLENGE_ESCROW_REF_UTXO.0 != "DEPLOY_PENDING"
}
