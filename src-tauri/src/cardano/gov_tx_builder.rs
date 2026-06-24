//! Governance transaction builders for Plutus V3 script interactions.
//!
//! Each builder follows this flow:
//! 1. Query Blockfrost for relevant UTxOs (wallet, script state)
//! 2. Build the transaction skeleton via pallas-txbuilder
//! 3. Inject Plutus-specific fields via decode-modify-reencode
//! 4. Evaluate execution units via Blockfrost
//! 5. Rebuild with accurate ex-units
//! 6. Sign and return CBOR bytes + tx hash

use pallas_addresses::Address as PallasAddress;
use pallas_codec::utils::MaybeIndefArray;
use pallas_crypto::hash::Hash;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_primitives::conway::{self, Redeemer, RedeemerTag, Redeemers, Tx};
use pallas_primitives::{Fragment, NonEmptySet};
use pallas_traverse::ComputeHash;
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

use super::blockfrost::BlockfrostClient;
use super::plutus_data;
use super::script_refs;
use super::tx_builder::{
    inject_metadata, parse_tx_hash, sign_raw_tx, TxBuildError, MIN_UTXO_LOVELACE, TTL_OFFSET,
};

/// Minimum ADA for a script UTxO with inline datum (~3 ADA).
const MIN_SCRIPT_UTXO_LOVELACE: u64 = 3_000_000;

/// Estimated size of a Plutus governance tx for fee estimation.
const ESTIMATED_PLUTUS_TX_SIZE: u64 = 1200;

/// Result of building a governance transaction.
#[derive(Debug)]
pub struct GovTxResult {
    /// Signed transaction CBOR bytes ready for submission.
    pub tx_cbor: Vec<u8>,
    /// Transaction hash (32 bytes, hex-encoded).
    pub tx_hash: String,
}

/// Derive a script address from a script hash for preprod testnet.
///
/// Uses network_id = 0 (testnet). The address is an enterprise script
/// address (type 7 header: 0x70 for testnet).
pub fn script_address(script_hash: &str) -> Result<PallasAddress, TxBuildError> {
    let hash_bytes = hex::decode(script_hash)
        .map_err(|e| TxBuildError::AddressParse(format!("invalid script hash hex: {e}")))?;

    // Enterprise script address for testnet: header byte 0x70 + 28-byte script hash
    let mut addr_bytes = Vec::with_capacity(29);
    addr_bytes.push(0x70); // type 7 (script) + network 0 (testnet)
    addr_bytes.extend_from_slice(&hash_bytes);

    PallasAddress::from_bytes(&addr_bytes)
        .map_err(|e| TxBuildError::AddressParse(format!("invalid script address: {e}")))
}

/// Inject Plutus V3 fields into a built transaction.
///
/// Sets reference inputs, collateral, redeemers, and optionally an inline
/// datum on the first output. This extends the decode-modify-reencode
/// pattern from `tx_builder::inject_metadata`.
pub fn inject_plutus_fields(
    tx_bytes: &[u8],
    reference_inputs: &[([u8; 32], u64)],
    collateral_inputs: &[([u8; 32], u64)],
    redeemer_cbor: &[u8],
    inline_datum_cbor: Option<&[u8]>,
) -> Result<(Vec<u8>, [u8; 32]), TxBuildError> {
    let mut tx =
        Tx::decode_fragment(tx_bytes).map_err(|e| TxBuildError::TxDecode(e.to_string()))?;

    // Set reference inputs (CIP-31/CIP-33)
    if !reference_inputs.is_empty() {
        let ref_inputs: Vec<conway::TransactionInput> = reference_inputs
            .iter()
            .map(|(hash, idx)| conway::TransactionInput {
                transaction_id: pallas_crypto::hash::Hash::new(*hash),
                index: *idx,
            })
            .collect();
        tx.transaction_body.reference_inputs = Some(
            NonEmptySet::from_vec(ref_inputs)
                .ok_or_else(|| TxBuildError::Cbor("empty reference inputs".into()))?,
        );
    }

    // Set collateral inputs
    if !collateral_inputs.is_empty() {
        let collateral: Vec<conway::TransactionInput> = collateral_inputs
            .iter()
            .map(|(hash, idx)| conway::TransactionInput {
                transaction_id: pallas_crypto::hash::Hash::new(*hash),
                index: *idx,
            })
            .collect();
        tx.transaction_body.collateral = NonEmptySet::from_vec(collateral);
    }

    // Set redeemers in the witness set
    if !redeemer_cbor.is_empty() {
        let redeemer = Redeemer {
            tag: RedeemerTag::Spend,
            index: 0,
            data: conway::PlutusData::decode_fragment(redeemer_cbor)
                .map_err(|e| TxBuildError::Cbor(format!("redeemer decode: {e}")))?,
            ex_units: conway::ExUnits {
                mem: 500_000, // initial estimate, refined by evaluate_tx
                steps: 200_000_000,
            },
        };
        tx.transaction_witness_set.redeemer =
            Some(Redeemers::List(MaybeIndefArray::Def(vec![redeemer])));
    }

    // Inject inline datum on the first output (PostAlonzo only)
    if let Some(datum_cbor) = inline_datum_cbor {
        if !datum_cbor.is_empty() {
            let datum = conway::PlutusData::decode_fragment(datum_cbor)
                .map_err(|e| TxBuildError::Cbor(format!("datum decode: {e}")))?;
            if let Some(conway::PseudoTransactionOutput::PostAlonzo(ref mut post_alonzo)) =
                tx.transaction_body.outputs.first_mut()
            {
                post_alonzo.datum_option = Some(conway::DatumOption::Data(
                    pallas_codec::utils::CborWrap(datum),
                ));
            }
        }
    }

    // Re-encode
    let new_tx_bytes = tx
        .encode_fragment()
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    let new_tx_hash = *tx.transaction_body.compute_hash();

    Ok((new_tx_bytes, new_tx_hash))
}

