use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::crypto::keystore::Keystore;
use crate::crypto::wallet;
use crate::domain::identity::{Identity, ProfileUpdate, WalletInfo};
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct GenerateWalletResponse {
    /// The mnemonic phrase — shown once during onboarding so the user
    /// can write it down. After this, it's only accessible via `export_mnemonic`.
    pub mnemonic: String,
    pub stake_address: String,
    pub payment_address: String,
}

/// Check whether a Stronghold vault file exists.
///
/// Used on app startup to decide between onboarding and unlock screen.
#[tauri::command]
pub async fn check_vault_exists(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(Keystore::exists(&state.vault_dir))
}

/// Unlock the vault with the user's password.
///
/// Called on app startup when a vault already exists. Decrypts the
/// Stronghold snapshot, derives the wallet from the stored mnemonic,
/// and loads the identity into memory.
#[tauri::command]
pub async fn unlock_vault(
    state: State<'_, AppState>,
    password: String,
) -> Result<WalletInfo, String> {
    // Open the vault
    let ks = Keystore::open(&state.vault_dir, &password).map_err(|e| e.to_string())?;

    // Retrieve the stored mnemonic and derive the wallet
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;

    // Ensure identity exists in DB (it should, but be defensive)
    let db = state.db.lock().await;
    let exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if !exists {
        // Re-create identity row from the wallet (recovery scenario)
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                params![w.stake_address, w.payment_address],
            )
            .map_err(|e| e.to_string())?;
    }
    drop(db);

    // Store keystore in app state
    let mut keystore = state.keystore.lock().await;
    *keystore = Some(ks);

    Ok(WalletInfo {
        stake_address: w.stake_address,
        payment_address: w.payment_address,
        has_mnemonic_backup: true,
    })
}

/// Generate a new wallet and store the identity in the local database.
///
/// Called during onboarding. Creates a new Stronghold vault, generates
/// a fresh 24-word mnemonic, stores it encrypted, and returns the
/// mnemonic once for the user to write down.
#[tauri::command]
pub async fn generate_wallet(
    state: State<'_, AppState>,
    password: String,
) -> Result<GenerateWalletResponse, String> {
    // Create the vault with the user's password
    let ks = Keystore::create(&state.vault_dir, &password).map_err(|e| e.to_string())?;

    // Generate wallet
    let w = wallet::generate_wallet().map_err(|e| e.to_string())?;

    // Store mnemonic in the encrypted vault
    ks.store_mnemonic(&w.mnemonic)
        .map_err(|e| e.to_string())?;

    // Store identity in DB
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
        // Update existing identity (e.g., re-onboarding after DB wipe recovery)
        db.conn()
            .execute(
                "UPDATE local_identity SET stake_address = ?1, payment_address = ?2, updated_at = datetime('now') WHERE id = 1",
                params![w.stake_address, w.payment_address],
            )
            .map_err(|e| e.to_string())?;
    } else {
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                params![w.stake_address, w.payment_address],
            )
            .map_err(|e| e.to_string())?;
    }
    drop(db);

    // Store keystore in app state
    let mut keystore = state.keystore.lock().await;
    *keystore = Some(ks);

    Ok(GenerateWalletResponse {
        mnemonic: w.mnemonic,
        stake_address: w.stake_address,
        payment_address: w.payment_address,
    })
}

/// Restore a wallet from an existing mnemonic phrase.
///
/// Called from the "Import Wallet" flow. Validates the mnemonic,
/// creates a new Stronghold vault, and derives the wallet.
#[tauri::command]
pub async fn restore_wallet(
    state: State<'_, AppState>,
    mnemonic: String,
    password: String,
) -> Result<WalletInfo, String> {
    // Validate mnemonic by attempting to derive a wallet
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;

    // Create the vault with the user's password
    let ks = Keystore::create(&state.vault_dir, &password).map_err(|e| e.to_string())?;

    // Store mnemonic in the encrypted vault
    ks.store_mnemonic(&mnemonic)
        .map_err(|e| e.to_string())?;

    // Store identity in DB
    let db = state.db.lock().await;

    let exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if exists {
        db.conn()
            .execute(
                "UPDATE local_identity SET stake_address = ?1, payment_address = ?2, updated_at = datetime('now') WHERE id = 1",
                params![w.stake_address, w.payment_address],
            )
            .map_err(|e| e.to_string())?;
    } else {
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                params![w.stake_address, w.payment_address],
            )
            .map_err(|e| e.to_string())?;
    }
    drop(db);

    // Store keystore in app state
    let mut keystore = state.keystore.lock().await;
    *keystore = Some(ks);

    Ok(WalletInfo {
        stake_address: w.stake_address,
        payment_address: w.payment_address,
        has_mnemonic_backup: true,
    })
}

/// Export the mnemonic phrase from the vault.
///
/// Requires the vault to be unlocked. Returns the plaintext mnemonic
/// for the user to write down again.
#[tauri::command]
pub async fn export_mnemonic(state: State<'_, AppState>) -> Result<String, String> {
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked")?;
    ks.retrieve_mnemonic().map_err(|e| e.to_string())
}

/// Lock the vault, clearing in-memory secrets.
///
/// After this call, `unlock_vault` must be called again to access
/// any identity or crypto operations.
#[tauri::command]
pub async fn lock_vault(state: State<'_, AppState>) -> Result<(), String> {
    let mut keystore = state.keystore.lock().await;
    if let Some(ks) = keystore.take() {
        ks.lock().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Get the current wallet info (no secrets).
#[tauri::command]
pub async fn get_wallet_info(
    state: State<'_, AppState>,
) -> Result<Option<WalletInfo>, String> {
    let db = state.db.lock().await;

    let result = db.conn().query_row(
        "SELECT stake_address, payment_address FROM local_identity WHERE id = 1",
        [],
        |row| {
            Ok(WalletInfo {
                stake_address: row.get(0)?,
                payment_address: row.get(1)?,
                has_mnemonic_backup: true, // Always true now (Stronghold stores it)
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
