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
use pallas_txbuilder::{BuildConway, ExUnits, Input, Output, ScriptKind, StagingTransaction};
use pallas_wallet::PrivateKey;

use crate::domain::reputation::OnChainSkillScore;

use super::blockfrost::BlockfrostClient;
use super::cost_models::PLUTUS_V3_COST_MODEL;
use super::gov_tx_builder::{self, GovTxResult};
use super::script_refs;
use super::snapshot;
use super::tx_builder::{
    inject_metadata, parse_tx_hash, sign_raw_tx, TxBuildError, MIN_NFT_LOVELACE, TTL_OFFSET,
};

/// Minimum ADA for the reference NFT UTxO at the soulbound script address.
/// Needs to cover the inline datum storage cost (~3 ADA).
const MIN_REF_UTXO_LOVELACE: u64 = 3_000_000;

/// Distinct collateral UTxO floor (a UTxO can't be input + collateral).
const MIN_COLLATERAL_LOVELACE: u64 = 5_000_000;

/// Plutus fee = size fee + execution fee (mem×0.0577 + steps×0.0000721) + buffer.
fn plutus_fee(min_fee_a: u64, min_fee_b: u64, tx_size: u64, mem: u64, steps: u64) -> u64 {
    let size_fee = min_fee_a * tx_size + min_fee_b;
    let exec_fee = (mem * 577).div_ceil(10_000) + (steps * 721).div_ceil(10_000_000);
    size_fee + exec_fee + 30_000
}

/// Build a CIP-68 soulbound reputation token minting transaction.
///
/// Creates two tokens under the reputation_minting policy:
/// - Reference NFT (label 100 prefix) → sent to soulbound script address
/// - User token (label 222 prefix) → sent to owner's payment address
///
/// The reference NFT output carries an inline ReputationDatum.
#[allow(clippy::too_many_arguments)]
pub async fn build_soulbound_mint_unsigned(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    _payment_key_extended: &[u8; 64],
    owner_key_hash: &[u8; 28],
    subject_id: &str,
    role: &crate::domain::reputation::ReputationRole,
    skills: &[OnChainSkillScore],
    window_start_ms: i64,
    window_end_ms: i64,
) -> Result<Vec<u8>, TxBuildError> {
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
    let policy_hash =
        gov_tx_builder::hash_from_hex_pub(script_refs::REPUTATION_MINTING_SCRIPT_HASH)?;
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
    // Largest UTxO funds the spend (must cover both token outputs + fee);
    // collateral is a separate UTxO selected below.
    let selected = utxos
        .iter()
        .max_by_key(|u| u.lovelace())
        .ok_or(TxBuildError::NoUtxos)?;

    // 6. Parse addresses
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let soulbound_addr = gov_tx_builder::script_address(script_refs::SOULBOUND_SCRIPT_HASH)?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;

    // 7. Distinct collateral UTxO (a UTxO can't be both spend input and
    //    collateral). Pick a separate pure-ADA UTxO.
    let input_lovelace = selected.lovelace();
    let collateral = utxos
        .iter()
        .filter(|u| u.tx_hash != selected.tx_hash || u.tx_index != selected.tx_index)
        .filter(|u| u.lovelace() >= MIN_COLLATERAL_LOVELACE)
        .max_by_key(|u| u.lovelace())
        .ok_or(TxBuildError::InsufficientFunds {
            needed: MIN_COLLATERAL_LOVELACE,
            available: 0,
        })?;
    let ref_utxo = script_refs::REPUTATION_MINTING_REF_UTXO;
    let ref_hash = parse_tx_hash(ref_utxo.0)?;
    let coll_hash = parse_tx_hash(&collateral.tx_hash)?;

    // 8. Build the CIP-68 dual-token mint natively so the Mint-purpose
    //    redeemer + script_data_hash are emitted correctly (the old
    //    inject_plutus_fields path produced neither). Ref NFT → soulbound
    //    script with inline datum; user token → owner wallet. CIP-25
    //    metadata is layered on after build (it does not affect
    //    script_data_hash).
    let build = |fee: u64, ex: ExUnits| -> Result<Vec<u8>, TxBuildError> {
        let change = input_lovelace
            .checked_sub(MIN_REF_UTXO_LOVELACE + MIN_NFT_LOVELACE + fee)
            .ok_or(TxBuildError::InsufficientFunds {
                needed: MIN_REF_UTXO_LOVELACE + MIN_NFT_LOVELACE + fee,
                available: input_lovelace,
            })?;
        let staging = StagingTransaction::new()
            .input(Input::new(input_tx_hash, selected.tx_index))
            .output(
                Output::new(soulbound_addr.clone(), MIN_REF_UTXO_LOVELACE)
                    .add_asset(policy_id, ref_asset_name.clone(), 1)
                    .map_err(|e| TxBuildError::Builder(e.to_string()))?
                    .set_inline_datum(datum.clone()),
            )
            .output(
                Output::new(pallas_addr.clone(), MIN_NFT_LOVELACE)
                    .add_asset(policy_id, usr_asset_name.clone(), 1)
                    .map_err(|e| TxBuildError::Builder(e.to_string()))?,
            )
            .output(Output::new(pallas_addr.clone(), change))
            .mint_asset(policy_id, ref_asset_name.clone(), 1)
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .mint_asset(policy_id, usr_asset_name.clone(), 1)
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .reference_input(Input::new(ref_hash, ref_utxo.1))
            .collateral_input(Input::new(coll_hash, collateral.tx_index))
            .add_mint_redeemer(policy_id, redeemer.clone(), Some(ex))
            .language_view(ScriptKind::PlutusV3, PLUTUS_V3_COST_MODEL.to_vec())
            .disclosed_signer(Hash::<28>::from(*payment_key_hash))
            .fee(fee)
            .invalid_from_slot(tip_slot + TTL_OFFSET)
            .network_id(0);
        let built = staging
            .build_conway_raw()
            .map_err(|e| TxBuildError::Builder(e.to_string()))?
            .tx_bytes
            .0;
        let metadata = snapshot::build_snapshot_metadata(
            "snap",
            subject_id,
            role.as_str(),
            "mint",
            skills.len() as i64,
        );
        let (with_meta, _) = inject_metadata(&built, metadata)?;
        Ok(with_meta)
    };

    // 9. Pass 1 (estimate) → evaluate_tx → rebuild with real units + fee.
    const EST_MEM: u64 = 2_000_000;
    const EST_STEPS: u64 = 700_000_000;
    let est_fee = plutus_fee(
        params.min_fee_a,
        params.min_fee_b,
        2_500,
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
            log::debug!("soulbound mint: evaluate_tx failed, using estimate: {e}");
            (EST_MEM, EST_STEPS)
        }
    };
    let real_ex = ExUnits {
        mem: if mem == 0 { EST_MEM } else { mem },
        steps: if steps == 0 { EST_STEPS } else { steps },
    };
    let fee = plutus_fee(
        params.min_fee_a,
        params.min_fee_b,
        draft.len() as u64 + 200,
        real_ex.mem,
        real_ex.steps,
    );
    build(fee, real_ex)
}

