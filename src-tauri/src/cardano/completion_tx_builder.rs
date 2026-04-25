//! Completion-witness minting transaction builder.
//!
//! Produces the tx that the observer (`cardano::completion`) will
//! later pick up: mints a single token under the completion policy
//! with asset name `learner_pkh || course_tag`, attaches an inline
//! `CompletionDatum` on the output carrying the token, and includes
//! the learner's signature + the full element-leaves list in the
//! redeemer so the validator can reconstruct the Merkle root.
//!
//! Gated behind [`super::script_refs::COMPLETION_MINTING_REF_UTXO`]
//! being deployed — matches the posture of the other governance tx
//! builders on main. Callers get a descriptive error while the
//! reference script deployment is pending.

use blake2::digest::consts::U28;
use blake2::{Blake2b, Digest};
use pallas_addresses::Address as PallasAddress;
use pallas_crypto::hash::Hash;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

use super::blockfrost::BlockfrostClient;
use super::gov_tx_builder::{self, GovTxResult};
use super::plutus_data;
use super::script_refs;
use super::tx_builder::{
    parse_tx_hash, sign_raw_tx, TxBuildError, MIN_NFT_LOVELACE, MIN_UTXO_LOVELACE, TTL_OFFSET,
};

/// Minimum ADA to attach to the output that carries the completion
/// token + inline datum. The datum is small (~80 bytes) so `MIN_NFT_LOVELACE`
/// (2 ADA) is a safe upper bound.
const MIN_COMPLETION_UTXO_LOVELACE: u64 = 2_000_000;

/// Bytes of the learner's pubkey-hash slot inside a completion asset name.
pub const LEARNER_PKH_LENGTH: usize = 28;

/// Bytes of the course-tag slot inside a completion asset name.
pub const COURSE_TAG_LENGTH: usize = 4;

/// Total asset-name length; matches `completion_asset_name_length` in
/// `lib/alexandria/completion.ak`.
pub const COMPLETION_ASSET_NAME_LENGTH: usize = LEARNER_PKH_LENGTH + COURSE_TAG_LENGTH;

/// Derive a 4-byte course tag from the full course id. The tag is the
/// first `COURSE_TAG_LENGTH` bytes of `blake2b-224(course_id)` —
/// enough entropy to avoid learner-scoped collisions without spending
/// more of the 32-byte asset-name budget.
pub fn course_tag(course_id: &[u8]) -> [u8; COURSE_TAG_LENGTH] {
    let mut hasher = Blake2b::<U28>::new();
    hasher.update(course_id);
    let full = hasher.finalize();
    let mut out = [0u8; COURSE_TAG_LENGTH];
    out.copy_from_slice(&full[..COURSE_TAG_LENGTH]);
    out
}

/// Build a completion asset name from the learner pkh and course id.
pub fn completion_asset_name(
    learner_pkh: &[u8; LEARNER_PKH_LENGTH],
    course_id: &[u8],
) -> [u8; COMPLETION_ASSET_NAME_LENGTH] {
    let tag = course_tag(course_id);
    let mut out = [0u8; COMPLETION_ASSET_NAME_LENGTH];
    out[..LEARNER_PKH_LENGTH].copy_from_slice(learner_pkh);
    out[LEARNER_PKH_LENGTH..].copy_from_slice(&tag);
    out
}