/// Common flow for building a Plutus governance transaction.
///
/// Steps: query chain state -> build skeleton -> inject plutus fields ->
/// evaluate ex-units -> rebuild -> inject metadata -> sign
#[allow(clippy::too_many_arguments)]
async fn build_gov_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    script_hash: &str,
    ref_utxo: (&str, u64),
    datum_cbor: &[u8],
    redeemer_cbor: &[u8],
    metadata: Option<
        pallas_codec::utils::KeyValuePairs<
            pallas_primitives::MetadatumLabel,
            pallas_primitives::Metadatum,
        >,
    >,
    mint_asset: Option<(Hash<28>, Vec<u8>, i64)>, // (policy_id, asset_name, quantity)
) -> Result<GovTxResult, TxBuildError> {
    // 1. Query chain state (parallel)
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

    // 2. Parse addresses
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let script_addr = script_address(script_hash)?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;

    // 3. Calculate fees
    let fee = params
        .calculate_min_fee(ESTIMATED_PLUTUS_TX_SIZE)
        .max(400_000);
    let input_lovelace = selected.lovelace();
    let needed = MIN_SCRIPT_UTXO_LOVELACE + fee;
    if input_lovelace < needed {
        return Err(TxBuildError::InsufficientFunds {
            needed,
            available: input_lovelace,
        });
    }
    let change = input_lovelace - MIN_SCRIPT_UTXO_LOVELACE - fee;

    // 4. Build transaction skeleton
    let mut staging_tx = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        // Output 0: script UTxO with inline datum
        .output(Output::new(script_addr, MIN_SCRIPT_UTXO_LOVELACE))
        // Output 1: change back to sender
        .output(Output::new(pallas_addr, change))
        .disclosed_signer(Hash::<28>::from(*payment_key_hash))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0);

    // Add minting if needed
    if let Some((policy_id, ref asset_name, quantity)) = mint_asset {
        staging_tx = staging_tx
            .mint_asset(policy_id, asset_name.clone(), quantity)
            .map_err(|e| TxBuildError::Builder(e.to_string()))?;
    }

    let built_tx = staging_tx
        .build_conway_raw()
        .map_err(|e| TxBuildError::Builder(e.to_string()))?;

    // 5. Inject Plutus fields (reference inputs, collateral, redeemers, inline datum)
    let ref_utxo_hash = hex::decode(ref_utxo.0)
        .map_err(|e| TxBuildError::TxDecode(format!("invalid ref utxo hash: {e}")))?;
    let ref_hash: [u8; 32] = ref_utxo_hash
        .try_into()
        .map_err(|_| TxBuildError::TxDecode("ref utxo hash must be 32 bytes".into()))?;

    let collateral_hash = selected.tx_hash.clone();
    let coll_bytes = hex::decode(&collateral_hash)
        .map_err(|e| TxBuildError::TxDecode(format!("invalid collateral hash: {e}")))?;
    let coll_hash: [u8; 32] = coll_bytes
        .try_into()
        .map_err(|_| TxBuildError::TxDecode("collateral hash must be 32 bytes".into()))?;

    let (tx_with_plutus, _) = inject_plutus_fields(
        &built_tx.tx_bytes.0,
        &[(ref_hash, ref_utxo.1)],
        &[(coll_hash, selected.tx_index)],
        redeemer_cbor,
        Some(datum_cbor),
    )?;

    // 6. Inject metadata if provided
    let tx_bytes = if let Some(meta) = metadata {
        let (tx_with_meta, _) = inject_metadata(&tx_with_plutus, meta)?;
        tx_with_meta
    } else {
        tx_with_plutus
    };

    // 7. Evaluate execution units and patch redeemers with actual values
    let tx_bytes_final = match blockfrost.evaluate_tx(&tx_bytes).await {
        Ok(units) if !units.is_empty() => {
            log::info!("Plutus execution units: {:?}", units);
            let mut tx = Tx::decode_fragment(&tx_bytes)
                .map_err(|e| TxBuildError::TxDecode(e.to_string()))?;
            if let Some(Redeemers::List(list)) = tx.transaction_witness_set.redeemer.take() {
                let mut vec: Vec<Redeemer> = list.into();
                for (i, rdmr) in vec.iter_mut().enumerate() {
                    if let Some(&(mem, steps)) = units.get(i) {
                        rdmr.ex_units = conway::ExUnits { mem, steps };
                    }
                }
                tx.transaction_witness_set.redeemer =
                    Some(Redeemers::List(MaybeIndefArray::Def(vec)));
            }
            tx.encode_fragment()
                .map_err(|e| TxBuildError::Cbor(e.to_string()))?
        }
        Ok(_) => tx_bytes,
        Err(e) => {
            log::warn!("Failed to evaluate tx (using estimates): {e}");
            tx_bytes
        }
    };

    // 8. Sign
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed_tx_bytes = sign_raw_tx(&tx_bytes_final, &private_key)?;

    // Compute tx hash
    let tx =
        Tx::decode_fragment(&signed_tx_bytes).map_err(|e| TxBuildError::TxDecode(e.to_string()))?;
    let tx_hash = hex::encode(tx.transaction_body.compute_hash().as_ref());

    Ok(GovTxResult {
        tx_cbor: signed_tx_bytes,
        tx_hash,
    })
}

/// Build a plain output-creation tx that pays a script address with an
/// inline datum, returning the UNSIGNED CBOR. No Plutus machinery
/// (collateral / redeemer / reference input / language view) is needed:
/// creating an output *at* a script address runs no validator — the
/// validator only runs when that output is later spent. Used to
/// bootstrap state UTxOs (e.g. the initial election UTxO) that spend
/// flows then consume.
async fn build_plain_create_unsigned(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    script_hash: &str,
    lovelace: u64,
    datum_cbor: &[u8],
) -> Result<Vec<u8>, TxBuildError> {
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

    let selected = BlockfrostClient::select_utxo(&utxos, lovelace + 1_000_000).ok_or(
        TxBuildError::InsufficientFunds {
            needed: lovelace + 1_000_000,
            available: utxos.iter().map(|u| u.lovelace()).sum(),
        },
    )?;
    let change_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let script_addr = script_address(script_hash)?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;
    let input_lovelace = selected.lovelace();

    let build = |fee: u64| -> Result<Vec<u8>, TxBuildError> {
        let change =
            input_lovelace
                .checked_sub(lovelace + fee)
                .ok_or(TxBuildError::InsufficientFunds {
                    needed: lovelace + fee,
                    available: input_lovelace,
                })?;
        let staging = StagingTransaction::new()
            .input(Input::new(input_tx_hash, selected.tx_index))
            .output(
                Output::new(script_addr.clone(), lovelace).set_inline_datum(datum_cbor.to_vec()),
            )
            .output(Output::new(change_addr.clone(), change))
            .fee(fee)
            .invalid_from_slot(tip_slot + TTL_OFFSET)
            .network_id(0);
        Ok(staging
            .build_conway_raw()
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .tx_bytes
            .0)
    };

    // Draft to measure the real serialized size (the inline datum can be
    // large), then size the fee off it with a witness allowance.
    let draft = build(200_000)?;
    let tx_size = draft.len() as u64 + 150; // +1 vkey witness
    let fee = params.min_fee_a * tx_size + params.min_fee_b + 5_000;
    build(fee)
}

