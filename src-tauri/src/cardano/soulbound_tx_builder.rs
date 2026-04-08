//! CIP-68 soulbound reputation token transaction builder.
//!
//! Builds a minting transaction that creates two tokens:
//!   - Reference NFT (label 100) → soulbound script address with inline datum
//!   - User token (label 222) → learner's wallet
//!
//! Uses the reputation_minting policy for minting and the soulbound
//! validator for the reference NFT spending conditions.

use pallas_addresses::Address as PallasAddress;
use pallas_crypto::hash::Hash;
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

use crate::domain::reputation::OnChainSkillScore;

use super::blockfrost::BlockfrostClient;
use super::gov_tx_builder::{self, GovTxResult};
use super::script_refs;
use super::snapshot;
use super::tx_builder::{
    inject_metadata, parse_tx_hash, sign_raw_tx, TxBuildError, MIN_NFT_LOVELACE,
    MIN_UTXO_LOVELACE, TTL_OFFSET,
};

/// Minimum ADA for the reference NFT UTxO at the soulbound script address.
/// Needs to cover the inline datum storage cost (~3 ADA).
const MIN_REF_UTXO_LOVELACE: u64 = 3_000_000;

/// Build a CIP-68 soulbound reputation token minting transaction.
///
/// Creates two tokens under the reputation_minting policy:
/// - Reference NFT (label 100 prefix) → sent to soulbound script address
/// - User token (label 222 prefix) → sent to owner's payment address
///
/// The reference NFT output carries an inline ReputationDatum.
#[allow(clippy::too_many_arguments)]
pub async fn build_soulbound_mint_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key_extended: &[u8; 64],
    owner_key_hash: &[u8; 28],
    subject_id: &str,
    role: &crate::domain::reputation::ReputationRole,
    skills: &[OnChainSkillScore],
    window_start_ms: i64,
    window_end_ms: i64,
) -> Result<GovTxResult, TxBuildError> {
    if !gov_tx_builder::validators_deployed() {
        return Err(TxBuildError::Cbor(
            "SoulboundMint: validators not yet deployed as reference scripts".into(),
        ));
    }

    // 1. Build asset names
    let base_name = snapshot::reputation_base_name(subject_id, role);
    let ref_asset_name = snapshot::reference_asset_name(&base_name);
    let usr_asset_name = snapshot::user_asset_name(&base_name);

    // 2. Encode the ReputationDatum
    let datum = snapshot::encode_reputation_datum(
        owner_key_hash,
        subject_id,
        role,
        skills,
        window_start_ms,
        window_end_ms,
    )?;

    // 3. Encode the minting redeemer
    let redeemer = super::plutus_data::encode_reputation_mint_redeemer("mint")?;

    // 4. Compute the reputation minting policy ID
    let policy_hash = gov_tx_builder::hash_from_hex_pub(script_refs::REPUTATION_MINTING_SCRIPT_HASH)?;
    let policy_id = Hash::<28>::from(policy_hash);

    // 5. Query chain state (parallel)
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

    // 6. Parse addresses
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let soulbound_addr = gov_tx_builder::script_address(script_refs::SOULBOUND_SCRIPT_HASH)?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;

    // 7. Calculate fees
    let fee = params.calculate_min_fee(1200).max(500_000); // Plutus tx is larger
    let input_lovelace = selected.lovelace();
    let total_out = MIN_REF_UTXO_LOVELACE + MIN_NFT_LOVELACE + fee;
    if input_lovelace < total_out {
        return Err(TxBuildError::InsufficientFunds {
            needed: total_out,
            available: input_lovelace,
        });
    }
    let change = input_lovelace - MIN_REF_UTXO_LOVELACE - MIN_NFT_LOVELACE - fee;

    // 8. Build transaction
    let staging_tx = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        // Output 0: Reference NFT → soulbound script address (inline datum will be injected)
        .output(
            Output::new(soulbound_addr, MIN_REF_UTXO_LOVELACE)
                .add_asset(policy_id, ref_asset_name.clone(), 1)
                .map_err(|e| TxBuildError::Builder(e.to_string()))?,
        )
        // Output 1: User token → owner's payment address
        .output(
            Output::new(pallas_addr.clone(), MIN_NFT_LOVELACE)
                .add_asset(policy_id, usr_asset_name.clone(), 1)
                .map_err(|e| TxBuildError::Builder(e.to_string()))?,
        )
        // Output 2: Change
        .output(Output::new(pallas_addr, change))
        // Mint both tokens
        .mint_asset(policy_id, ref_asset_name, 1)
        .map_err(|e| TxBuildError::Builder(e.to_string()))?
        .mint_asset(policy_id, usr_asset_name, 1)
        .map_err(|e| TxBuildError::Builder(e.to_string()))?
        .disclosed_signer(Hash::<28>::from(*payment_key_hash))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0);

    let built_tx = staging_tx
        .build_conway_raw()
        .map_err(|e| TxBuildError::Builder(e.to_string()))?;

    // 9. Inject Plutus fields (reference inputs, collateral, redeemer)
    let ref_utxo = script_refs::REPUTATION_MINTING_REF_UTXO;
    let ref_hash = hex::decode(ref_utxo.0)
        .map_err(|e| TxBuildError::TxDecode(format!("invalid ref utxo hash: {e}")))?;
    let ref_hash_arr: [u8; 32] = ref_hash
        .try_into()
        .map_err(|_| TxBuildError::TxDecode("ref utxo hash must be 32 bytes".into()))?;

    let coll_hash = hex::decode(&selected.tx_hash)
        .map_err(|e| TxBuildError::TxDecode(format!("invalid collateral hash: {e}")))?;
    let coll_hash_arr: [u8; 32] = coll_hash
        .try_into()
        .map_err(|_| TxBuildError::TxDecode("collateral hash must be 32 bytes".into()))?;

    let (tx_with_plutus, _) = gov_tx_builder::inject_plutus_fields(
        &built_tx.tx_bytes.0,
        &[(ref_hash_arr, ref_utxo.1)],
        &[(coll_hash_arr, selected.tx_index)],
        &redeemer,
        Some(&datum),
    )?;

    // 10. Inject CIP-25 metadata (label 1694 for reputation)
    let metadata = snapshot::build_snapshot_metadata(
        "snap",    // snapshot_id (short placeholder, overridden by caller)
        subject_id,
        role.as_str(),
        "mint",
        skills.len() as i64,
    );
    let (tx_bytes, _) = inject_metadata(&tx_with_plutus, metadata)?;

    // 11. Evaluate execution units
    match blockfrost.evaluate_tx(&tx_bytes).await {
        Ok(units) => log::info!("Soulbound mint execution units: {:?}", units),
        Err(e) => log::warn!("Failed to evaluate soulbound tx: {e}"),
    }

    // 12. Sign
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed_tx_bytes = sign_raw_tx(&tx_bytes, &private_key)?;

    // Compute tx hash
    let tx_hash = super::tx_builder::compute_tx_hash(&signed_tx_bytes)?;

    Ok(GovTxResult {
        tx_cbor: signed_tx_bytes,
        tx_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::reputation::ReputationRole;

    #[test]
    fn asset_names_have_correct_cip68_prefixes() {
        let base = snapshot::reputation_base_name("sub123", &ReputationRole::Learner);
        let ref_name = snapshot::reference_asset_name(&base);
        let usr_name = snapshot::user_asset_name(&base);

        // CIP-68 reference label 100: [0x00, 0x06, 0x43, 0xb0]
        assert_eq!(&ref_name[..4], &[0x00, 0x06, 0x43, 0xb0]);
        // CIP-68 user label 222: [0x00, 0x0d, 0xe1, 0x40]
        assert_eq!(&usr_name[..4], &[0x00, 0x0d, 0xe1, 0x40]);
        // Same base name suffix
        assert_eq!(&ref_name[4..], &usr_name[4..]);
    }

    #[test]
    fn datum_encoding_produces_valid_cbor() {
        let skills = vec![OnChainSkillScore {
            skill_id_bytes: "736b696c6c5f746573740000".into(),
            proficiency: 3,
            impact_score: 850_000,
            confidence: 5_000,
            evidence_count: 12,
        }];

        let datum = snapshot::encode_reputation_datum(
            &[0xAA; 28],
            "subject_test",
            &ReputationRole::Instructor,
            &skills,
            1700000000000,
            1703000000000,
        )
        .unwrap();

        // Verify CBOR starts with Constr(0, ...) tag
        assert_eq!(datum[0], 0xd8); // CBOR tag
        assert_eq!(datum[1], 0x79); // tag 121 = Constr(0, ...)
    }

    #[test]
    fn soulbound_mint_fails_when_validators_not_deployed() {
        // Since validators aren't deployed, this should return an error
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            let bf = BlockfrostClient::new("test_project_id".into()).unwrap();
            build_soulbound_mint_tx(
                &bf,
                "addr_test1qz...",
                &[0; 28],
                &[0; 64],
                &[0; 28],
                "sub1",
                &ReputationRole::Learner,
                &[],
                1700000000000,
                1703000000000,
            )
            .await
        });
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("validators not yet deployed"));
    }
}
