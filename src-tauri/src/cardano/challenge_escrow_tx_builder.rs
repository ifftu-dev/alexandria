//! Challenge-stake escrow transaction builders.
//!
//! Two flows backing the `challenge_escrow.ak` validator:
//!
//!   * [`build_lock_tx`] — the challenger pays their stake to the escrow
//!     script address with an inline [`ChallengeEscrowDatum`]. This is a
//!     plain pay-to-script output, so it does not require the validator
//!     to be deployed as a reference script.
//!   * [`build_settle_tx`] — the DAO authority spends the escrow UTxO,
//!     paying the stake to the challenger (`Refund`, challenge upheld) or
//!     the DAO treasury (`Forfeit`, rejected). Spending the script needs
//!     its reference script on-chain, so this is gated on
//!     [`script_refs::challenge_escrow_deployed`].
//!
//! Execution-unit estimation and redeemer indexing are validated against
//! preprod at deploy time, matching the rest of the governance tx layer.

use pallas_addresses::Address as PallasAddress;
use pallas_crypto::hash::Hash;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

use super::blockfrost::BlockfrostClient;
use super::gov_tx_builder::{inject_plutus_fields, script_address, GovTxResult};
use super::plutus_data;
use super::script_refs;
use super::tx_builder::{
    compute_tx_hash, parse_tx_hash, sign_raw_tx, TxBuildError, MIN_UTXO_LOVELACE, TTL_OFFSET,
};

/// Minimum ADA at the escrow UTxO (covers the inline datum storage).
const MIN_ESCROW_UTXO_LOVELACE: u64 = 2_000_000;

/// Build the stake-lock transaction: pay `stake_lovelace` to the escrow
/// script address carrying an inline `ChallengeEscrowDatum`. Signed by
/// the challenger's wallet.
#[allow(clippy::too_many_arguments)]
pub async fn build_lock_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    treasury_key_hash: &[u8; 28],
    dao_authority_key_hash: &[u8; 28],
    challenge_id_hash: &[u8],
    stake_lovelace: u64,
) -> Result<GovTxResult, TxBuildError> {
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
    if stake_lovelace < MIN_ESCROW_UTXO_LOVELACE {
        return Err(TxBuildError::Cbor(format!(
            "stake {stake_lovelace} below escrow UTxO minimum {MIN_ESCROW_UTXO_LOVELACE}"
        )));
    }

    let need = stake_lovelace + 1_000_000; // stake + fee headroom
    let selected =
        BlockfrostClient::select_utxo(&utxos, need).ok_or(TxBuildError::InsufficientFunds {
            needed: need,
            available: utxos.iter().map(|u| u.lovelace()).sum(),
        })?;

    let escrow_addr = script_address(script_refs::CHALLENGE_ESCROW_SCRIPT_HASH)?;
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;

    let fee = params.calculate_min_fee(800).max(200_000);
    let input_lovelace = selected.lovelace();
    if input_lovelace < stake_lovelace + fee {
        return Err(TxBuildError::InsufficientFunds {
            needed: stake_lovelace + fee,
            available: input_lovelace,
        });
    }
    let change = input_lovelace - stake_lovelace - fee;

    let datum = plutus_data::encode_challenge_escrow_datum(
        payment_key_hash,
        treasury_key_hash,
        dao_authority_key_hash,
        challenge_id_hash,
    )?;

    let built_tx = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        // Output 0: escrow UTxO with inline datum.
        .output(Output::new(escrow_addr, stake_lovelace))
        // Output 1: change back to the challenger.
        .output(Output::new(pallas_addr, change))
        .disclosed_signer(Hash::<28>::from(*payment_key_hash))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0)
        .build_conway_raw()
        .map_err(|e| TxBuildError::Builder(e.to_string()))?;

    // Inline the datum on output 0 (no redeemer / collateral — this is a
    // plain pay-to-script, not a script spend).
    let (tx_bytes, _) = inject_plutus_fields(&built_tx.tx_bytes.0, &[], &[], &[], Some(&datum))?;

    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed = sign_raw_tx(&tx_bytes, &private_key)?;
    let tx_hash = compute_tx_hash(&signed)?;
    Ok(GovTxResult {
        tx_cbor: signed,
        tx_hash,
    })
}