// ---- DAO Transaction Builders ----

/// Build a CreateDao transaction.
///
/// Mints a DAO state token via the dao_minting policy and creates the
/// initial DAO UTxO at the dao_registry script address with inline datum.
#[allow(clippy::too_many_arguments)]
pub async fn build_create_dao_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    scope_type: &str,
    scope_id: &[u8],
    committee: &[&[u8; 28]],
    committee_size: i64,
    election_interval_ms: i64,
) -> Result<GovTxResult, TxBuildError> {
    if !validators_deployed() {
        return Err(TxBuildError::Cbor(
            "CreateDao: validators not yet deployed as reference scripts".into(),
        ));
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let datum = plutus_data::encode_dao_datum(
        scope_type,
        scope_id,
        &hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH)?,
        &[],
        "remember",
        &hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH)?,
        committee,
        committee_size,
        election_interval_ms,
        now_ms,
        now_ms + election_interval_ms,
    )?;

    let redeemer = plutus_data::encode_dao_redeemer("create", None)?;

    // The dao_minting policy requires: tx signed by authorized_admin,
    // exactly +1 token minted, and that token sent to the dao_registry
    // script. State-token name = "dao" ++ scope_id (see dao_registry.ak
    // get_dao_token_name). The minted token + DAO datum land at the
    // registry address; the admin is the funding wallet (payment_key).
    let mut asset_name = b"dao".to_vec();
    asset_name.extend_from_slice(scope_id);

    crate::cardano::plutus_mint::build_mint_to_address_tx(
        blockfrost,
        crate::cardano::plutus_mint::MintToAddress {
            payment_address,
            payment_key_extended,
            required_signers: std::slice::from_ref(payment_key_hash),
            policy_id: Hash::<28>::from(hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH)?),
            asset_name,
            mint_redeemer: redeemer,
            ref_script: script_refs::DAO_MINTING_REF_UTXO,
            recipient_address: script_address(script_refs::DAO_REGISTRY_SCRIPT_HASH)?,
            recipient_lovelace: MIN_SCRIPT_UTXO_LOVELACE,
            recipient_datum: Some(datum),
        },
    )
    .await
}

/// Build an OpenElection (bootstrap) transaction.
///
/// Creates the initial election UTxO at the election script address with
/// an inline `ElectionDatum` in the `Nomination` phase and an empty
/// nominee list. This is a plain output-creation: no validator runs when
/// an output is *created* at a script address, so creation is
/// permissionless (any funded wallet works) and needs no
/// collateral/redeemer/reference-script. The election validator only runs
/// once this UTxO is spent (AcceptNomination / StartVoting / Finalize).
///
/// `dao_token_name` is the DAO state token name (`"dao" ++ scope_id`);
/// the spend transitions reference the DAO UTxO bearing this token to
/// authorize phase changes, so it is baked into the datum here.
#[allow(clippy::too_many_arguments)]
pub async fn build_open_election_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    dao_policy: &[u8; 28],
    dao_token_name: &[u8],
    election_id: i64,
    seats: i64,
    nominee_min_proficiency: &str,
    voter_min_proficiency: &str,
    nomination_end_ms: i64,
    voting_end_ms: i64,
) -> Result<GovTxResult, TxBuildError> {
    let _ = payment_key_hash;
    if !validators_deployed() {
        return Err(TxBuildError::Cbor(
            "OpenElection: validators not yet deployed".into(),
        ));
    }

    let datum = plutus_data::encode_election_datum(
        dao_policy,
        dao_token_name,
        election_id,
        "nomination",
        seats,
        nominee_min_proficiency,
        voter_min_proficiency,
        &[],
        &hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH)?,
        &[],
        nomination_end_ms,
        voting_end_ms,
        &hash_from_hex(script_refs::VOTE_MINTING_SCRIPT_HASH)?,
    )?;

    let unsigned = build_plain_create_unsigned(
        blockfrost,
        payment_address,
        script_refs::ELECTION_SCRIPT_HASH,
        MIN_SCRIPT_UTXO_LOVELACE,
        &datum,
    )
    .await?;

    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed = sign_raw_tx(&unsigned, &private_key)?;
    let tx_hash = super::tx_builder::compute_tx_hash(&signed)?;
    Ok(GovTxResult {
        tx_cbor: signed,
        tx_hash,
    })
}

/// Build a CastVote transaction (election or proposal) with vote receipt mint.
pub async fn build_cast_vote_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    target_type: &str,
    vote_for: Option<bool>,
) -> Result<GovTxResult, TxBuildError> {
    if !validators_deployed() {
        return Err(TxBuildError::Cbor(
            "CastVote: validators not yet deployed".into(),
        ));
    }

    let (script_hash, ref_utxo, redeemer) = match target_type {
        "election" => (
            script_refs::ELECTION_SCRIPT_HASH,
            script_refs::ELECTION_REF_UTXO,
            plutus_data::encode_election_redeemer("accept_nomination", Some(0))?,
        ),
        "proposal" => (
            script_refs::PROPOSAL_SCRIPT_HASH,
            script_refs::PROPOSAL_REF_UTXO,
            plutus_data::encode_proposal_redeemer("vote", vote_for)?,
        ),
        _ => {
            return Err(TxBuildError::Cbor(format!(
                "unknown vote target type: {target_type}"
            )))
        }
    };

    let _receipt_redeemer = plutus_data::encode_vote_receipt_redeemer("mint")?;
    // Use empty datum for voting (state UTxO is consumed and recreated)
    let empty_datum = vec![0xd8, 0x79, 0x80]; // Constr(0, [])

    build_gov_tx(
        blockfrost,
        payment_address,
        payment_key_hash,
        payment_key_extended,
        script_hash,
        ref_utxo,
        &empty_datum,
        &redeemer,
        None,
        None,
    )
    .await
}

/// Build a ResolveProposal transaction.
pub async fn build_resolve_proposal_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
) -> Result<GovTxResult, TxBuildError> {
    if !validators_deployed() {
        return Err(TxBuildError::Cbor(
            "ResolveProposal: validators not yet deployed".into(),
        ));
    }

    let redeemer = plutus_data::encode_proposal_redeemer("resolve", None)?;
    let empty_datum = vec![0xd8, 0x79, 0x80];

    build_gov_tx(
        blockfrost,
        payment_address,
        payment_key_hash,
        payment_key_extended,
        script_refs::PROPOSAL_SCRIPT_HASH,
        script_refs::PROPOSAL_REF_UTXO,
        &empty_datum,
        &redeemer,
        None,
        None,
    )
    .await
}

