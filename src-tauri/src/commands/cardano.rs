use rusqlite::params;
use tauri::State;

use crate::cardano::blockfrost::BlockfrostClient;
use crate::cardano::tx_builder;
use crate::cardano::types::{CourseRegistrationResult, MintResult};
use crate::crypto::wallet;
use crate::AppState;

/// Mint a SkillProof NFT on Cardano preprod.
///
/// This command:
/// 1. Retrieves the wallet from the unlocked vault
/// 2. Creates a learner-owned `sig` NativeScript policy
/// 3. Builds a Conway-era minting transaction with CIP-25 metadata
/// 4. Signs and submits the transaction via Blockfrost
/// 5. Updates the skill_proofs table with NFT details
///
/// Requires: vault unlocked, Blockfrost project ID configured,
/// wallet funded with >= 5 ADA on preprod testnet.
#[tauri::command]
pub async fn mint_skill_proof_nft(
    state: State<'_, AppState>,
    proof_id: String,
    skill_name: String,
    proficiency_level: String,
    confidence: f64,
    content_hash: Option<String>,
) -> Result<MintResult, String> {
    // Rate limit check
    {
        let mut limiter = state.ipc_limiter.lock().map_err(|e| e.to_string())?;
        limiter.check("mint_skill_proof_nft")?;
    }
    // 1. Get wallet from unlocked vault
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    drop(ks_guard);

    // 2. Create Blockfrost client
    let blockfrost = create_blockfrost_client(&state).await?;

    // 3. Build, sign, and get the tx bytes
    let (signed_tx, mut result) = tx_builder::build_skill_proof_mint(
        &blockfrost,
        &w.payment_address,
        &w.payment_key_hash,
        &w.payment_key_extended,
        &proof_id,
        &skill_name,
        &proficiency_level,
        confidence,
        content_hash.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    // 4. Submit to Blockfrost
    let submitted_hash = blockfrost
        .submit_tx(&signed_tx)
        .await
        .map_err(|e| e.to_string())?;

    // Use the hash from Blockfrost
    result.tx_hash = submitted_hash;

    // 5. Update the skill_proofs table with NFT details
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    db.conn()
        .execute(
            "UPDATE skill_proofs SET nft_policy_id = ?1, nft_asset_name = ?2, nft_tx_hash = ?3, updated_at = datetime('now') WHERE id = ?4",
            params![result.policy_id, result.asset_name, result.tx_hash, proof_id],
        )
        .map_err(|e| e.to_string())?;

    log::info!(
        "SkillProof NFT minted: policy={}, asset={}, tx={}",
        result.policy_id,
        result.asset_name,
        result.tx_hash
    );

    Ok(result)
}

/// Register a course on-chain by minting a course registration NFT.
///
/// This command:
/// 1. Retrieves the wallet from the unlocked vault
/// 2. Looks up the course details from the database
/// 3. Creates a learner-owned `sig` NativeScript policy
/// 4. Builds a Conway-era minting transaction with CIP-25 metadata
/// 5. Signs and submits the transaction via Blockfrost
/// 6. Updates the courses table with on_chain_tx
///
/// Requires: vault unlocked, Blockfrost project ID configured,
/// wallet funded with >= 5 ADA on preprod testnet.
#[tauri::command]
pub async fn register_course_onchain(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<CourseRegistrationResult, String> {
    // Rate limit check
    {
        let mut limiter = state.ipc_limiter.lock().map_err(|e| e.to_string())?;
        limiter.check("register_course_onchain")?;
    }
    // 1. Get wallet from unlocked vault
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    drop(ks_guard);

    // 2. Look up course details
    let (title, content_cid) = {
        let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
        let (title, cid): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT title, content_cid FROM courses WHERE id = ?1",
                params![course_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("course not found: {e}"))?;
        (title, cid)
    };

    // 3. Create Blockfrost client
    let blockfrost = create_blockfrost_client(&state).await?;

    // 4. Build, sign, and get the tx bytes
    let (signed_tx, mut result) = tx_builder::build_course_registration(
        &blockfrost,
        &w.payment_address,
        &w.payment_key_hash,
        &w.payment_key_extended,
        &course_id,
        &title,
        content_cid.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    // 5. Submit to Blockfrost
    let submitted_hash = blockfrost
        .submit_tx(&signed_tx)
        .await
        .map_err(|e| e.to_string())?;

    result.tx_hash = submitted_hash;

    // 6. Update the courses table with on_chain_tx
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    db.conn()
        .execute(
            "UPDATE courses SET on_chain_tx = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![result.tx_hash, course_id],
        )
        .map_err(|e| e.to_string())?;

    log::info!(
        "Course registered on-chain: policy={}, asset={}, tx={}",
        result.policy_id,
        result.asset_name,
        result.tx_hash
    );

    Ok(result)
}

/// Helper: Create a Blockfrost client from the local_identity's config.
///
/// The Blockfrost project ID is stored as an environment variable or
/// in a config table. For now, we check the BLOCKFROST_PROJECT_ID env var.
async fn create_blockfrost_client(
    _state: &State<'_, AppState>,
) -> Result<BlockfrostClient, String> {
    let project_id = std::env::var("BLOCKFROST_PROJECT_ID")
        .map_err(|_| "BLOCKFROST_PROJECT_ID environment variable not set. Set it to your preprod project ID from blockfrost.io".to_string())?;

    BlockfrostClient::new(project_id).map_err(|e| e.to_string())
}
