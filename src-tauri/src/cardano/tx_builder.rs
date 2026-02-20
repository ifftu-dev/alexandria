use pallas_addresses::Address as PallasAddress;
use pallas_codec::minicbor;
use pallas_codec::utils::KeyValuePairs;
use pallas_crypto::hash::Hash;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_primitives::conway::{AuxiliaryData, Tx};
use pallas_primitives::{Fragment, Metadatum, MetadatumLabel};
use pallas_traverse::ComputeHash;
use pallas_txbuilder::{BuildConway, Input, Output, ScriptKind, StagingTransaction};
use pallas_wallet::PrivateKey;
use thiserror::Error;

use super::blockfrost::BlockfrostClient;
use super::policy;
use super::types::{CourseRegistrationResult, MintResult, ProtocolParameters};

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
    #[error("policy computation failed: {0}")]
    Policy(#[from] policy::PolicyError),
    #[error("Blockfrost error: {0}")]
    Blockfrost(#[from] super::blockfrost::BlockfrostError),
}

/// CIP-25 metadata label for NFTs.
const CIP25_LABEL: MetadatumLabel = 721;

/// Minimum ADA to send with an NFT output (2 ADA).
const MIN_NFT_LOVELACE: u64 = 2_000_000;

/// Minimum ADA required in a UTxO for coin selection (5 ADA).
const MIN_UTXO_LOVELACE: u64 = 5_000_000;

/// TTL offset from current slot (1 hour = 3600 slots on preprod).
const TTL_OFFSET: u64 = 3600;

/// Estimated fee for initial transaction sizing (300k lovelace, ~0.3 ADA).
/// Will be refined with actual protocol parameters.
const ESTIMATED_FEE: u64 = 300_000;

/// Build CIP-25 metadata for a SkillProof NFT.
///
/// Format per CIP-25:
/// ```json
/// { 721: { "<policy_id>": { "<asset_name>": { "name": "...", ... } } } }
/// ```
fn build_skill_proof_metadata(
    policy_id_hex: &str,
    asset_name: &str,
    proof_id: &str,
    skill_name: &str,
    proficiency_level: &str,
    confidence: f64,
    content_hash: Option<&str>,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    // Inner asset metadata map
    let mut asset_fields = vec![
        (
            Metadatum::Text("name".into()),
            Metadatum::Text(format!("Alexandria SkillProof - {}", &proof_id[..8.min(proof_id.len())])),
        ),
        (
            Metadatum::Text("description".into()),
            Metadatum::Text(format!(
                "Skill proof: {} at {} level (confidence: {:.2})",
                skill_name, proficiency_level, confidence
            )),
        ),
        (
            Metadatum::Text("mediaType".into()),
            Metadatum::Text("application/json".into()),
        ),
        (
            Metadatum::Text("proofId".into()),
            Metadatum::Text(proof_id.into()),
        ),
        (
            Metadatum::Text("skill".into()),
            Metadatum::Text(skill_name.into()),
        ),
        (
            Metadatum::Text("proficiencyLevel".into()),
            Metadatum::Text(proficiency_level.into()),
        ),
        (
            Metadatum::Text("confidence".into()),
            Metadatum::Text(format!("{:.4}", confidence)),
        ),
        (
            Metadatum::Text("version".into()),
            Metadatum::Int(1.into()),
        ),
    ];

    if let Some(hash) = content_hash {
        asset_fields.push((
            Metadatum::Text("contentHash".into()),
            Metadatum::Text(hash.into()),
        ));
    }

    let asset_meta = Metadatum::Map(KeyValuePairs::from(asset_fields));

    // { asset_name: asset_meta }
    let asset_map = Metadatum::Map(KeyValuePairs::from(vec![(
        Metadatum::Text(asset_name.into()),
        asset_meta,
    )]));

    // { policy_id: { asset_name: ... } }
    let policy_map = Metadatum::Map(KeyValuePairs::from(vec![(
        Metadatum::Text(policy_id_hex.into()),
        asset_map,
    )]));

    // { 721: { ... } }
    KeyValuePairs::from(vec![(CIP25_LABEL, policy_map)])
}

/// Build CIP-25 metadata for a course registration NFT.
fn build_course_metadata(
    policy_id_hex: &str,
    asset_name: &str,
    course_id: &str,
    course_title: &str,
    content_cid: Option<&str>,
) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let mut asset_fields = vec![
        (
            Metadatum::Text("name".into()),
            Metadatum::Text(format!("Alexandria Course - {}", &course_id[..8.min(course_id.len())])),
        ),
        (
            Metadatum::Text("description".into()),
            Metadatum::Text(format!("Course registration: {}", course_title)),
        ),
        (
            Metadatum::Text("mediaType".into()),
            Metadatum::Text("application/json".into()),
        ),
        (
            Metadatum::Text("courseId".into()),
            Metadatum::Text(course_id.into()),
        ),
        (
            Metadatum::Text("title".into()),
            Metadatum::Text(course_title.into()),
        ),
        (
            Metadatum::Text("version".into()),
            Metadatum::Int(1.into()),
        ),
    ];

    if let Some(cid) = content_cid {
        asset_fields.push((
            Metadatum::Text("contentCid".into()),
            Metadatum::Text(cid.into()),
        ));
    }

    let asset_meta = Metadatum::Map(KeyValuePairs::from(asset_fields));

    let asset_map = Metadatum::Map(KeyValuePairs::from(vec![(
        Metadatum::Text(asset_name.into()),
        asset_meta,
    )]));

    let policy_map = Metadatum::Map(KeyValuePairs::from(vec![(
        Metadatum::Text(policy_id_hex.into()),
        asset_map,
    )]));

    KeyValuePairs::from(vec![(CIP25_LABEL, policy_map)])
}

