//! Shared Cardano transaction-builder primitives.
//!
//! Post-migration 040 this module no longer holds the legacy
//! SkillProof-NFT or course-registration minters — those were the
//! only users of CIP-25 metadata in-app. What remains is the generic
//! substrate used by the VC integrity-anchor flow (`anchor_tx`), the
//! soulbound reputation minter, and the DAO governance tx builder:
//!
//! * `TxBuildError`
//! * `MIN_UTXO_LOVELACE`, `MIN_NFT_LOVELACE`, `TTL_OFFSET`
//! * `inject_metadata`, `estimate_fee`
//! * `parse_tx_hash`, `sign_raw_tx`, `compute_tx_hash`

use pallas_codec::utils::KeyValuePairs;
use pallas_crypto::hash::Hash;
use pallas_primitives::conway::{AuxiliaryData, Tx};
use pallas_primitives::{Fragment, Metadatum, MetadatumLabel};
use pallas_traverse::ComputeHash;
use pallas_wallet::PrivateKey;
use thiserror::Error;

use super::types::ProtocolParameters;

#[derive(Error, Debug)]
pub enum TxBuildError {
    #[error("insufficient funds: need {needed} lovelace, have {available}")]
    InsufficientFunds { needed: u64, available: u64 },
    #[error("no UTxOs available at address")]
    NoUtxos,
    #[error("address parsing failed: {0}")]
    AddressParse(String),
    #[error("transaction builder error: {0}")]
    Builder(String),
    #[error("CBOR encoding error: {0}")]
    Cbor(String),
    #[error("transaction decoding failed: {0}")]
    TxDecode(String),
    #[error("Blockfrost error: {0}")]
    Blockfrost(#[from] super::blockfrost::BlockfrostError),
}

/// Minimum ADA to send with an NFT output (2 ADA).
///
/// Still used by `soulbound_tx_builder` for the CIP-68 reputation mint.
pub const MIN_NFT_LOVELACE: u64 = 2_000_000;

/// Minimum ADA required in a UTxO for coin selection (5 ADA).
pub const MIN_UTXO_LOVELACE: u64 = 5_000_000;

/// TTL offset from current slot (1 hour = 3600 slots on preprod).
pub const TTL_OFFSET: u64 = 3600;

/// Estimated fee floor for initial transaction sizing (300k lovelace, ~0.3 ADA).
/// The calculated linear fee is used when it exceeds this floor.
pub(crate) const ESTIMATED_FEE: u64 = 300_000;

/// Inject a metadata map into a pre-built transaction's auxiliary data.
///
/// The pallas-txbuilder doesn't support auxiliary data yet, so we:
/// 1. Decode the built tx bytes as `conway::Tx`
/// 2. Set `auxiliary_data` to `AuxiliaryData::Shelley(metadata)`
/// 3. Recompute `auxiliary_data_hash` in the transaction body
/// 4. Re-encode and recalculate the tx hash
pub fn inject_metadata(
    tx_bytes: &[u8],
    metadata: KeyValuePairs<MetadatumLabel, Metadatum>,
) -> Result<(Vec<u8>, [u8; 32]), TxBuildError> {
    let mut tx =
        Tx::decode_fragment(tx_bytes).map_err(|e| TxBuildError::TxDecode(e.to_string()))?;

    let aux_data = AuxiliaryData::Shelley(metadata);
    let aux_data_hash = aux_data.compute_hash();
    tx.auxiliary_data = pallas_primitives::Nullable::Some(aux_data);
    tx.transaction_body.auxiliary_data_hash = Some(aux_data_hash.to_vec().into());

    let new_tx_bytes = tx
        .encode_fragment()
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    let new_tx_hash = *tx.transaction_body.compute_hash();

    Ok((new_tx_bytes, new_tx_hash))
}

/// Estimate the minimum fee for a small metadata-only transaction.
///
/// Uses the linear fee formula: `min_fee_a * estimated_size + min_fee_b`,
/// clamped up to an `ESTIMATED_FEE` floor.
pub fn estimate_fee(params: &ProtocolParameters, _num_witnesses: u32) -> u64 {
    let estimated_size: u64 = 600;
    let calculated = params.calculate_min_fee(estimated_size);
    calculated.max(ESTIMATED_FEE)
}

/// Parse a hex-encoded 32-byte transaction hash into a pallas Hash.
pub fn parse_tx_hash(hex_str: &str) -> Result<Hash<32>, TxBuildError> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| TxBuildError::TxDecode(format!("invalid tx hash hex: {e}")))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| TxBuildError::TxDecode("tx hash must be 32 bytes".into()))?;
    Ok(Hash::from(arr))
}