/// Build the settlement transaction: the DAO authority spends the escrow
/// UTxO and pays the stake to `recipient_key_hash` — the challenger on
/// `Refund` (upheld) or the treasury on `Forfeit` (rejected). Gated on
/// the escrow validator being deployed as a reference script.
#[allow(clippy::too_many_arguments)]
pub async fn build_settle_tx(
    blockfrost: &BlockfrostClient,
    authority_address: &str,
    authority_key_hash: &[u8; 28],
    authority_key_extended: &[u8; 64],
    escrow_utxo: (&str, u64),
    recipient_key_hash: &[u8; 28],
    stake_lovelace: u64,
    refund: bool,
) -> Result<GovTxResult, TxBuildError> {
    if !script_refs::challenge_escrow_deployed() {
        return Err(TxBuildError::Cbor(
            "ChallengeSettle: escrow validator not yet deployed as a reference script".into(),
        ));
    }

    // Settlement is a Plutus SPEND of the escrow UTxO: it needs a
    // Spend-purpose redeemer keyed to that input plus a script_data_hash,
    // which the legacy `inject_plutus_fields` path never produced (it
    // hardcoded a Spend redeemer at index 0 and omitted script_data_hash,
    // so every settle tx was rejected on-chain). Delegate to the native
    // `plutus_spend` builder — the path verified on preprod for the escrow
    // refund. The escrow holds no continuing state, so the output simply
    // pays the recipient (challenger on Refund, treasury on Forfeit) at a
    // vkey address; the validator's `pays_to` check is satisfied by its
    // payment credential.
    let recipient_addr = enterprise_address(recipient_key_hash)?;
    let redeemer = plutus_data::encode_challenge_escrow_redeemer(refund)?;
    let signers = [*authority_key_hash];

    crate::cardano::plutus_spend::build_spend_tx(
        blockfrost,
        crate::cardano::plutus_spend::SpendScript {
            payment_address: authority_address,
            payment_key_extended: authority_key_extended,
            required_signers: &signers,
            script_input: escrow_utxo,
            script_input_lovelace: stake_lovelace,
            spend_redeemer: redeemer,
            continuing_address: recipient_addr,
            continuing_lovelace: stake_lovelace,
            // Constr(0, []) placeholder datum; the recipient is a vkey
            // address and the escrow keeps no continuing state.
            continuing_datum: vec![0xd8, 0x79, 0x80],
            continuing_assets: &[],
            reference_inputs: &[script_refs::CHALLENGE_ESCROW_REF_UTXO],
            mint: None,
            invalid_from_slot: None,
            valid_from_slot: None,
        },
    )
    .await
}

/// Build a preprod enterprise address (payment-only, no staking part)
/// from a 28-byte payment key hash. Header `0x60` = testnet enterprise
/// key address.
fn enterprise_address(key_hash: &[u8; 28]) -> Result<PallasAddress, TxBuildError> {
    let mut bytes = Vec::with_capacity(29);
    bytes.push(0x60);
    bytes.extend_from_slice(key_hash);
    PallasAddress::from_bytes(&bytes)
        .map_err(|e| TxBuildError::AddressParse(format!("enterprise address: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enterprise_address_is_testnet_payment_only() {
        let kh = [0x11u8; 28];
        let addr = enterprise_address(&kh).unwrap();
        // Round-trips to bech32 and is a testnet address.
        let b32 = addr.to_bech32().unwrap();
        assert!(b32.starts_with("addr_test1"), "got {b32}");
    }

    #[test]
    fn escrow_datum_redeemer_encode_distinct() {
        let d = plutus_data::encode_challenge_escrow_datum(
            &[1u8; 28],
            &[2u8; 28],
            &[3u8; 28],
            b"challenge-1",
        )
        .unwrap();
        // Constr 0 with 4 fields → tag 121 (0xd8 0x79) + array(4).
        assert_eq!(&d[0..2], &[0xd8, 0x79]);

        let refund = plutus_data::encode_challenge_escrow_redeemer(true).unwrap();
        let forfeit = plutus_data::encode_challenge_escrow_redeemer(false).unwrap();
        assert_ne!(refund, forfeit);
        assert_eq!(&refund[0..2], &[0xd8, 0x79]); // Constr 0
        assert_eq!(&forfeit[0..2], &[0xd8, 0x7a]); // Constr 1
    }
}