/// Build, sign, and submit a completion-witness minting tx.
///
/// Requires:
///   * `payment_address` — learner's wallet address (receives the minted token).
///   * `payment_key_hash` — pkh that will sign; must match the 28-byte
///     prefix of the completion asset name.
///   * `payment_key_extended` — 64-byte extended Ed25519 secret key.
///   * `subject_pubkey` — learner's 32-byte Ed25519 verification key,
///     embedded in the datum so the observer can derive the `did:key`.
///   * `course_id` — course identifier bytes (arbitrary length).
///   * `element_leaves` — completion leaves in declaration order.
///   * `completion_root` — precomputed root; MUST equal
///     `domain::completion::merkle_root(element_leaves)`. We don't
///     recompute to avoid accidentally diverging if the caller already
///     derived the root from a trusted course template.
///   * `timestamp_ms` — learner-supplied POSIX milliseconds; the
///     validator requires it to fall inside the tx validity window.
///
/// On success returns the signed tx CBOR + its hash; the hash is what
/// the observer will index via `list_policy_assets` and eventually
/// land in the issued VC's `witness.tx_hash`.
#[allow(clippy::too_many_arguments)]
pub async fn build_completion_mint_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; LEARNER_PKH_LENGTH],
    payment_key_extended: &[u8; 64],
    subject_pubkey: &[u8; 32],
    course_id: &[u8],
    element_leaves: &[[u8; 32]],
    completion_root: &[u8; 32],
    timestamp_ms: i64,
) -> Result<GovTxResult, TxBuildError> {
    if !script_refs::completion_ref_deployed() {
        return Err(TxBuildError::Cbor(
            "CompletionMint: completion validator reference UTxO not yet deployed".into(),
        ));
    }

    // 1. Derive policy id + asset name.
    let policy_hash =
        gov_tx_builder::hash_from_hex_pub(script_refs::COMPLETION_MINTING_SCRIPT_HASH)?;
    let policy_id = Hash::<28>::from(policy_hash);
    let asset_name = completion_asset_name(payment_key_hash, course_id);

    // 2. Encode datum + redeemer.
    let datum_cbor = plutus_data::encode_completion_datum(
        subject_pubkey,
        course_id,
        completion_root,
        timestamp_ms,
    )?;
    let redeemer_cbor = plutus_data::encode_completion_mint_redeemer(element_leaves)?;

    // 3. Chain state (fan-out).
    let (utxos_res, params_res, tip_res) = tokio::join!(
        blockfrost.get_utxos(payment_address),
        blockfrost.get_protocol_params(),
        blockfrost.get_tip_slot(),
    );
    let utxos = utxos_res?;
    let params = params_res?;
    let tip_slot = tip_res?;

    if utxos.is_empty() {
        return Err(TxBuildError::NoUtxos);
    }
    let selected = BlockfrostClient::select_utxo(&utxos, MIN_UTXO_LOVELACE).ok_or(
        TxBuildError::InsufficientFunds {
            needed: MIN_UTXO_LOVELACE,
            available: utxos.iter().map(|u| u.lovelace()).sum(),
        },
    )?;

    // 4. Addresses + fee.
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;
    let fee = params.calculate_min_fee(1200).max(500_000);
    let input_lovelace = selected.lovelace();
    let needed = MIN_COMPLETION_UTXO_LOVELACE + fee;
    if input_lovelace < needed {
        return Err(TxBuildError::InsufficientFunds {
            needed,
            available: input_lovelace,
        });
    }
    let change = input_lovelace - MIN_COMPLETION_UTXO_LOVELACE - fee;

    // 5. Build skeleton. Output 0 is the token + inline-datum output
    //    (pallas_txbuilder can't emit inline datums; we inject below).
    //    `inject_plutus_fields` writes the datum on the first output.
    let staging_tx = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        .output(
            Output::new(pallas_addr.clone(), MIN_COMPLETION_UTXO_LOVELACE)
                .add_asset(policy_id, asset_name.to_vec(), 1)
                .map_err(|e| TxBuildError::Builder(e.to_string()))?,
        )
        .output(Output::new(pallas_addr, change))
        .mint_asset(policy_id, asset_name.to_vec(), 1)
        .map_err(|e| TxBuildError::Builder(e.to_string()))?
        .disclosed_signer(Hash::<28>::from(*payment_key_hash))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0);

    let built = staging_tx
        .build_conway_raw()
        .map_err(|e| TxBuildError::Builder(e.to_string()))?;

    // 6. Inject the Plutus fields — reference script UTxO, collateral,
    //    redeemer, and inline datum on output 0.
    let ref_utxo = script_refs::COMPLETION_MINTING_REF_UTXO;
    let ref_hash: [u8; 32] = hex::decode(ref_utxo.0)
        .map_err(|e| TxBuildError::TxDecode(format!("invalid ref utxo hash: {e}")))?
        .try_into()
        .map_err(|_| TxBuildError::TxDecode("ref utxo hash must be 32 bytes".into()))?;

    let coll_hash: [u8; 32] = hex::decode(&selected.tx_hash)
        .map_err(|e| TxBuildError::TxDecode(format!("invalid collateral hash: {e}")))?
        .try_into()
        .map_err(|_| TxBuildError::TxDecode("collateral hash must be 32 bytes".into()))?;

    let (tx_with_plutus, _) = gov_tx_builder::inject_plutus_fields(
        &built.tx_bytes.0,
        &[(ref_hash, ref_utxo.1)],
        &[(coll_hash, selected.tx_index)],
        &redeemer_cbor,
        Some(&datum_cbor),
    )?;

    // 7. Evaluate ex units (non-fatal log — real exec units are
    //    computed by the cardano-node during submission; our initial
    //    estimate lives inside `inject_plutus_fields`).
    if let Err(e) = blockfrost.evaluate_tx(&tx_with_plutus).await {
        log::debug!("completion_mint: evaluate_tx failed (non-fatal): {e}");
    }

    // 8. Sign.
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed_tx_bytes = sign_raw_tx(&tx_with_plutus, &private_key)?;
    let tx_hash = super::tx_builder::compute_tx_hash(&signed_tx_bytes)?;

    Ok(GovTxResult {
        tx_cbor: signed_tx_bytes,
        tx_hash,
    })
}

