//! Shared Plutus V3 transaction helpers: script addresses, Plutus field
//! injection, and script-hash parsing. Used by the completion, soulbound,
//! mint, spend and stake-registration builders. The obsolete governance
//! transaction builders are deleted.

use pallas_addresses::Address as PallasAddress;
use pallas_codec::utils::MaybeIndefArray;
use pallas_primitives::conway::{self, Redeemer, RedeemerTag, Redeemers, Tx};
use pallas_primitives::{Fragment, NonEmptySet};
use pallas_traverse::ComputeHash;

use super::script_refs;
use super::tx_builder::TxBuildError;

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
    use pallas_crypto::hash::Hash;
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};

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
}