/// Sign the soulbound mint with the app's extended key. (The deployed
/// reputation_minting policy's `authorized_minter` is the operator key,
/// whose witness scheme differs — callers minting under that policy sign
/// the unsigned tx out-of-band instead.)
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
    let unsigned = build_soulbound_mint_unsigned(
        blockfrost,
        payment_address,
        payment_key_hash,
        payment_key_extended,
        owner_key_hash,
        subject_id,
        role,
        skills,
        window_start_ms,
        window_end_ms,
    )
    .await?;
    let private_key = PrivateKey::Extended(unsafe {
        SecretKeyExtended::from_bytes_unchecked(*payment_key_extended)
    });
    let signed_tx_bytes = sign_raw_tx(&unsigned, &private_key)?;
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
    fn soulbound_mint_passes_deploy_gate_now_validators_are_deployed() {
        // The reputation-minting reference script is deployed (preprod,
        // 2026-05-22), so the builder must get PAST the deploy gate. With
        // a bogus address + offline test Blockfrost it still fails — but
        // for a downstream reason, NOT "validators not yet deployed".
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
        assert!(
            !result
                .unwrap_err()
                .to_string()
                .contains("validators not yet deployed"),
            "deploy gate should be open now that the reference script is live"
        );
    }

    /// Live preprod soulbound/reputation mint: builds the UNSIGNED CBOR
    /// (signed out-of-band by the treasury minter via cardano-cli).
    ///   BLOCKFROST_PROJECT_ID=preprod… cargo test -p alexandria-node \
    ///     live_soulbound_mint_unsigned -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_soulbound_mint_unsigned() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let treasury_addr =
            "addr_test1qps9dhjrekj8d7nuf94ltzeslzwfj30u0f5tgy6ddmecxvm5wes3g9ja43ewdtq6ww3rccuzjvv7gdd4hghj9jdg7njqpu4uns";
        let admin_pkh: [u8; 28] = gov_tx_builder::hash_from_hex_pub(
            "6056de43cda476fa7c496bf58b30f89c9945fc7a68b4134d6ef38333",
        )
        .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let skills = vec![OnChainSkillScore {
            skill_id_bytes: "00112233445566778899aabbccddeeff".into(),
            proficiency: 3,
            impact_score: 1_000_000,
            confidence: 9_000,
            evidence_count: 2,
        }];
        let rt = tokio::runtime::Runtime::new().unwrap();
        let unsigned = rt.block_on(async {
            let bf = BlockfrostClient::new(pid).unwrap();
            build_soulbound_mint_unsigned(
                &bf,
                treasury_addr,
                &admin_pkh,
                &[0u8; 64],
                &admin_pkh,
                "rep1",
                &crate::domain::reputation::ReputationRole::Instructor,
                &skills,
                now,
                now + 2_592_000_000,
            )
            .await
            .expect("build unsigned soulbound tx")
        });
        println!("UNSIGNED_CBOR:{}", hex::encode(&unsigned));
    }
}
