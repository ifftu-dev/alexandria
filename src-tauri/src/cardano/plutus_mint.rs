//! Shared builder for Plutus minting transactions.
//!
//! Mints exactly one token under a reference-scripted policy and sends it
//! to a recipient (a wallet, or a script address with an inline datum),
//! using pallas-txbuilder's native Plutus API so the Mint-purpose
//! redeemer and `script_data_hash` are produced correctly. This is the
//! generalization of the proven `completion_tx_builder` path — the
//! earlier `gov_tx_builder::inject_plutus_fields` route emitted a Spend
//! redeemer and never set `script_data_hash`, so every mint it built was
//! rejected on-chain.
//!
//! Flow: build (estimate) → `evaluate_tx` for real execution units →
//! rebuild with a real Plutus fee → sign.

use pallas_addresses::Address as PallasAddress;
use pallas_crypto::hash::Hash;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_txbuilder::{BuildConway, ExUnits, Input, Output, ScriptKind, StagingTransaction};
use pallas_wallet::PrivateKey;

use super::blockfrost::BlockfrostClient;
use super::cost_models::PLUTUS_V3_COST_MODEL;
use super::gov_tx_builder::GovTxResult;
use super::tx_builder::{parse_tx_hash, sign_raw_tx, TxBuildError, TTL_OFFSET};

const PRICE_MEM_NUM: u64 = 577;
const PRICE_MEM_DEN: u64 = 10_000;
const PRICE_STEP_NUM: u64 = 721;
const PRICE_STEP_DEN: u64 = 10_000_000;
const MIN_COLLATERAL_LOVELACE: u64 = 5_000_000;
const EST_MEM: u64 = 2_000_000;
const EST_STEPS: u64 = 700_000_000;

fn plutus_fee(min_fee_a: u64, min_fee_b: u64, tx_size: u64, mem: u64, steps: u64) -> u64 {
    let size_fee = min_fee_a * tx_size + min_fee_b;
    let exec_fee = (mem * PRICE_MEM_NUM).div_ceil(PRICE_MEM_DEN)
        + (steps * PRICE_STEP_NUM).div_ceil(PRICE_STEP_DEN);
    size_fee + exec_fee + 30_000
}

/// One token to mint (qty +1) under `policy_id`, sent to `recipient`.
pub struct MintToAddress<'a> {
    /// Wallet that funds the tx (pays fee + min-UTxO) and signs.
    pub payment_address: &'a str,
    pub payment_key_extended: &'a [u8; 64],
    /// Required signers added to the tx body (e.g. the policy's
    /// authorized admin). The funding wallet's own key hash should be
    /// included when it must appear in `extra_signatories`.
    pub required_signers: &'a [[u8; 28]],
    pub policy_id: Hash<28>,
    pub asset_name: Vec<u8>,
    /// Mint redeemer CBOR (Plutus Data). Always emitted Mint-purpose.
    pub mint_redeemer: Vec<u8>,
    /// Reference UTxO carrying the minting policy script (hash, index).
    pub ref_script: (&'a str, u64),
    /// Where the minted token lands (wallet or script address).
    pub recipient_address: PallasAddress,
    /// Lovelace attached to the recipient output (≥ min-UTxO for a
    /// token + datum, typically 2–3 ADA).
    pub recipient_lovelace: u64,
    /// Inline datum CBOR for the recipient output (script recipients).
    pub recipient_datum: Option<Vec<u8>>,
}

/// Build + sign a mint tx per `spec`. Returns the signed CBOR + tx hash.
pub async fn build_mint_to_address_tx(
    blockfrost: &BlockfrostClient,
    spec: MintToAddress<'_>,
) -> Result<GovTxResult, TxBuildError> {
    let unsigned = build_mint_to_address_unsigned(blockfrost, &spec).await?;
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*spec.payment_key_extended)
    });
    let signed = sign_raw_tx(&unsigned, &private_key)?;
    let tx_hash = super::tx_builder::compute_tx_hash(&signed)?;
    Ok(GovTxResult {
        tx_cbor: signed,
        tx_hash,
    })
}