/// Inject CIP-25 metadata into a built transaction.
///
/// The pallas-txbuilder doesn't support auxiliary data yet, so we:
/// 1. Decode the built tx bytes as `conway::Tx`
/// 2. Set `auxiliary_data` to `AuxiliaryData::Shelley(metadata)`
/// 3. Recompute `auxiliary_data_hash` in the transaction body
/// 4. Re-encode and recalculate the tx hash
fn inject_metadata(
    tx_bytes: &[u8],
    metadata: KeyValuePairs<MetadatumLabel, Metadatum>,
) -> Result<(Vec<u8>, [u8; 32]), TxBuildError> {
    let mut tx = Tx::decode_fragment(tx_bytes)
        .map_err(|e| TxBuildError::TxDecode(e.to_string()))?;

    // Use the simplest AuxiliaryData variant — just a metadata map
    let aux_data = AuxiliaryData::Shelley(metadata);
    let aux_data_hash = aux_data.compute_hash();
    tx.auxiliary_data = pallas_primitives::Nullable::Some(aux_data);
    tx.transaction_body.auxiliary_data_hash = Some(aux_data_hash.to_vec().into());

    // Re-encode the full transaction
    let new_tx_bytes = tx.encode_fragment()
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // Recalculate the tx hash from the modified body
    let new_tx_hash = *tx.transaction_body.compute_hash();

    Ok((new_tx_bytes, new_tx_hash))
}