/// Sign raw transaction CBOR bytes with a private key.
///
/// Decodes the tx, computes the body hash, signs it, adds the
/// VKeyWitness to the witness set, and re-encodes.
pub fn sign_raw_tx(tx_bytes: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>, TxBuildError> {
    let mut tx =
        Tx::decode_fragment(tx_bytes).map_err(|e| TxBuildError::TxDecode(e.to_string()))?;

    let tx_hash = tx.transaction_body.compute_hash();
    let signature = private_key.sign(*tx_hash);
    let pub_key = private_key.public_key();

    let vkey_witness = pallas_primitives::conway::VKeyWitness {
        vkey: pub_key.as_ref().to_vec().into(),
        signature: signature.as_ref().to_vec().into(),
    };

    let mut witnesses = tx
        .transaction_witness_set
        .vkeywitness
        .map(|set| set.to_vec())
        .unwrap_or_default();
    witnesses.push(vkey_witness);

    tx.transaction_witness_set.vkeywitness = pallas_primitives::NonEmptySet::from_vec(witnesses);

    tx.encode_fragment()
        .map_err(|e| TxBuildError::Cbor(e.to_string()))
}

/// Compute the transaction hash from signed CBOR bytes.
pub fn compute_tx_hash(signed_tx_bytes: &[u8]) -> Result<String, TxBuildError> {
    let tx =
        Tx::decode_fragment(signed_tx_bytes).map_err(|e| TxBuildError::TxDecode(e.to_string()))?;
    let hash = tx.transaction_body.compute_hash();
    Ok(hex::encode(hash.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pallas_addresses::Address as PallasAddress;
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};

    #[test]
    fn parse_tx_hash_valid() {
        let hex = "a".repeat(64);
        let hash = parse_tx_hash(&hex);
        assert!(hash.is_ok());
        assert_eq!(hash.unwrap().as_ref(), &[0xAA; 32]);
    }

    #[test]
    fn parse_tx_hash_invalid_length() {
        let result = parse_tx_hash("aabb");
        assert!(result.is_err());
    }

    #[test]
    fn parse_tx_hash_invalid_hex() {
        let result = parse_tx_hash(&"zz".repeat(32));
        assert!(result.is_err());
    }

    #[test]
    fn estimate_fee_reasonable() {
        let params = ProtocolParameters {
            min_fee_a: 44,
            min_fee_b: 155381,
            max_tx_size: 16384,
            coins_per_utxo_size: Some("4310".into()),
            coins_per_utxo_word: None,
            key_deposit: None,
        };
        let fee = estimate_fee(&params, 1);
        // 44 * 600 + 155381 = 181781, less than the 300_000 floor.
        assert_eq!(fee, 300_000);
    }

    #[test]
    fn inject_metadata_into_simple_tx() {
        let staging = StagingTransaction::new()
            .input(Input::new(Hash::from([0x42u8; 32]), 0))
            .output(Output::new(
                PallasAddress::from_bech32(
                    "addr_test1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp"
                ).unwrap(),
                2_000_000,
            ))
            .fee(200_000)
            .network_id(0);

        let built = staging.build_conway_raw().expect("build should succeed");

        let metadata = KeyValuePairs::from(vec![(721u64, Metadatum::Text("test".into()))]);

        let (new_bytes, new_hash) =
            inject_metadata(&built.tx_bytes.0, metadata).expect("inject should succeed");

        let decoded = Tx::decode_fragment(&new_bytes).expect("should decode");

        assert!(
            matches!(decoded.auxiliary_data, pallas_primitives::Nullable::Some(_)),
            "auxiliary_data should be set after injection"
        );

        assert!(
            decoded.transaction_body.auxiliary_data_hash.is_some(),
            "auxiliary_data_hash should be set"
        );

        assert_ne!(
            new_hash, built.tx_hash.0,
            "tx hash should change after metadata injection"
        );
    }
}
