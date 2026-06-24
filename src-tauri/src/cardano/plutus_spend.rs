//! Shared builder for Plutus spending transactions.
//!
//! Spends a single script-locked state UTxO, runs its validator via a
//! reference script, and (re)creates a continuing output carrying the
//! new datum — the shape every governance state transition takes
//! (election nominate / start-voting / finalize, proposal submit /
//! approve / vote / resolve, committee install, escrow settle).
//!
//! This is the spend-side counterpart to `plutus_mint`. Both use
//! pallas-txbuilder's native Plutus API so the redeemer purpose and
//! `script_data_hash` are emitted correctly; the legacy
//! `gov_tx_builder::inject_plutus_fields` path hardcoded a Spend
//! redeemer at index 0 and never set `script_data_hash`, so every script
//! tx it built was rejected on-chain.
//!
//! Flow: build (estimate) → `evaluate_tx` for real execution units →
//! rebuild with a real Plutus fee → sign (or hand back unsigned for
//! out-of-band cardano-cli signing).

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
const EST_MEM: u64 = 3_000_000;
const EST_STEPS: u64 = 1_200_000_000;

fn plutus_fee(min_fee_a: u64, min_fee_b: u64, tx_size: u64, mem: u64, steps: u64) -> u64 {
    let size_fee = min_fee_a * tx_size + min_fee_b;
    let exec_fee = (mem * PRICE_MEM_NUM).div_ceil(PRICE_MEM_DEN)
        + (steps * PRICE_STEP_NUM).div_ceil(PRICE_STEP_DEN);
    size_fee + exec_fee + 100_000
}

/// A token amount carried on an output (policy, asset name, quantity).
pub type AssetAmount = (Hash<28>, Vec<u8>, i64);

/// Spend one script state UTxO and recreate a continuing output.
pub struct SpendScript<'a> {
    /// Wallet that funds the tx (fee + collateral) and, by default,
    /// signs. Distinct from the script being spent.
    pub payment_address: &'a str,
    pub payment_key_extended: &'a [u8; 64],
    /// Key hashes added to `extra_signatories` (e.g. the self-nominee,
    /// the proposal author, committee members). The funding wallet's own
    /// key hash must be listed here when the validator checks for it.
    pub required_signers: &'a [[u8; 28]],

    /// The script UTxO being spent (tx hash, index) and its lovelace.
    pub script_input: (&'a str, u64),
    pub script_input_lovelace: u64,
    /// Spend redeemer CBOR (Plutus Data); always emitted Spend-purpose
    /// keyed to `script_input`.
    pub spend_redeemer: Vec<u8>,

    /// Continuing output: the script address the state returns to, its
    /// lovelace, the new inline datum, and any state tokens it must keep.
    pub continuing_address: PallasAddress,
    pub continuing_lovelace: u64,
    pub continuing_datum: Vec<u8>,
    pub continuing_assets: &'a [AssetAmount],

    /// Reference inputs (tx hash, index): the validator's reference
    /// script, plus any reputation / DAO-state-token UTxOs the validator
    /// reads via CIP-31.
    pub reference_inputs: &'a [(&'a str, u64)],

    /// Optional token mint (e.g. a vote receipt): policy, asset name,
    /// quantity, mint redeemer CBOR, and where the minted token lands.
    pub mint: Option<(AssetAmount, Vec<u8>, PallasAddress, u64)>,

    /// Validity-range upper bound slot (`invalid_from_slot`) — set for
    /// `before_deadline` checks. Defaults to tip + TTL when None.
    pub invalid_from_slot: Option<u64>,
    /// Validity-range lower bound slot (`valid_from_slot`) — set for
    /// `after_deadline` checks.
    pub valid_from_slot: Option<u64>,
}