/// Build a minting transaction for a SkillProof NFT.
///
/// This constructs a Conway-era transaction that:
/// 1. Consumes a UTxO from the learner's address (for fees)
/// 2. Mints 1 NFT with a learner-owned `sig` policy
/// 3. Sends the NFT to the learner's own address
/// 4. Includes CIP-25 metadata (label 721)
///
/// Returns the signed transaction CBOR bytes and metadata.
pub async fn build_skill_proof_mint(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    proof_id: &str,
    skill_name: &str,
    proficiency_level: &str,
    confidence: f64,
    content_hash: Option<&str>,
) -> Result<(Vec<u8>, MintResult), TxBuildError> {
    // 1. Create the NativeScript policy (sig type, learner-owned)
    let native_script = policy::create_sig_policy(payment_key_hash);
    let policy_id = policy::compute_policy_id(&native_script)?;
    let policy_id_hex_str = policy::policy_id_hex(&policy_id);
    let asset_name_bytes = policy::skill_proof_asset_name(proof_id);
    let asset_name_str = String::from_utf8_lossy(&asset_name_bytes).to_string();

    // 2. CBOR-encode the NativeScript for the witness set
    let mut script_cbor = Vec::new();
    minicbor::encode(&native_script, &mut script_cbor)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // 3. Query chain state
    let utxos = blockfrost.get_utxos(payment_address).await?;
    if utxos.is_empty() {
        return Err(TxBuildError::NoUtxos);
    }
    let selected = BlockfrostClient::select_utxo(&utxos, MIN_UTXO_LOVELACE)
        .ok_or(TxBuildError::InsufficientFunds {
            needed: MIN_UTXO_LOVELACE,
            available: utxos.iter().map(|u| u.lovelace()).sum(),
        })?;

    let params = blockfrost.get_protocol_params().await?;
    let tip_slot = blockfrost.get_tip_slot().await?;

    // 4. Parse addresses
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;

    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;

    // 5. Build the transaction with pallas-txbuilder
    let input_lovelace = selected.lovelace();
    let fee = estimate_fee(&params, 1); // 1 native script witness

    if input_lovelace < MIN_NFT_LOVELACE + fee {
        return Err(TxBuildError::InsufficientFunds {
            needed: MIN_NFT_LOVELACE + fee,
            available: input_lovelace,
        });
    }
    let change = input_lovelace - MIN_NFT_LOVELACE - fee;

    let staging_tx = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        // Output 0: NFT + min ADA to learner
        .output(
            Output::new(pallas_addr.clone(), MIN_NFT_LOVELACE)
                .add_asset(policy_id, asset_name_bytes.clone(), 1)
                .map_err(|e| TxBuildError::Builder(e.to_string()))?,
        )
        // Output 1: change back to learner
        .output(Output::new(pallas_addr, change))
        // Mint 1 token
        .mint_asset(policy_id, asset_name_bytes, 1)
        .map_err(|e| TxBuildError::Builder(e.to_string()))?
        // Attach the native script
        .script(ScriptKind::Native, script_cbor)
        // Declare the signer
        .disclosed_signer(Hash::<28>::from(*payment_key_hash))
        // Fee
        .fee(fee)
        // TTL
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        // Testnet
        .network_id(0);

    // 6. Build the Conway-era transaction
    let built_tx = staging_tx
        .build_conway_raw()
        .map_err(|e| TxBuildError::Builder(e.to_string()))?;

    // 7. Inject CIP-25 metadata (not supported by the builder)
    let metadata = build_skill_proof_metadata(
        &policy_id_hex_str,
        &asset_name_str,
        proof_id,
        skill_name,
        proficiency_level,
        confidence,
        content_hash,
    );
    let (tx_with_metadata, _) = inject_metadata(&built_tx.tx_bytes.0, metadata)?;

    // 8. Re-decode so we can sign, then sign
    // We need to reconstruct a BuiltTransaction with the new bytes
    // and sign it. Since BuiltTransaction.sign() decodes internally,
    // we can just re-build.
    // Safety: these bytes were extracted from pallas-wallet BIP32 derivation
    // via leak_into_bytes — they satisfy BIP32-Ed25519 clamping invariants.
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });

    // Sign by decoding as Tx, adding witness, re-encoding
    let signed_tx_bytes = sign_raw_tx(&tx_with_metadata, &private_key)?;

    Ok((
        signed_tx_bytes,
        MintResult {
            tx_hash: String::new(), // Will be filled after submission
            policy_id: policy_id_hex_str,
            asset_name: asset_name_str,
        },
    ))
}

