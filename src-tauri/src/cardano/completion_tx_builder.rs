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
use pallas_txbuilder::{BuildConway, ExUnits, Input, Output, ScriptKind, StagingTransaction};
use pallas_wallet::PrivateKey;

use super::blockfrost::BlockfrostClient;
use super::cost_models::PLUTUS_V3_COST_MODEL;
use super::gov_tx_builder::{self, GovTxResult};
use super::plutus_data;
use super::script_refs;
use super::tx_builder::{parse_tx_hash, sign_raw_tx, TxBuildError, MIN_NFT_LOVELACE, TTL_OFFSET};

/// Execution-unit prices (preprod/mainnet, current protocol version):
/// price_mem = 0.0577, price_step = 0.0000721. Expressed as rationals to
/// keep the fee math integer-only.
const PRICE_MEM_NUM: u64 = 577;
const PRICE_MEM_DEN: u64 = 10_000;
const PRICE_STEP_NUM: u64 = 721;
const PRICE_STEP_DEN: u64 = 10_000_000;

/// Collateral must cover `collateral_percent` (150%) of the fee. A small
/// dedicated pure-ADA UTxO (≥ 5 ADA) comfortably covers any mint fee.
const MIN_COLLATERAL_LOVELACE: u64 = 5_000_000;

/// Total fee = size fee + Plutus execution fee, with a safety buffer so
/// the on-chain recomputation never undershoots.
fn plutus_fee(min_fee_a: u64, min_fee_b: u64, tx_size: u64, mem: u64, steps: u64) -> u64 {
    let size_fee = min_fee_a * tx_size + min_fee_b;
    let exec_fee = (mem * PRICE_MEM_NUM).div_ceil(PRICE_MEM_DEN)
        + (steps * PRICE_STEP_NUM).div_ceil(PRICE_STEP_DEN);
    size_fee + exec_fee + 30_000 // buffer
}

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

    // 4. UTxO selection. The spend input funds the mint; collateral MUST
    //    be a *distinct* pure-ADA UTxO (a UTxO cannot be both a regular
    //    input and collateral). Pick the largest as the spend input and a
    //    separate ≥5-ADA UTxO as collateral.
    let pure: Vec<&_> = {
        let mut v: Vec<_> = utxos.iter().filter(|u| u.lovelace() > 0).collect();
        v.sort_by_key(|u| std::cmp::Reverse(u.lovelace()));
        v
    };
    let selected = *pure.first().ok_or(TxBuildError::NoUtxos)?;
    let collateral = pure
        .iter()
        .skip(1)
        .find(|u| u.lovelace() >= MIN_COLLATERAL_LOVELACE)
        .copied()
        .ok_or(TxBuildError::InsufficientFunds {
            needed: MIN_COLLATERAL_LOVELACE,
            available: pure.iter().skip(1).map(|u| u.lovelace()).max().unwrap_or(0),
        })?;

    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;
    let input_lovelace = selected.lovelace();

    let ref_utxo = script_refs::COMPLETION_MINTING_REF_UTXO;
    let ref_hash = parse_tx_hash(ref_utxo.0)?;
    let coll_hash = parse_tx_hash(&collateral.tx_hash)?;

    // Build the staging tx for a given fee + ex-units. Output 0 carries
    // the minted token + inline CompletionDatum; output 1 is change.
    // pallas-txbuilder's native Plutus support sets the Mint-purpose
    // redeemer and computes script_data_hash (from `language_view`) —
    // both of which the previous `inject_plutus_fields` path got wrong.
    let build = |fee: u64, ex: ExUnits| -> Result<Vec<u8>, TxBuildError> {
        let change = input_lovelace
            .checked_sub(MIN_COMPLETION_UTXO_LOVELACE + fee)
            .ok_or(TxBuildError::InsufficientFunds {
                needed: MIN_COMPLETION_UTXO_LOVELACE + fee,
                available: input_lovelace,
            })?;
        let staging = StagingTransaction::new()
            .input(Input::new(input_tx_hash, selected.tx_index))
            .output(
                Output::new(pallas_addr.clone(), MIN_COMPLETION_UTXO_LOVELACE)
                    .add_asset(policy_id, asset_name.to_vec(), 1)
                    .map_err(|e| TxBuildError::Builder(e.to_string()))?
                    .set_inline_datum(datum_cbor.clone()),
            )
            .output(Output::new(pallas_addr.clone(), change))
            .mint_asset(policy_id, asset_name.to_vec(), 1)
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .reference_input(Input::new(ref_hash, ref_utxo.1))
            .collateral_input(Input::new(coll_hash, collateral.tx_index))
            .add_mint_redeemer(policy_id, redeemer_cbor.clone(), Some(ex))
            .language_view(ScriptKind::PlutusV3, PLUTUS_V3_COST_MODEL.to_vec())
            .disclosed_signer(Hash::<28>::from(*payment_key_hash))
            .fee(fee)
            .invalid_from_slot(tip_slot + TTL_OFFSET)
            .network_id(0);
        Ok(staging
            .build_conway_raw()
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .tx_bytes
            .0)
    };

    // 5. Pass 1 — generous estimate so evaluate_tx can run.
    const EST_MEM: u64 = 2_000_000;
    const EST_STEPS: u64 = 700_000_000;
    let est_fee = plutus_fee(
        params.min_fee_a,
        params.min_fee_b,
        2_000,
        EST_MEM,
        EST_STEPS,
    );
    let draft = build(
        est_fee,
        ExUnits {
            mem: EST_MEM,
            steps: EST_STEPS,
        },
    )?;

    // 6. Evaluate real execution units, then recompute fee from the
    //    drafted size + true ex-units. Falls back to the estimate if the
    //    evaluator is unavailable.
    let (mem, steps) = match blockfrost.evaluate_tx(&draft).await {
        Ok(units) => units
            .into_iter()
            .fold((0u64, 0u64), |(m, s), (um, us)| (m.max(um), s.max(us))),
        Err(e) => {
            log::debug!("completion_mint: evaluate_tx failed, using estimate: {e}");
            (EST_MEM, EST_STEPS)
        }
    };
    let real_ex = ExUnits {
        mem: if mem == 0 { EST_MEM } else { mem },
        steps: if steps == 0 { EST_STEPS } else { steps },
    };
    // +200 bytes covers the witness signature added at signing time.
    let tx_size = draft.len() as u64 + 200;
    let fee = plutus_fee(
        params.min_fee_a,
        params.min_fee_b,
        tx_size,
        real_ex.mem,
        real_ex.steps,
    );

    // 7. Pass 2 — rebuild with true ex-units + fee, then sign.
    let final_tx = build(fee, real_ex)?;
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed_tx_bytes = sign_raw_tx(&final_tx, &private_key)?;
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

    /// Live preprod end-to-end mint. Ignored by default; run with a
    /// funded wallet:
    ///   BLOCKFROST_PROJECT_ID=preprod… TEST_MNEMONIC="…24 words…" \
    ///     cargo test -p alexandria-node live_completion_mint_preprod -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_completion_mint_preprod() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let mnemonic = std::env::var("TEST_MNEMONIC").expect("TEST_MNEMONIC");
        let course_id = b"course_plugin_demo";
        let leaves: Vec<[u8; 32]> = vec![[7u8; 32]];
        let root = crate::domain::completion::merkle_root(&leaves);
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(async {
            let bf = BlockfrostClient::new(pid).unwrap();
            let wallet = crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).unwrap();
            let subject_pubkey: [u8; 32] = *wallet.signing_key.verifying_key().as_bytes();
            let built = build_completion_mint_tx(
                &bf,
                &wallet.payment_address,
                &wallet.payment_key_hash,
                &wallet.payment_key_extended,
                &subject_pubkey,
                course_id,
                &leaves,
                &root,
                ts_ms,
            )
            .await
            .map_err(|e| format!("build: {e}"))?;
            bf.submit_tx(&built.tx_cbor)
                .await
                .map_err(|e| format!("submit: {e}"))
        });
        println!("LIVE_MINT_RESULT: {outcome:?}");
        outcome.expect("mint accepted on preprod");
    }
}
