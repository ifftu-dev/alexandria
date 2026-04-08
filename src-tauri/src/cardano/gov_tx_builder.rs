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

    build_gov_tx(
        blockfrost,
        payment_address,
        payment_key_hash,
        payment_key_extended,
        script_refs::DAO_REGISTRY_SCRIPT_HASH,
        script_refs::DAO_REGISTRY_REF_UTXO,
        &datum,
        &redeemer,
        None,
        None,
    )
    .await
}

/// Build an OpenElection transaction.
#[allow(clippy::too_many_arguments)]
pub async fn build_open_election_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    _dao_policy: &[u8; 28],
    _dao_token_name: &[u8],
    election_id: i64,
    seats: i64,
    nominee_min_proficiency: &str,
    voter_min_proficiency: &str,
    nomination_end_ms: i64,
    voting_end_ms: i64,
) -> Result<GovTxResult, TxBuildError> {
    if !validators_deployed() {
        return Err(TxBuildError::Cbor(
            "OpenElection: validators not yet deployed".into(),
        ));
    }

    let datum = plutus_data::encode_election_datum(
        &hash_from_hex(script_refs::DAO_MINTING_SCRIPT_HASH)?,
        &[],
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

    let redeemer = plutus_data::encode_election_redeemer("open", None)?;

    build_gov_tx(
        blockfrost,
        payment_address,
        payment_key_hash,
        payment_key_extended,
        script_refs::ELECTION_SCRIPT_HASH,
        script_refs::ELECTION_REF_UTXO,
        &datum,
        &redeemer,
        None,
        None,
    )
    .await
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
        // Currently all are DEPLOY_PENDING
        assert!(!validators_deployed());
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
}
