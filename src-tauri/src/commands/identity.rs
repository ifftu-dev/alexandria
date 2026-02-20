use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::crypto::wallet;
use crate::domain::identity::{Identity, ProfileUpdate, WalletInfo};
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct GenerateWalletResponse {
    pub mnemonic: String,
    pub stake_address: String,
    pub payment_address: String,
}

/// Generate a new wallet and store the identity in the local database.
/// This is called on first launch (onboarding).
#[tauri::command]
pub async fn generate_wallet(
    state: State<'_, AppState>,
) -> Result<GenerateWalletResponse, String> {
    let w = wallet::generate_wallet().map_err(|e| e.to_string())?;

    let db = state.db.lock().await;

    // Check if identity already exists
    let exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if exists {
        return Err("wallet already exists — use restore_wallet to change identity".into());
    }

    db.conn()
        .execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
            params![w.stake_address, w.payment_address],
        )
        .map_err(|e| e.to_string())?;

    Ok(GenerateWalletResponse {
        mnemonic: w.mnemonic,
        stake_address: w.stake_address,
        payment_address: w.payment_address,
    })
}

/// Get the current wallet info (no secrets).
#[tauri::command]
pub async fn get_wallet_info(
    state: State<'_, AppState>,
) -> Result<Option<WalletInfo>, String> {
    let db = state.db.lock().await;

    let result = db.conn().query_row(
        "SELECT stake_address, payment_address, mnemonic_enc IS NOT NULL FROM local_identity WHERE id = 1",
        [],
        |row| {
            Ok(WalletInfo {
                stake_address: row.get(0)?,
                payment_address: row.get(1)?,
                has_mnemonic_backup: row.get(2)?,
            })
        },
    );

    match result {
        Ok(info) => Ok(Some(info)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Get the local user's profile.
#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>) -> Result<Option<Identity>, String> {
    let db = state.db.lock().await;

    let result = db.conn().query_row(
        "SELECT stake_address, payment_address, display_name, bio, avatar_cid, created_at, updated_at
         FROM local_identity WHERE id = 1",
        [],
        |row| {
            Ok(Identity {
                stake_address: row.get(0)?,
                payment_address: row.get(1)?,
                display_name: row.get(2)?,
                bio: row.get(3)?,
                avatar_cid: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    );

    match result {
        Ok(profile) => Ok(Some(profile)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Update the local user's profile.
#[tauri::command]
pub async fn update_profile(
    state: State<'_, AppState>,
    update: ProfileUpdate,
) -> Result<Identity, String> {
    let db = state.db.lock().await;

    // Build dynamic UPDATE statement
    let mut set_clauses = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref name) = update.display_name {
        set_clauses.push("display_name = ?");
        values.push(Box::new(name.clone()));
    }
    if let Some(ref bio) = update.bio {
        set_clauses.push("bio = ?");
        values.push(Box::new(bio.clone()));
    }
    if let Some(ref avatar) = update.avatar_cid {
        set_clauses.push("avatar_cid = ?");
        values.push(Box::new(avatar.clone()));
    }

    if set_clauses.is_empty() {
        return Err("no fields to update".into());
    }

    set_clauses.push("updated_at = datetime('now')");

    let sql = format!(
        "UPDATE local_identity SET {} WHERE id = 1",
        set_clauses.join(", ")
    );

    let params: Vec<&dyn rusqlite::types::ToSql> =
        values.iter().map(|v| v.as_ref()).collect();

    db.conn()
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;

    // Return the updated profile
    db.conn()
        .query_row(
            "SELECT stake_address, payment_address, display_name, bio, avatar_cid, created_at, updated_at
             FROM local_identity WHERE id = 1",
            [],
            |row| {
                Ok(Identity {
                    stake_address: row.get(0)?,
                    payment_address: row.get(1)?,
                    display_name: row.get(2)?,
                    bio: row.get(3)?,
                    avatar_cid: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}