/// Build a FinalizeElection transaction.
pub async fn build_finalize_election_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
) -> Result<GovTxResult, TxBuildError> {
    if !validators_deployed() {
        return Err(TxBuildError::Cbor(
            "FinalizeElection: validators not yet deployed".into(),
        ));
    }

    let redeemer = plutus_data::encode_election_redeemer("finalize", None)?;
    let empty_datum = vec![0xd8, 0x79, 0x80];

    build_gov_tx(
        blockfrost,
        payment_address,
        payment_key_hash,
        payment_key_extended,
        script_refs::ELECTION_SCRIPT_HASH,
        script_refs::ELECTION_REF_UTXO,
        &empty_datum,
        &redeemer,
        None,
        None,
    )
    .await
}

/// Build an InstallCommittee transaction.
pub async fn build_install_committee_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    election_ref: (&[u8], u64),
) -> Result<GovTxResult, TxBuildError> {
    if !validators_deployed() {
        return Err(TxBuildError::Cbor(
            "InstallCommittee: validators not yet deployed".into(),
        ));
    }

    let redeemer = plutus_data::encode_dao_redeemer(
        "install_committee",
        Some((election_ref.0, election_ref.1)),
    )?;
    let empty_datum = vec![0xd8, 0x79, 0x80];

    build_gov_tx(
        blockfrost,
        payment_address,
        payment_key_hash,
        payment_key_extended,
        script_refs::DAO_REGISTRY_SCRIPT_HASH,
        script_refs::DAO_REGISTRY_REF_UTXO,
        &empty_datum,
        &redeemer,
        None,
        None,
    )
    .await
}

/// Check if governance validators have been deployed as reference scripts.
pub fn validators_deployed() -> bool {
    script_refs::ref_utxos_deployed()
}

/// Parse a 28-byte script hash from hex.
pub(crate) fn hash_from_hex_pub(hex_str: &str) -> Result<[u8; 28], TxBuildError> {
    hash_from_hex(hex_str)
}