/// Rough CIP-25 analog for completion mints. Currently a no-op
/// placeholder so callers can pick a consistent label if they want to
/// attach human-readable metadata alongside the on-chain witness.
/// The current validator does not inspect metadata.
pub const COMPLETION_METADATA_LABEL: u64 = 1698;

// Keeping MIN_NFT_LOVELACE re-exported for test ergonomics; same
// budget as the minted output.
pub const MIN_NFT_UTXO: u64 = MIN_NFT_LOVELACE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_name_has_expected_length_and_structure() {
        let pkh = [0x11u8; LEARNER_PKH_LENGTH];
        let name = completion_asset_name(&pkh, b"course-42");
        assert_eq!(name.len(), COMPLETION_ASSET_NAME_LENGTH);
        assert_eq!(&name[..LEARNER_PKH_LENGTH], &pkh);
        let tag = course_tag(b"course-42");
        assert_eq!(&name[LEARNER_PKH_LENGTH..], &tag);
    }

    #[test]
    fn course_tag_is_deterministic() {
        assert_eq!(course_tag(b"abc"), course_tag(b"abc"));
        assert_ne!(course_tag(b"abc"), course_tag(b"abd"));
    }

    #[test]
    fn different_learners_get_different_asset_names() {
        let pkh_a = [0x01u8; LEARNER_PKH_LENGTH];
        let pkh_b = [0x02u8; LEARNER_PKH_LENGTH];
        assert_ne!(
            completion_asset_name(&pkh_a, b"x"),
            completion_asset_name(&pkh_b, b"x")
        );
    }

    #[test]
    fn build_fails_with_invalid_blockfrost_credentials() {
        // With the completion validator now deployed
        // (`COMPLETION_MINTING_REF_UTXO` populated), the deploy gate
        // passes and the call falls through to the Blockfrost query.
        // The fake project id makes the chain query fail; we only
        // care that the function errors out cleanly rather than
        // panicking or silently succeeding.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            let bf = BlockfrostClient::new("test_project_id".into()).unwrap();
            build_completion_mint_tx(
                &bf,
                "addr_test1qzrlkwg6uwk96ak8lslkyqjjx20x4x0qkvxcs9htm2c0ucpdwjsxgyjhvv3w7p3lfrq8gngaehaw6vqwsm25x2la2l2s2lg7hu",
                &[0u8; 28],
                &[0u8; 64],
                &[0u8; 32],
                b"course-x",
                &[[0u8; 32]],
                &[0u8; 32],
                1_700_000_000_000,
            )
            .await
        });
        assert!(result.is_err(), "expected error with fake credentials");
    }

    #[test]
    fn metadata_label_chosen_clear_of_existing_labels() {
        // Avoids CIP-25 (721), governance (1694), and anchor (1697)
        // labels, matching the block of reserved-ish Alexandria labels.
        assert_ne!(COMPLETION_METADATA_LABEL, 721);
        assert_ne!(
            COMPLETION_METADATA_LABEL,
            script_refs::GOVERNANCE_METADATA_LABEL
        );
        assert_ne!(
            COMPLETION_METADATA_LABEL,
            script_refs::ALEXANDRIA_ANCHOR_LABEL
        );
    }

    #[test]
    fn min_utxo_matches_nft_budget() {
        assert_eq!(MIN_NFT_UTXO, super::super::tx_builder::MIN_NFT_LOVELACE);
    }
}