/// Build + sign a spend tx per `spec`. Returns signed CBOR + tx hash.
pub async fn build_spend_tx(
    blockfrost: &BlockfrostClient,
    spec: SpendScript<'_>,
) -> Result<GovTxResult, TxBuildError> {
    let unsigned = build_spend_unsigned(blockfrost, &spec).await?;
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

/// Build the unsigned spend tx (final fee + ex-units, ready to witness).
/// Exposed for callers that sign out-of-band (e.g. a cardano-cli admin
/// key whose witness scheme differs from the app's extended key).
pub async fn build_spend_unsigned(
    blockfrost: &BlockfrostClient,
    spec: &SpendScript<'_>,
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

    // Funding input (largest non-ref-script) + a distinct collateral
    // UTxO. Reference-script UTxOs are excluded: consuming one destroys
    // a deployed validator's reference script.
    let mut pure: Vec<_> = utxos
        .iter()
        .filter(|u| u.lovelace() > 0 && !u.has_reference_script())
        .collect();
    pure.sort_by_key(|u| std::cmp::Reverse(u.lovelace()));
    let funding = *pure.first().ok_or(TxBuildError::NoUtxos)?;
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
    let script_in_hash = parse_tx_hash(spec.script_input.0)?;
    let funding_hash = parse_tx_hash(&funding.tx_hash)?;
    let funding_lovelace = funding.lovelace();
    let coll_hash = parse_tx_hash(&collateral.tx_hash)?;
    let script_input = Input::new(script_in_hash, spec.script_input.1);

    // Lovelace minted-token recipients receive their own min-UTxO; the
    // continuing output's lovelace comes from the spent script UTxO plus
    // any top-up from the funding wallet.
    let mint_lovelace = spec.mint.as_ref().map(|(_, _, _, l)| *l).unwrap_or(0);

    let build =
        |fee: u64, spend_ex: ExUnits, mint_ex: Option<ExUnits>| -> Result<Vec<u8>, TxBuildError> {
            // Value in  = script_input + funding (+ mint adds tokens, no ADA)
            // Value out = continuing + mint recipient + change + fee
            let total_in = spec.script_input_lovelace + funding_lovelace;
            let spent = spec.continuing_lovelace + mint_lovelace + fee;
            let change = total_in
                .checked_sub(spent)
                .ok_or(TxBuildError::InsufficientFunds {
                    needed: spent,
                    available: total_in,
                })?;

            // Continuing output (script address, new datum, kept state tokens).
            let mut continuing =
                Output::new(spec.continuing_address.clone(), spec.continuing_lovelace)
                    .set_inline_datum(spec.continuing_datum.clone());
            for (policy, name, qty) in spec.continuing_assets {
                continuing = continuing
                    .add_asset(*policy, name.clone(), *qty as u64)
                    .map_err(|e| TxBuildError::Builder(e.to_string()))?;
            }

            let mut staging = StagingTransaction::new()
                .input(script_input.clone())
                .input(Input::new(funding_hash, funding.tx_index))
                .add_spend_redeemer(
                    script_input.clone(),
                    spec.spend_redeemer.clone(),
                    Some(spend_ex),
                )
                .output(continuing)
                .collateral_input(Input::new(coll_hash, collateral.tx_index))
                .language_view(ScriptKind::PlutusV3, PLUTUS_V3_COST_MODEL.to_vec())
                .fee(fee)
                .network_id(0);

            // Reference inputs (script ref + reputation/DAO reads).
            for (h, idx) in spec.reference_inputs {
                let rh = parse_tx_hash(h)?;
                staging = staging.reference_input(Input::new(rh, *idx));
            }

            // Optional mint + its recipient output.
            if let Some(((policy, name, qty), redeemer, recipient, lovelace)) = &spec.mint {
                staging = staging
                    .mint_asset(*policy, name.clone(), *qty)
                    .map_err(|e| TxBuildError::Builder(e.to_string()))?
                    .add_mint_redeemer(*policy, redeemer.clone(), mint_ex)
                    .output(
                        Output::new(recipient.clone(), *lovelace)
                            .add_asset(*policy, name.clone(), *qty as u64)
                            .map_err(|e| TxBuildError::Builder(e.to_string()))?,
                    );
            }

            // Change back to the funding wallet.
            staging = staging.output(Output::new(change_addr.clone(), change));

            // Required signers.
            for s in spec.required_signers {
                staging = staging.disclosed_signer(Hash::<28>::from(*s));
            }

            // Validity range.
            staging =
                staging.invalid_from_slot(spec.invalid_from_slot.unwrap_or(tip_slot + TTL_OFFSET));
            if let Some(lb) = spec.valid_from_slot {
                staging = staging.valid_from_slot(lb);
            }

            Ok(staging
                .build_conway_raw()
                .map_err(|e| TxBuildError::Builder(e.to_string()))?
                .tx_bytes
                .0)
        };

    let ex = |m: u64, s: u64| ExUnits { mem: m, steps: s };
    let est_fee = plutus_fee(
        params.min_fee_a,
        params.min_fee_b,
        3_000,
        EST_MEM,
        EST_STEPS,
    );
    let mint_est = spec.mint.as_ref().map(|_| ex(EST_MEM, EST_STEPS));
    let draft = build(est_fee, ex(EST_MEM, EST_STEPS), mint_est)?;

    // evaluate_tx returns per-redeemer ex-units; take the max so both the
    // spend and any mint redeemer get a safe budget.
    let (mem, steps) = match blockfrost.evaluate_tx(&draft).await {
        Ok(units) if !units.is_empty() => units
            .into_iter()
            .fold((0u64, 0u64), |(m, s), (um, us)| (m.max(um), s.max(us))),
        Ok(_) => (EST_MEM, EST_STEPS),
        Err(e) => {
            log::debug!("plutus_spend: evaluate_tx failed, using estimate: {e}");
            (EST_MEM, EST_STEPS)
        }
    };
    let real_mem = if mem == 0 { EST_MEM } else { mem };
    let real_steps = if steps == 0 { EST_STEPS } else { steps };
    let tx_size = draft.len() as u64 + 200;
    let fee = plutus_fee(
        params.min_fee_a,
        params.min_fee_b,
        tx_size,
        real_mem,
        real_steps,
    );

    build(
        fee,
        ex(real_mem, real_steps),
        spec.mint.as_ref().map(|_| ex(real_mem, real_steps)),
    )
}