fn hash_from_hex(hex_str: &str) -> Result<[u8; 28], TxBuildError> {
    let bytes =
        hex::decode(hex_str).map_err(|e| TxBuildError::Cbor(format!("invalid hash hex: {e}")))?;
    bytes
        .try_into()
        .map_err(|_| TxBuildError::Cbor("hash must be 28 bytes".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_address_produces_valid_testnet_address() {
        let addr = script_address(script_refs::DAO_REGISTRY_SCRIPT_HASH).unwrap();
        // Should be a 29-byte address (1 header + 28 hash)
        let bytes = addr.to_vec();
        assert_eq!(bytes.len(), 29);
        // Header byte 0x70 = script enterprise address on testnet
        assert_eq!(bytes[0], 0x70);
    }

    #[test]
    fn inject_plutus_fields_sets_reference_inputs() {
        // Build a minimal valid tx to test injection
        let staging = StagingTransaction::new()
            .input(Input::new(Hash::new([0xAA; 32]), 0))
            .output(Output::new(
                PallasAddress::from_bytes(&[0x70; 29]).unwrap(),
                3_000_000,
            ))
            .fee(200_000)
            .network_id(0);
        let built = staging.build_conway_raw().unwrap();

        let ref_hash = [0xBB; 32];
        let coll_hash = [0xCC; 32];
        let redeemer = vec![0xd8, 0x79, 0x80]; // Constr(0, [])

        let (result_bytes, _hash) = inject_plutus_fields(
            &built.tx_bytes.0,
            &[(ref_hash, 0)],
            &[(coll_hash, 1)],
            &redeemer,
            None,
        )
        .unwrap();

        // Decode and verify reference inputs were set
        let tx = Tx::decode_fragment(&result_bytes).unwrap();
        assert!(tx.transaction_body.reference_inputs.is_some());
        assert!(tx.transaction_body.collateral.is_some());
        assert!(tx.transaction_witness_set.redeemer.is_some());
    }

    #[test]
    fn hash_from_hex_parses_28_byte_hash() {
        let result = hash_from_hex(script_refs::DAO_REGISTRY_SCRIPT_HASH);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 28);
    }

    #[test]
    fn hash_from_hex_rejects_wrong_length() {
        let result = hash_from_hex("aabb");
        assert!(result.is_err());
    }

    #[test]
    fn validators_deployed_reflects_script_refs() {
        // Governance reference scripts were deployed to preprod
        // 2026-05-22 (see script_refs), so the gate now opens.
        assert!(validators_deployed());
    }

    #[test]
    fn inject_plutus_fields_sets_inline_datum() {
        let staging = StagingTransaction::new()
            .input(Input::new(Hash::new([0xAA; 32]), 0))
            .output(Output::new(
                PallasAddress::from_bytes(&[0x70; 29]).unwrap(),
                3_000_000,
            ))
            .fee(200_000)
            .network_id(0);
        let built = staging.build_conway_raw().unwrap();

        // Constr(0, [Int(42)]) as PlutusData CBOR
        let datum_cbor = vec![0xd8, 0x79, 0x81, 0x18, 0x2a];
        let redeemer = vec![0xd8, 0x79, 0x80]; // Constr(0, [])

        let (result_bytes, _) = inject_plutus_fields(
            &built.tx_bytes.0,
            &[([0xBB; 32], 0)],
            &[([0xCC; 32], 1)],
            &redeemer,
            Some(&datum_cbor),
        )
        .unwrap();

        let tx = Tx::decode_fragment(&result_bytes).unwrap();
        let first_output = tx.transaction_body.outputs.first().unwrap();
        match first_output {
            conway::PseudoTransactionOutput::PostAlonzo(post) => {
                assert!(
                    post.datum_option.is_some(),
                    "inline datum should be set on first output"
                );
            }
            _ => panic!("expected PostAlonzo output"),
        }
    }

    #[test]
    fn redeemer_ex_units_can_be_patched() {
        // Build a tx with a redeemer
        let staging = StagingTransaction::new()
            .input(Input::new(Hash::new([0xAA; 32]), 0))
            .output(Output::new(
                PallasAddress::from_bytes(&[0x70; 29]).unwrap(),
                3_000_000,
            ))
            .fee(200_000)
            .network_id(0);
        let built = staging.build_conway_raw().unwrap();

        let redeemer_cbor = vec![0xd8, 0x79, 0x80]; // Constr(0, [])
        let (tx_bytes, _) =
            inject_plutus_fields(&built.tx_bytes.0, &[], &[], &redeemer_cbor, None).unwrap();

        // Patch with new ex-units
        let mut tx = Tx::decode_fragment(&tx_bytes).unwrap();
        if let Some(Redeemers::List(list)) = tx.transaction_witness_set.redeemer.take() {
            let mut vec: Vec<Redeemer> = list.into();
            for rdmr in vec.iter_mut() {
                rdmr.ex_units = conway::ExUnits {
                    mem: 1_000_000,
                    steps: 500_000_000,
                };
            }
            tx.transaction_witness_set.redeemer = Some(Redeemers::List(MaybeIndefArray::Def(vec)));
        }
        let patched = tx.encode_fragment().unwrap();

        // Verify the patched tx decodes with new units
        let tx2 = Tx::decode_fragment(&patched).unwrap();
        if let Some(Redeemers::List(ref list)) = tx2.transaction_witness_set.redeemer {
            let rdmr = list.first().unwrap();
            assert_eq!(rdmr.ex_units.mem, 1_000_000);
            assert_eq!(rdmr.ex_units.steps, 500_000_000);
        } else {
            panic!("expected redeemers list");
        }
    }

    /// Live preprod DAO-create: builds the UNSIGNED mint tx and prints
    /// its CBOR. The deployed dao_minting admin is the treasury
    /// cardano-cli key (normal Ed25519), so the witness is attached
    /// out-of-band by the test harness (cardano-cli), not the app's
    /// extended key. Run:
    ///   BLOCKFROST_PROJECT_ID=preprod… cargo test -p alexandria-node \
    ///     live_dao_create_unsigned -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_dao_create_unsigned() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let admin_pkh: [u8; 28] =
            hash_from_hex("6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333")
                .expect("admin pkh");
        let scope_id: &[u8] = b"d1";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let interval = 2_592_000_000i64;
        let datum = plutus_data::encode_dao_datum(
            "dao_design",
            scope_id,
            &hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH).unwrap(),
            &[],
            "remember",
            &hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH).unwrap(),
            &[&admin_pkh],
            1,
            interval,
            now,
            now + interval,
        )
        .unwrap();
        let redeemer = plutus_data::encode_dao_redeemer("create", None).unwrap();
        let mut asset_name = b"dao".to_vec();
        asset_name.extend_from_slice(scope_id);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let unsigned = rt.block_on(async {
            let bf = BlockfrostClient::new(pid).unwrap();
            crate::cardano::plutus_mint::build_mint_to_address_unsigned(
                &bf,
                &crate::cardano::plutus_mint::MintToAddress {
                    payment_address: treasury_addr,
                    payment_key_extended: &[0u8; 64],
                    required_signers: std::slice::from_ref(&admin_pkh),
                    policy_id: Hash::<28>::from(
                        hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH).unwrap(),
                    ),
                    asset_name,
                    mint_redeemer: redeemer,
                    ref_script: script_refs::DAO_MINTING_REF_UTXO,
                    recipient_address: script_address(script_refs::DAO_REGISTRY_SCRIPT_HASH)
                        .unwrap(),
                    recipient_lovelace: 3_000_000,
                    recipient_datum: Some(datum),
                },
            )
            .await
            .expect("build unsigned dao tx")
        });
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }

    /// Live preprod election bootstrap: builds the UNSIGNED plain-create
    /// tx that lands the initial election UTxO (Nomination phase, empty
    /// nominees) at the election script address with its inline datum.
    /// Creation is permissionless, but we fund + sign with the treasury
    /// cardano-cli key out-of-band (same harness as the DAO create). The
    /// DAO state token is `daod1` under policy DAO_MINTING (the verified
    /// DAO from tx d554c635). Run:
    ///   BLOCKFROST_PROJECT_ID=preprod… cargo test -p alexandria-node \
    ///     live_election_bootstrap_unsigned -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_election_bootstrap_unsigned() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let dao_policy = hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH).unwrap();
        let dao_token_name = b"daod1".to_vec();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let datum = plutus_data::encode_election_datum(
            &dao_policy,
            &dao_token_name,
            1,
            "nomination",
            1,
            "remember",
            "remember",
            &[],
            &hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH).unwrap(),
            &[],
            now + 86_400_000,
            now + 172_800_000,
            &hash_from_hex(script_refs::VOTE_MINTING_SCRIPT_HASH).unwrap(),
        )
        .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let unsigned = rt.block_on(async {
            let bf = BlockfrostClient::new(pid).unwrap();
            build_plain_create_unsigned(
                &bf,
                treasury_addr,
                script_refs::ELECTION_SCRIPT_HASH,
                MIN_SCRIPT_UTXO_LOVELACE,
                &datum,
            )
            .await
            .expect("build unsigned election tx")
        });
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }

    /// Live preprod election lifecycle driver. One ignored test, one
    /// `STEP` env var, so a shell harness can drive the full sequence
    /// (each step spends the previous step's continuing output and is
    /// signed out-of-band by the treasury cardano-cli key):
    ///   bootstrap → nominate → start_voting → finalize
    ///
    /// Env:
    ///   STEP               bootstrap | nominate | start_voting | finalize
    ///   T_NOM_MS, T_VOT_MS nomination/voting deadlines (POSIX ms). The
    ///                      bootstrap step prints fresh values; later
    ///                      steps must be passed the SAME values.
    ///   ELECTION_UTXO      `txhash#idx` of the election UTxO to spend
    ///                      (non-bootstrap steps).
    ///   ELECTION_LOVELACE  lovelace on that UTxO.
    ///
    /// The self-nominee is the treasury key, which owns the reputation
    /// reference NFT (subject 72657031…, skill proficiency Analyze) at
    /// the soulbound UTxO referenced below. Run e.g.:
    ///   STEP=bootstrap BLOCKFROST_PROJECT_ID=… cargo test -p \
    ///     alexandria-node live_election_step -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_election_step() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let step = std::env::var("STEP").expect("STEP");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let nominee =
            hash_from_hex("6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333").unwrap();
        let dao_policy = hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH).unwrap();
        let dao_token = b"daod1".to_vec();
        let subject = hex::decode("72657031000000000000000000000000").unwrap();
        let rep_policy = hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH).unwrap();
        let vote_policy = hash_from_hex(script_refs::VOTE_MINTING_SCRIPT_HASH).unwrap();
        let election_id = 7i64;
        let seats = 1i64;
        // Soulbound UTxO holding the treasury's reputation reference NFT.
        let rep_ref = (
            "3eb0620bdd0c5a124c9ce7212295fe7b34b0933174cfed20b05b48c0bbc19a39",
            0u64,
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let bf = BlockfrostClient::new(pid).unwrap();

        if step == "bootstrap" {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let t_nom = now + 600_000;
            let t_vot = now + 1_200_000;
            let datum = plutus_data::encode_election_datum(
                &dao_policy,
                &dao_token,
                election_id,
                "nomination",
                seats,
                "remember",
                "remember",
                &[&subject[..]],
                &rep_policy,
                &[],
                t_nom,
                t_vot,
                &vote_policy,
            )
            .unwrap();
            let unsigned = rt
                .block_on(build_plain_create_unsigned(
                    &bf,
                    treasury_addr,
                    script_refs::ELECTION_SCRIPT_HASH,
                    MIN_SCRIPT_UTXO_LOVELACE,
                    &datum,
                ))
                .expect("build bootstrap");
            println!("T_NOM_MS={t_nom}");
            println!("T_VOT_MS={t_vot}");
            println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
            return;
        }

        let t_nom: i64 = std::env::var("T_NOM_MS").unwrap().parse().unwrap();
        let t_vot: i64 = std::env::var("T_VOT_MS").unwrap().parse().unwrap();

        // Validity-range bounds must be SLOTS the ledger can convert to
        // POSIX time. Preprod's Byron era ran 20s slots, so an absolute
        // posix→slot conversion (PREPROD_SHELLEY_EPOCH_START) overshoots
        // by ~16 days and trips PastHorizon. Derive bounds from the live
        // chain tip instead: in the post-Shelley 1s-slot era, tip + N
        // seconds ≈ the slot N seconds from now.
        let tip = rt.block_on(bf.get_tip_slot()).expect("tip");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let secs_to = |deadline_ms: i64| -> i64 { (deadline_ms - now_ms) / 1000 };
        let before_slot =
            |deadline_ms: i64| -> u64 { (tip as i64 + secs_to(deadline_ms) - 60).max(1) as u64 };
        let after_slot =
            |deadline_ms: i64| -> u64 { (tip as i64 + secs_to(deadline_ms) + 60).max(1) as u64 };

        let eu = std::env::var("ELECTION_UTXO").unwrap();
        let (eh, ei_s) = eu.split_once('#').unwrap();
        let ei: u64 = ei_s.parse().unwrap();
        let elov: u64 = std::env::var("ELECTION_LOVELACE").unwrap().parse().unwrap();

        let datum = |phase: &str, noms: &[(&[u8; 28], bool)]| {
            plutus_data::encode_election_datum(
                &dao_policy,
                &dao_token,
                election_id,
                phase,
                seats,
                "remember",
                "remember",
                &[&subject[..]],
                &rep_policy,
                noms,
                t_nom,
                t_vot,
                &vote_policy,
            )
            .unwrap()
        };

        let (redeemer, cont_datum, refs, inval, valfrom): (
            Vec<u8>,
            Vec<u8>,
            Vec<(&str, u64)>,
            Option<u64>,
            Option<u64>,
        ) = match step.as_str() {
            "nominate" => (
                plutus_data::encode_election_redeemer("nominate", None).unwrap(),
                datum("nomination", &[(&nominee, true)]),
                vec![script_refs::ELECTION_REF_UTXO, rep_ref],
                Some(before_slot(t_nom)),
                None,
            ),
            "start_voting" => (
                plutus_data::encode_election_redeemer("start_voting", None).unwrap(),
                datum("voting", &[(&nominee, true)]),
                vec![script_refs::ELECTION_REF_UTXO],
                None,
                Some(after_slot(t_nom)),
            ),
            "finalize" => (
                plutus_data::encode_election_finalize_redeemer(&[&nominee]).unwrap(),
                datum("finalized", &[(&nominee, true)]),
                vec![script_refs::ELECTION_REF_UTXO],
                None,
                Some(after_slot(t_vot)),
            ),
            other => panic!("unknown STEP: {other}"),
        };

        let signers = [nominee];
        let unsigned = rt
            .block_on(crate::cardano::plutus_spend::build_spend_unsigned(
                &bf,
                &crate::cardano::plutus_spend::SpendScript {
                    payment_address: treasury_addr,
                    payment_key_extended: &[0u8; 64],
                    required_signers: &signers,
                    script_input: (eh, ei),
                    script_input_lovelace: elov,
                    spend_redeemer: redeemer,
                    continuing_address: script_address(script_refs::ELECTION_SCRIPT_HASH).unwrap(),
                    continuing_lovelace: MIN_SCRIPT_UTXO_LOVELACE,
                    continuing_datum: cont_datum,
                    continuing_assets: &[],
                    reference_inputs: &refs,
                    mint: None,
                    invalid_from_slot: inval,
                    valid_from_slot: valfrom,
                },
            ))
            .expect("build spend");
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }

    /// Live preprod proposal lifecycle driver, mirroring
    /// `live_election_step`:
    ///   bootstrap (Draft) → approve (committee, Draft→Published) →
    ///   vote (tally +1) → resolve (after deadline, Published→Approved)
    ///
    /// Env:
    ///   STEP             bootstrap | approve | vote | resolve
    ///   VOTE_DEADLINE_MS proposal voting deadline (POSIX ms); the
    ///                    `approve` step prints a fresh value, later steps
    ///                    must be passed the SAME value.
    ///   PROPOSAL_UTXO    `txhash#idx` of the proposal UTxO to spend
    ///                    (non-bootstrap steps).
    ///   PROPOSAL_LOVELACE lovelace on that UTxO.
    ///
    /// The committee + voter are the treasury key (sole DAO committee
    /// member, and owner of the reputation reference NFT). The DAO state
    /// UTxO (daod1) is read as a reference input for committee/quorum.
    #[test]
    #[ignore]
    fn live_proposal_step() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let step = std::env::var("STEP").expect("STEP");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let actor =
            hash_from_hex("6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333").unwrap();
        let dao_policy = hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH).unwrap();
        let dao_token = b"daod1".to_vec();
        let subject = hex::decode("72657031000000000000000000000000").unwrap();
        let rep_policy = hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH).unwrap();
        let vote_policy = hash_from_hex(script_refs::VOTE_MINTING_SCRIPT_HASH).unwrap();
        let content_cid = b"Qmproposal1".to_vec();
        let proposal_id = 3i64;
        // DAO state UTxO (daod1) for the committee reference input.
        let dao_ref = (
            "d554c635ec17f1d6db2e8b49e7cf78fcdf042c23078a8055fa5a48814bc553ef",
            0u64,
        );
        // Soulbound UTxO holding the treasury's reputation reference NFT.
        let rep_ref = (
            "3eb0620bdd0c5a124c9ce7212295fe7b34b0933174cfed20b05b48c0bbc19a39",
            0u64,
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let bf = BlockfrostClient::new(pid).unwrap();

        let datum = |status: &str, deadline_ms: i64, vf: i64, va: i64| {
            plutus_data::encode_proposal_datum(
                &dao_policy,
                &dao_token,
                proposal_id,
                &actor,
                status,
                "general",
                "remember",
                &[&subject[..]],
                &rep_policy,
                &content_cid,
                deadline_ms,
                vf,
                va,
                &vote_policy,
            )
            .unwrap()
        };

        if step == "bootstrap" {
            // Draft proposal: voting_deadline 0, tallies 0.
            let d = datum("draft", 0, 0, 0);
            let unsigned = rt
                .block_on(build_plain_create_unsigned(
                    &bf,
                    treasury_addr,
                    script_refs::PROPOSAL_SCRIPT_HASH,
                    MIN_SCRIPT_UTXO_LOVELACE,
                    &d,
                ))
                .expect("build proposal bootstrap");
            println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
            return;
        }

        let pu = std::env::var("PROPOSAL_UTXO").unwrap();
        let (ph, pi_s) = pu.split_once('#').unwrap();
        let pi: u64 = pi_s.parse().unwrap();
        let plov: u64 = std::env::var("PROPOSAL_LOVELACE").unwrap().parse().unwrap();

        let tip = rt.block_on(bf.get_tip_slot()).expect("tip");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let before_slot =
            |ms: i64| -> u64 { (tip as i64 + (ms - now_ms) / 1000 - 60).max(1) as u64 };
        let after_slot =
            |ms: i64| -> u64 { (tip as i64 + (ms - now_ms) / 1000 + 60).max(1) as u64 };

        let (redeemer, cont_datum, refs, signers, inval, valfrom): (
            Vec<u8>,
            Vec<u8>,
            Vec<(&str, u64)>,
            Vec<[u8; 28]>,
            Option<u64>,
            Option<u64>,
        ) = match step.as_str() {
            "approve" => {
                // Draft → Published; set a voting deadline 10 min out.
                let vd = now_ms + 600_000;
                let duration = 600_000i64;
                println!("VOTE_DEADLINE_MS={vd}");
                (
                    plutus_data::encode_proposal_approve_redeemer(duration).unwrap(),
                    datum("published", vd, 0, 0),
                    vec![script_refs::PROPOSAL_REF_UTXO, dao_ref],
                    vec![actor],
                    None,
                    None,
                )
            }
            "vote" => {
                let vd: i64 = std::env::var("VOTE_DEADLINE_MS").unwrap().parse().unwrap();
                (
                    plutus_data::encode_proposal_redeemer("vote", Some(true)).unwrap(),
                    datum("published", vd, 1, 0),
                    vec![script_refs::PROPOSAL_REF_UTXO, rep_ref],
                    vec![actor],
                    Some(before_slot(vd)),
                    None,
                )
            }
            "resolve" => {
                let vd: i64 = std::env::var("VOTE_DEADLINE_MS").unwrap().parse().unwrap();
                (
                    plutus_data::encode_proposal_redeemer("resolve", None).unwrap(),
                    datum("approved", vd, 1, 0),
                    vec![script_refs::PROPOSAL_REF_UTXO],
                    vec![actor],
                    None,
                    Some(after_slot(vd)),
                )
            }
            other => panic!("unknown STEP: {other}"),
        };

        let unsigned = rt
            .block_on(crate::cardano::plutus_spend::build_spend_unsigned(
                &bf,
                &crate::cardano::plutus_spend::SpendScript {
                    payment_address: treasury_addr,
                    payment_key_extended: &[0u8; 64],
                    required_signers: &signers,
                    script_input: (ph, pi),
                    script_input_lovelace: plov,
                    spend_redeemer: redeemer,
                    continuing_address: script_address(script_refs::PROPOSAL_SCRIPT_HASH).unwrap(),
                    continuing_lovelace: MIN_SCRIPT_UTXO_LOVELACE,
                    continuing_datum: cont_datum,
                    continuing_assets: &[],
                    reference_inputs: &refs,
                    mint: None,
                    invalid_from_slot: inval,
                    valid_from_slot: valfrom,
                },
            ))
            .expect("build proposal spend");
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }

    /// Live preprod committee install: spends the DAO state UTxO with the
    /// `InstallCommittee { election_ref }` redeemer, references the
    /// finalized election as a read-only input, and writes a new DaoDatum
    /// whose committee is the election winners. Closes the governance
    /// loop election → committee. No signature is required by the
    /// validator once the election is finalized.
    ///
    /// Env: DAO_UTXO (`txhash#idx`), DAO_LOVELACE, ELECTION_REF
    /// (`txhash#idx` of the finalized election).
    #[test]
    #[ignore]
    fn live_install_committee() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let winner =
            hash_from_hex("6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333").unwrap();
        let dao_policy = hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH).unwrap();
        let rep_policy = hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH).unwrap();

        let du = std::env::var("DAO_UTXO").unwrap();
        let (dh, di_s) = du.split_once('#').unwrap();
        let di: u64 = di_s.parse().unwrap();
        let dlov: u64 = std::env::var("DAO_LOVELACE").unwrap().parse().unwrap();
        let er = std::env::var("ELECTION_REF").unwrap();
        let (erh, eri_s) = er.split_once('#').unwrap();
        let eri: u64 = eri_s.parse().unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let interval = 2_592_000_000i64;
        // New DAO datum: scope (SubjectDao "d1") and state_token_policy
        // MUST be preserved; committee becomes the winners; new term.
        let new_datum = plutus_data::encode_dao_datum(
            "subject",
            b"d1",
            &rep_policy,
            &[],
            "remember",
            &dao_policy,
            &[&winner],
            1,
            interval,
            now,
            now + interval,
        )
        .unwrap();

        // InstallCommittee { election_ref = OutputReference(erh, eri) }.
        let erh_bytes = hex::decode(erh).unwrap();
        let redeemer =
            plutus_data::encode_dao_redeemer("install_committee", Some((&erh_bytes, eri))).unwrap();

        let dao_token = b"daod1".to_vec();
        let assets = [(Hash::<28>::from(dao_policy), dao_token, 1i64)];
        let refs = vec![script_refs::DAO_REGISTRY_REF_UTXO, (erh, eri)];

        let rt = tokio::runtime::Runtime::new().unwrap();
        let bf = BlockfrostClient::new(pid).unwrap();
        let unsigned = rt
            .block_on(crate::cardano::plutus_spend::build_spend_unsigned(
                &bf,
                &crate::cardano::plutus_spend::SpendScript {
                    payment_address: treasury_addr,
                    payment_key_extended: &[0u8; 64],
                    required_signers: &[],
                    script_input: (dh, di),
                    script_input_lovelace: dlov,
                    spend_redeemer: redeemer,
                    continuing_address: script_address(script_refs::DAO_REGISTRY_SCRIPT_HASH)
                        .unwrap(),
                    continuing_lovelace: MIN_SCRIPT_UTXO_LOVELACE,
                    continuing_datum: new_datum,
                    continuing_assets: &assets,
                    reference_inputs: &refs,
                    mint: None,
                    invalid_from_slot: None,
                    valid_from_slot: None,
                },
            ))
            .expect("build install_committee");
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }

    /// Live preprod challenge-escrow lifecycle: lock a stake at the
    /// escrow script, then settle it. `Refund` returns the stake to the
    /// challenger (must be signed by the DAO authority and pay the
    /// challenger). Here challenger = treasury = dao_authority.
    ///
    /// Env: STEP=lock|refund ; ESCROW_UTXO, ESCROW_LOVELACE (refund).
    #[test]
    #[ignore]
    fn live_escrow_step() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let step = std::env::var("STEP").expect("STEP");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let actor =
            hash_from_hex("6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333").unwrap();
        const STAKE: u64 = 5_000_000;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let bf = BlockfrostClient::new(pid).unwrap();

        if step == "lock" {
            let datum =
                plutus_data::encode_challenge_escrow_datum(&actor, &actor, &actor, b"chal1")
                    .unwrap();
            let unsigned = rt
                .block_on(build_plain_create_unsigned(
                    &bf,
                    treasury_addr,
                    script_refs::CHALLENGE_ESCROW_SCRIPT_HASH,
                    STAKE,
                    &datum,
                ))
                .expect("build escrow lock");
            println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
            return;
        }

        // refund: spend the escrow UTxO, pay the challenger (treasury).
        let eu = std::env::var("ESCROW_UTXO").unwrap();
        let (eh, ei_s) = eu.split_once('#').unwrap();
        let ei: u64 = ei_s.parse().unwrap();
        let elov: u64 = std::env::var("ESCROW_LOVELACE").unwrap().parse().unwrap();
        let redeemer = plutus_data::encode_challenge_escrow_redeemer(true).unwrap();
        let pay_challenger = PallasAddress::from_bech32(treasury_addr).unwrap();
        let signers = [actor];
        let unsigned = rt
            .block_on(crate::cardano::plutus_spend::build_spend_unsigned(
                &bf,
                &crate::cardano::plutus_spend::SpendScript {
                    payment_address: treasury_addr,
                    payment_key_extended: &[0u8; 64],
                    required_signers: &signers,
                    script_input: (eh, ei),
                    script_input_lovelace: elov,
                    spend_redeemer: redeemer,
                    // "Continuing" output pays the challenger (a vkey
                    // address); the escrow keeps no on-chain state.
                    continuing_address: pay_challenger,
                    continuing_lovelace: elov,
                    continuing_datum: vec![0xd8, 0x79, 0x80],
                    continuing_assets: &[],
                    reference_inputs: &[script_refs::CHALLENGE_ESCROW_REF_UTXO],
                    mint: None,
                    invalid_from_slot: None,
                    valid_from_slot: None,
                },
            ))
            .expect("build escrow refund");
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }

    /// Live preprod soulbound transfer-guard spend: spends the CIP-68
    /// reputation reference NFT UTxO with `UpdateReputation`, returning
    /// it to the same soulbound script with the same owner/subject/role.
    /// Signed by the authorized minter (= treasury).
    ///
    /// Env: SB_UTXO (`txhash#idx`), SB_LOVELACE.
    #[test]
    #[ignore]
    fn live_soulbound_update() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let minter =
            hash_from_hex("6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333").unwrap();
        let rep_policy = hash_from_hex(script_refs::REPUTATION_MINTING_SCRIPT_HASH).unwrap();
        // (100) reference NFT asset name: cip68 label 000643b0 ++ base
        // (subject 16B "rep1.." ++ role byte 01).
        let ref_asset = hex::decode("000643b07265703100000000000000000000000001").unwrap();
        // Re-attach the identical ReputationDatum so owner/subject/role
        // are provably preserved (the validator's only datum checks).
        let datum = hex::decode(
            "d87987581c6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef383335072657031000000000000000000000000d8798081d879855000112233445566778899aabbccddeeffd87c801a000f424019232802021b0000019eef2378ca1b0000019f89a240ca",
        )
        .unwrap();

        let su = std::env::var("SB_UTXO").unwrap();
        let (sh, si_s) = su.split_once('#').unwrap();
        let si: u64 = si_s.parse().unwrap();
        let slov: u64 = std::env::var("SB_LOVELACE").unwrap().parse().unwrap();

        let redeemer = plutus_data::encode_soulbound_redeemer("update").unwrap();
        let assets = [(Hash::<28>::from(rep_policy), ref_asset, 1i64)];
        let signers = [minter];

        let rt = tokio::runtime::Runtime::new().unwrap();
        let bf = BlockfrostClient::new(pid).unwrap();
        let unsigned = rt
            .block_on(crate::cardano::plutus_spend::build_spend_unsigned(
                &bf,
                &crate::cardano::plutus_spend::SpendScript {
                    payment_address: treasury_addr,
                    payment_key_extended: &[0u8; 64],
                    required_signers: &signers,
                    script_input: (sh, si),
                    script_input_lovelace: slov,
                    spend_redeemer: redeemer,
                    continuing_address: script_address(script_refs::SOULBOUND_SCRIPT_HASH).unwrap(),
                    continuing_lovelace: slov,
                    continuing_datum: datum,
                    continuing_assets: &assets,
                    reference_inputs: &[script_refs::SOULBOUND_REF_UTXO],
                    mint: None,
                    invalid_from_slot: None,
                    valid_from_slot: None,
                },
            ))
            .expect("build soulbound update");
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }
}