/// Build a minting transaction for a course registration NFT.
pub async fn build_course_registration(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    course_id: &str,
    course_title: &str,
    content_cid: Option<&str>,
) -> Result<(Vec<u8>, CourseRegistrationResult), TxBuildError> {
    // 1. Create the NativeScript policy
    let native_script = policy::create_sig_policy(payment_key_hash);
    let policy_id = policy::compute_policy_id(&native_script)?;
    let policy_id_hex_str = policy::policy_id_hex(&policy_id);
    let asset_name_bytes = policy::course_asset_name(course_id);
    let asset_name_str = String::from_utf8_lossy(&asset_name_bytes).to_string();

    // 2. CBOR-encode the NativeScript
    let mut script_cbor = Vec::new();
    minicbor::encode(&native_script, &mut script_cbor)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    // 3. Query chain state
    let utxos = blockfrost.get_utxos(payment_address).await?;
    if utxos.is_empty() {
        return Err(TxBuildError::NoUtxos);
    }
    let selected = BlockfrostClient::select_utxo(&utxos, MIN_UTXO_LOVELACE)
        .ok_or(TxBuildError::InsufficientFunds {
            needed: MIN_UTXO_LOVELACE,
            available: utxos.iter().map(|u| u.lovelace()).sum(),
        })?;

    let params = blockfrost.get_protocol_params().await?;
    let tip_slot = blockfrost.get_tip_slot().await?;

    // 4. Parse addresses
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;

    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;

    // 5. Build the transaction
    let input_lovelace = selected.lovelace();
    let fee = estimate_fee(&params, 1);

    if input_lovelace < MIN_NFT_LOVELACE + fee {
        return Err(TxBuildError::InsufficientFunds {
            needed: MIN_NFT_LOVELACE + fee,
            available: input_lovelace,
        });
    }
    let change = input_lovelace - MIN_NFT_LOVELACE - fee;

    let staging_tx = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        .output(
            Output::new(pallas_addr.clone(), MIN_NFT_LOVELACE)
                .add_asset(policy_id, asset_name_bytes.clone(), 1)
                .map_err(|e| TxBuildError::Builder(e.to_string()))?,
        )
        .output(Output::new(pallas_addr, change))
        .mint_asset(policy_id, asset_name_bytes, 1)
        .map_err(|e| TxBuildError::Builder(e.to_string()))?
        .script(ScriptKind::Native, script_cbor)
        .disclosed_signer(Hash::<28>::from(*payment_key_hash))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0);

    // 6. Build
    let built_tx = staging_tx
        .build_conway_raw()
        .map_err(|e| TxBuildError::Builder(e.to_string()))?;

    // 7. Inject CIP-25 metadata
    let metadata = build_course_metadata(
        &policy_id_hex_str,
        &asset_name_str,
        course_id,
        course_title,
        content_cid,
    );
    let (tx_with_metadata, _) = inject_metadata(&built_tx.tx_bytes.0, metadata)?;

    // 8. Sign
    // Safety: bytes from pallas-wallet BIP32 derivation — clamping invariants satisfied.
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed_tx_bytes = sign_raw_tx(&tx_with_metadata, &private_key)?;

    Ok((
        signed_tx_bytes,
        CourseRegistrationResult {
            tx_hash: String::new(),
            policy_id: policy_id_hex_str,
            asset_name: asset_name_str,
        },
    ))
}

/// Estimate the minimum fee for a minting transaction.
///
/// Uses the linear fee formula: `min_fee_a * estimated_size + min_fee_b`.
/// We estimate ~500 bytes for a simple mint tx with CIP-25 metadata.
fn estimate_fee(params: &ProtocolParameters, _num_witnesses: u32) -> u64 {
    // A simple mint tx with 1 input, 2 outputs, native script, and CIP-25 metadata
    // is typically 400-600 bytes. We use 600 as a conservative estimate.
    let estimated_size: u64 = 600;
    let calculated = params.calculate_min_fee(estimated_size);
    // Use whichever is higher: calculated or our floor estimate
    calculated.max(ESTIMATED_FEE)
}