/// Build the unsigned mint tx (final fee + ex-units, ready to witness).
/// Exposed so callers that sign with an out-of-band key (e.g. a
/// cardano-cli admin key whose witness scheme differs from the app's
/// extended key) can attach the witness themselves.
pub async fn build_mint_to_address_unsigned(
    blockfrost: &BlockfrostClient,
    spec: &MintToAddress<'_>,
) -> Result<Vec<u8>, TxBuildError> {
    let (utxos_res, params_res, tip_res) = tokio::join!(
        blockfrost.get_utxos(spec.payment_address),
        blockfrost.get_protocol_params(),
        blockfrost.get_tip_slot(),
    );
    let utxos = utxos_res?;
    let params = params_res?;
    let tip_slot = tip_res?;
    if utxos.is_empty() {
        return Err(TxBuildError::NoUtxos);
    }

    // Distinct spend input (largest) + collateral (separate ≥5-ADA UTxO).
    let mut pure: Vec<_> = utxos.iter().filter(|u| u.lovelace() > 0).collect();
    pure.sort_by_key(|u| std::cmp::Reverse(u.lovelace()));
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

    let change_addr = PallasAddress::from_bech32(spec.payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;
    let input_lovelace = selected.lovelace();
    let ref_hash = parse_tx_hash(spec.ref_script.0)?;
    let coll_hash = parse_tx_hash(&collateral.tx_hash)?;

    let build = |fee: u64, ex: ExUnits| -> Result<Vec<u8>, TxBuildError> {
        let change = input_lovelace
            .checked_sub(spec.recipient_lovelace + fee)
            .ok_or(TxBuildError::InsufficientFunds {
                needed: spec.recipient_lovelace + fee,
                available: input_lovelace,
            })?;
        let mut recipient = Output::new(spec.recipient_address.clone(), spec.recipient_lovelace)
            .add_asset(spec.policy_id, spec.asset_name.clone(), 1)
            .map_err(|e| TxBuildError::Builder(e.to_string()))?;
        if let Some(ref datum) = spec.recipient_datum {
            recipient = recipient.set_inline_datum(datum.clone());
        }
        let mut staging = StagingTransaction::new()
            .input(Input::new(input_tx_hash, selected.tx_index))
            .output(recipient)
            .output(Output::new(change_addr.clone(), change))
            .mint_asset(spec.policy_id, spec.asset_name.clone(), 1)
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .reference_input(Input::new(ref_hash, spec.ref_script.1))
            .collateral_input(Input::new(coll_hash, collateral.tx_index))
            .add_mint_redeemer(spec.policy_id, spec.mint_redeemer.clone(), Some(ex))
            .language_view(ScriptKind::PlutusV3, PLUTUS_V3_COST_MODEL.to_vec())
            .fee(fee)
            .invalid_from_slot(tip_slot + TTL_OFFSET)
            .network_id(0);
        for s in spec.required_signers {
            staging = staging.disclosed_signer(Hash::<28>::from(*s));
        }
        Ok(staging
            .build_conway_raw()
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .tx_bytes
            .0)
    };

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
    let (mem, steps) = match blockfrost.evaluate_tx(&draft).await {
        Ok(units) => units
            .into_iter()
            .fold((0u64, 0u64), |(m, s), (um, us)| (m.max(um), s.max(us))),
        Err(e) => {
            log::debug!("plutus_mint: evaluate_tx failed, using estimate: {e}");
            (EST_MEM, EST_STEPS)
        }
    };
    let real_ex = ExUnits {
        mem: if mem == 0 { EST_MEM } else { mem },
        steps: if steps == 0 { EST_STEPS } else { steps },
    };
    let tx_size = draft.len() as u64 + 200;
    let fee = plutus_fee(
        params.min_fee_a,
        params.min_fee_b,
        tx_size,
        real_ex.mem,
        real_ex.steps,
    );

    build(fee, real_ex)
}
