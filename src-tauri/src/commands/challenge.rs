//! IPC for the credential-challenge mechanism (VC-first rebuild).

use tauri::State;

use crate::cardano::{blockfrost::BlockfrostClient, challenge_escrow_tx_builder};
use crate::crypto::hash::blake2b_256;
use crate::domain::challenge::{
    ChallengeResolution, ChallengeVote, CredentialChallenge, SubmitCredentialChallengeParams,
};
use crate::evidence::challenge as challenge_logic;
use crate::AppState;

/// Parse a hex-encoded 28-byte payment key hash.
fn parse_key_hash(hex_str: &str) -> Result<[u8; 28], String> {
    let bytes = hex::decode(hex_str.trim()).map_err(|e| format!("invalid key hash hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "key hash must be 28 bytes".to_string())
}

#[tauri::command]
pub async fn submit_credential_challenge(
    state: State<'_, AppState>,
    params: SubmitCredentialChallengeParams,
) -> Result<CredentialChallenge, String> {
    // Look up the challenger (local node's stake address) and a
    // placeholder signature — real signing is done at the gossip
    // envelope layer.
    let challenger: String = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("local identity not initialized: {e}"))?
    };

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::submit_challenge(db.conn(), &params, &challenger, "ipc-placeholder-sig")
}

#[tauri::command]
pub async fn vote_on_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
    upheld: bool,
    reason: Option<String>,
) -> Result<ChallengeVote, String> {
    let voter: String = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("local identity not initialized: {e}"))?
    };

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::vote(db.conn(), &challenge_id, &voter, upheld, reason.as_deref())
}

#[tauri::command]
pub async fn resolve_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<ChallengeResolution, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::resolve(db.conn(), &challenge_id)
}

#[tauri::command]
pub async fn list_credential_challenges(
    state: State<'_, AppState>,
    status: Option<String>,
    credential_id: Option<String>,
) -> Result<Vec<CredentialChallenge>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::list_challenges(db.conn(), status.as_deref(), credential_id.as_deref())
}

#[tauri::command]
pub async fn get_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<Option<CredentialChallenge>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::get_challenge(db.conn(), &challenge_id)
}

#[tauri::command]
pub async fn expire_overdue_credential_challenges(
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::expire_overdue(db.conn())
}

/// Build helper: derive the local wallet from the unlocked vault and a
/// Blockfrost client from the environment. Shared by stake lock/settle.
async fn wallet_and_blockfrost(
    state: &State<'_, AppState>,
) -> Result<(crate::crypto::wallet::Wallet, BlockfrostClient), String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let wallet =
        crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    let project_id = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let conn = db_guard.as_ref().map(|db| db.conn());
        crate::cardano::blockfrost::resolve_project_id(conn)
    }
    .ok_or(
        "Blockfrost project id not configured \
         (set in Settings → Cardano, or export BLOCKFROST_PROJECT_ID)",
    )?;
    let bf = BlockfrostClient::new(project_id).map_err(|e| e.to_string())?;
    Ok((wallet, bf))
}

/// Lock the challenger's stake at the escrow script for a challenge.
///
/// Works today on preprod (a plain pay-to-script); spending it later
/// needs the escrow validator deployed. `treasury_key_hash` and
/// `dao_authority_key_hash` (hex, 28 bytes) are stored in the escrow
/// datum; both default to the challenger's own key when omitted (solo
/// operator / testing).
#[tauri::command]
pub async fn lock_challenge_stake(
    state: State<'_, AppState>,
    challenge_id: String,
    treasury_key_hash: Option<String>,
    dao_authority_key_hash: Option<String>,
) -> Result<String, String> {
    let stake = {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        challenge_logic::get_stake_info(db.conn(), &challenge_id)?
    };
    if stake.stake_status != "none" {
        return Err(format!(
            "challenge stake already {} — cannot re-lock",
            stake.stake_status
        ));
    }

    let (wallet, bf) = wallet_and_blockfrost(&state).await?;
    let treasury = match treasury_key_hash {
        Some(h) => parse_key_hash(&h)?,
        None => wallet.payment_key_hash,
    };
    let authority = match dao_authority_key_hash {
        Some(h) => parse_key_hash(&h)?,
        None => wallet.payment_key_hash,
    };
    let challenge_id_hash = blake2b_256(challenge_id.as_bytes());

    let result = challenge_escrow_tx_builder::build_lock_tx(
        &bf,
        &wallet.payment_address,
        &wallet.payment_key_hash,
        &wallet.payment_key_extended,
        &treasury,
        &authority,
        &challenge_id_hash,
        stake.stake_lovelace as u64,
    )
    .await
    .map_err(|e| e.to_string())?;

    let tx_hash = bf
        .submit_tx(&result.tx_cbor)
        .await
        .map_err(|e| format!("lock tx submission failed: {e}"))?;

    {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        challenge_logic::set_stake_locked(
            db.conn(),
            &challenge_id,
            &tx_hash,
            &hex::encode(wallet.payment_key_hash),
            &hex::encode(treasury),
        )?;
    }
    Ok(tx_hash)
}

/// Settle a resolved challenge's escrowed stake. The DAO authority
/// spends the escrow UTxO, refunding the challenger (upheld) or
/// forfeiting to the treasury (rejected). Gated on the escrow validator
/// being deployed. The destination is read from what the lock tx recorded
/// (challenger pkh on refund, treasury pkh on forfeit), never from the caller.
#[tauri::command]
pub async fn settle_challenge_stake(
    state: State<'_, AppState>,
    challenge_id: String,
    escrow_tx_hash: String,
    escrow_index: u64,
) -> Result<String, String> {
    let stake = {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        challenge_logic::get_stake_info(db.conn(), &challenge_id)?
    };
    if stake.stake_status != "locked" {
        return Err(format!(
            "challenge stake is '{}', expected 'locked'",
            stake.stake_status
        ));
    }
    let refund = match stake.challenge_status.as_str() {
        "upheld" => true,
        "rejected" => false,
        other => {
            return Err(format!(
                "challenge not resolved (status: {other}) — settle after resolution"
            ))
        }
    };

    // The recipient is whatever the escrow datum says it is, and the datum
    // was written by *this* app at lock time — so read it back rather than
    // asking the frontend to repeat it. The on-chain validator would reject a
    // wrong recipient anyway, but a settle that cannot be misdirected is
    // better than one the ledger has to catch.
    let recipient_hex = if refund {
        stake.escrow_challenger_pkh.as_deref()
    } else {
        stake.escrow_treasury_pkh.as_deref()
    }
    .ok_or(
        "this stake was locked before its escrow recipients were recorded — \
         re-lock it, or settle it with a manually built transaction",
    )?;
    let recipient = parse_key_hash(recipient_hex)?;
    let (wallet, bf) = wallet_and_blockfrost(&state).await?;

    let result = challenge_escrow_tx_builder::build_settle_tx(
        &bf,
        &wallet.payment_address,
        &wallet.payment_key_hash,
        &wallet.payment_key_extended,
        (&escrow_tx_hash, escrow_index),
        &recipient,
        stake.stake_lovelace as u64,
        refund,
    )
    .await
    .map_err(|e| e.to_string())?;

    let tx_hash = bf
        .submit_tx(&result.tx_cbor)
        .await
        .map_err(|e| format!("settle tx submission failed: {e}"))?;

    {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        challenge_logic::set_stake_settled(db.conn(), &challenge_id, &tx_hash, refund)?;
    }
    Ok(tx_hash)
}