/// Parse a hex-encoded 32-byte transaction hash into a pallas Hash.
fn parse_tx_hash(hex_str: &str) -> Result<Hash<32>, TxBuildError> {
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
fn sign_raw_tx(tx_bytes: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>, TxBuildError> {
    let mut tx = Tx::decode_fragment(tx_bytes)
        .map_err(|e| TxBuildError::TxDecode(e.to_string()))?;

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

    tx.transaction_witness_set.vkeywitness =
        pallas_primitives::NonEmptySet::from_vec(witnesses);

    tx.encode_fragment()
        .map_err(|e| TxBuildError::Cbor(e.to_string()))
}

/// Compute the transaction hash from signed CBOR bytes.
///
/// This is the hash that Blockfrost returns after successful submission.
pub fn compute_tx_hash(signed_tx_bytes: &[u8]) -> Result<String, TxBuildError> {
    let tx = Tx::decode_fragment(signed_tx_bytes)
        .map_err(|e| TxBuildError::TxDecode(e.to_string()))?;
    let hash = tx.transaction_body.compute_hash();
    Ok(hex::encode(hash.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_proof_metadata_structure() {
        let metadata = build_skill_proof_metadata(
            "aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb",
            "AlexProofabc12345",
            "abc12345def67890",
            "Rust Programming",
            "apply",
            0.85,
            Some("blake3hash123"),
        );

        // Should have exactly one entry under label 721
        assert_eq!(metadata.len(), 1);
        let (label, value) = &metadata[0];
        assert_eq!(*label, 721u64);

        // The value should be a Map
        match value {
            Metadatum::Map(entries) => {
                assert_eq!(entries.len(), 1, "should have one policy entry");
            }
            _ => panic!("expected Map under label 721"),
        }
    }

    #[test]
    fn course_metadata_structure() {
        let metadata = build_course_metadata(
            "aabbccdd00112233",
            "AlexCoursexyz98765",
            "xyz98765abc12345",
            "Introduction to Haskell",
            Some("QmXyz..."),
        );

        assert_eq!(metadata.len(), 1);
        let (label, _) = &metadata[0];
        assert_eq!(*label, 721u64);
    }

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
        // 44 * 600 + 155381 = 181781, which is > 300_000? No: 26400 + 155381 = 181781
        // That's less than ESTIMATED_FEE (300_000), so it should use the floor
        assert_eq!(fee, 300_000);
    }

    #[test]
    fn inject_metadata_into_simple_tx() {
        // Build a minimal valid transaction to test metadata injection
        let staging = StagingTransaction::new()
            .input(Input::new(Hash::from([0x42u8; 32]), 0))
            .output(Output::new(
                // Minimal Shelley address (testnet, key-key type)
                PallasAddress::from_bech32(
                    // Use a dummy but valid bech32 address for testing
                    "addr_test1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp"
                ).unwrap(),
                2_000_000,
            ))
            .fee(200_000)
            .network_id(0);

        let built = staging.build_conway_raw().expect("build should succeed");

        let metadata = KeyValuePairs::from(vec![(
            721u64,
            Metadatum::Text("test".into()),
        )]);

        let (new_bytes, new_hash) = inject_metadata(&built.tx_bytes.0, metadata)
            .expect("inject should succeed");

        // Verify the new bytes are valid CBOR containing a Tx
        let decoded = Tx::decode_fragment(&new_bytes).expect("should decode");

        // Verify auxiliary_data is set
        assert!(
            matches!(decoded.auxiliary_data, pallas_primitives::Nullable::Some(_)),
            "auxiliary_data should be set after injection"
        );

        // Verify auxiliary_data_hash is set
        assert!(
            decoded.transaction_body.auxiliary_data_hash.is_some(),
            "auxiliary_data_hash should be set"
        );

        // Verify tx hash changed (because body changed with aux_data_hash)
        assert_ne!(
            new_hash, built.tx_hash.0,
            "tx hash should change after metadata injection"
        );
    }
}
