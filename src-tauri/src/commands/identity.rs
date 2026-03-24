use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::crypto::keystore::Keystore;
use crate::crypto::wallet;
use crate::domain::identity::{Identity, ProfileUpdate, WalletInfo};
use crate::domain::profile::{ProfilePayload, PublishProfileResult, SignedProfile};
use crate::ipfs::profile as ipfs_profile;
use crate::AppState;

/// Minimum password length for vault encryption (NIST SP 800-63B guidance).
const MIN_PASSWORD_LENGTH: usize = 12;

/// Validate password meets minimum strength requirements.
fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(format!(
            "Password must be at least {MIN_PASSWORD_LENGTH} characters"
        ));
    }
    Ok(())
}

/// Progress event payload sent to the frontend during wallet operations.
#[derive(Clone, Serialize)]
struct VaultProgress {
    step: String,
    detail: String,
}

#[derive(Debug, Serialize)]
pub struct GenerateWalletResponse {
    /// The mnemonic phrase — shown once during onboarding so the user
    /// can write it down. After this, it's only accessible via `export_mnemonic`.
    pub mnemonic: String,
    pub stake_address: String,
    pub payment_address: String,
}

/// Combined response from unlock/generate that includes wallet + profile,
/// eliminating the need for a separate `get_profile` IPC call.
#[derive(Debug, Serialize)]
pub struct UnlockResponse {
    pub wallet: WalletInfo,
    pub profile: Option<Identity>,
}

/// Emit a vault progress event to the frontend.
fn emit_progress(app: &AppHandle, step: &str, detail: &str) {
    let _ = app.emit(
        "vault-progress",
        VaultProgress {
            step: step.to_string(),
            detail: detail.to_string(),
        },
    );
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
///
/// Returns both wallet info and profile in a single response to avoid
/// an extra IPC round-trip.
#[tauri::command]
pub async fn unlock_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    password: String,
) -> Result<UnlockResponse, String> {
    emit_progress(&app, "vault", "Decrypting vault...");

    // Move all CPU-bound crypto work to a blocking thread so we don't
    // stall the Tokio async executor. Stronghold snapshot decryption
    // (scrypt internally) and BIP32-Ed25519 derivation (PBKDF2) are
    // the main time sinks.
    let vault_dir = state.vault_dir.clone();
    let (ks, w) = tokio::task::spawn_blocking(move || {
        let ks = Keystore::open(&vault_dir, &password).map_err(|e| e.to_string())?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        Ok::<_, String>((ks, w))
    })
    .await
    .map_err(|e| format!("blocking task failed: {e}"))?
    .map_err(|e: String| e)?;

    emit_progress(&app, "db", "Loading identity from database...");
    let profile = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
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
                    params![w.stake_address.clone(), w.payment_address],
                )
                .map_err(|e| e.to_string())?;
        }

        // Read back the full profile in the same DB lock — no extra IPC needed
        read_profile(db.conn())
    }; // db guard dropped here — before any .await

    // Store keystore in app state
    let mut keystore = state.keystore.lock().await;
    *keystore = Some(ks);
    drop(keystore);

    emit_progress(&app, "done", "Vault unlocked successfully");

    Ok(UnlockResponse {
        wallet: WalletInfo {
            stake_address: w.stake_address.clone(),
            payment_address: w.payment_address.clone(),
            has_mnemonic_backup: true,
        },
        profile,
    })
}

/// Generate a new wallet and store the identity in the local database.
///
/// Called during onboarding. Creates a new Stronghold vault, generates
/// a fresh 24-word mnemonic, stores it encrypted, and returns the
/// mnemonic once for the user to write down.
#[tauri::command]
pub async fn generate_wallet(
    app: AppHandle,
    state: State<'_, AppState>,
    password: String,
) -> Result<GenerateWalletResponse, String> {
    validate_password(&password)?;
    emit_progress(&app, "vault", "Creating encrypted vault...");

    // Move all CPU-bound crypto to a blocking thread:
    // - Keystore::create() sets up Stronghold in memory
    // - wallet::generate_wallet() does PBKDF2 + BIP32-Ed25519
    // - ks.store_mnemonic() encrypts + commits to disk (single write)
    let vault_dir = state.vault_dir.clone();
    let (ks, w) = tokio::task::spawn_blocking(move || {
        #[allow(unused_mut)] // portable keystore (mobile) needs mut; desktop stronghold does not
        let mut ks = Keystore::create(&vault_dir, &password).map_err(|e| e.to_string())?;
        let w = wallet::generate_wallet().map_err(|e| e.to_string())?;
        ks.store_mnemonic(&w.mnemonic).map_err(|e| e.to_string())?;
        Ok::<_, String>((ks, w))
    })
    .await
    .map_err(|e| format!("blocking task failed: {e}"))?
    .map_err(|e: String| e)?;

    emit_progress(&app, "db", "Storing identity in local database...");
    {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;

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
                    params![w.stake_address.clone(), w.payment_address],
                )
                .map_err(|e| e.to_string())?;
        } else {
            db.conn()
                .execute(
                    "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                    params![w.stake_address.clone(), w.payment_address],
                )
                .map_err(|e| e.to_string())?;
        }
    } // db guard dropped here — before any .await

    // Store keystore in app state
    let mut keystore = state.keystore.lock().await;
    *keystore = Some(ks);
    drop(keystore);

    emit_progress(&app, "done", "Identity created successfully");

    Ok(GenerateWalletResponse {
        mnemonic: w.mnemonic.clone(),
        stake_address: w.stake_address.clone(),
        payment_address: w.payment_address.clone(),
    })
}

/// Restore a wallet from an existing mnemonic phrase.
///
/// Called from the "Import Wallet" flow. Validates the mnemonic,
/// creates a new Stronghold vault, and derives the wallet.
#[tauri::command]
pub async fn restore_wallet(
    app: AppHandle,
    state: State<'_, AppState>,
    mnemonic: String,
    password: String,
) -> Result<WalletInfo, String> {
    validate_password(&password)?;
    emit_progress(&app, "validate", "Validating recovery phrase...");

    // Move all CPU-bound crypto to a blocking thread
    let vault_dir = state.vault_dir.clone();
    let (ks, w) = tokio::task::spawn_blocking(move || {
        let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        #[allow(unused_mut)] // portable keystore (mobile) needs mut; desktop stronghold does not
        let mut ks = Keystore::create(&vault_dir, &password).map_err(|e| e.to_string())?;
        ks.store_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        Ok::<_, String>((ks, w))
    })
    .await
    .map_err(|e| format!("blocking task failed: {e}"))?
    .map_err(|e: String| e)?;

    emit_progress(&app, "db", "Storing identity in local database...");
    {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;

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
                    params![w.stake_address.clone(), w.payment_address],
                )
                .map_err(|e| e.to_string())?;
        } else {
            db.conn()
                .execute(
                    "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                    params![w.stake_address.clone(), w.payment_address],
                )
                .map_err(|e| e.to_string())?;
        }
    } // db guard dropped here — before any .await

    // Store keystore in app state
    let mut keystore = state.keystore.lock().await;
    *keystore = Some(ks);
    drop(keystore);

    emit_progress(&app, "done", "Wallet restored successfully");

    Ok(WalletInfo {
        stake_address: w.stake_address.clone(),
        payment_address: w.payment_address.clone(),
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
pub async fn get_wallet_info(state: State<'_, AppState>) -> Result<Option<WalletInfo>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;

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
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    Ok(read_profile(db.conn()))
}

/// Internal helper: read the local user's profile from the database.
fn read_profile(conn: &rusqlite::Connection) -> Option<Identity> {
    conn.query_row(
        "SELECT stake_address, payment_address, display_name, bio, avatar_cid, profile_hash, created_at, updated_at
         FROM local_identity WHERE id = 1",
        [],
        |row| {
            Ok(Identity {
                stake_address: row.get(0)?,
                payment_address: row.get(1)?,
                display_name: row.get(2)?,
                bio: row.get(3)?,
                avatar_cid: row.get(4)?,
                profile_hash: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .ok()
}

/// Update the local user's profile.
#[tauri::command]
pub async fn update_profile(
    state: State<'_, AppState>,
    update: ProfileUpdate,
) -> Result<Identity, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;

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

    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    db.conn()
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;

    // Return the updated profile
    db.conn()
        .query_row(
            "SELECT stake_address, payment_address, display_name, bio, avatar_cid, profile_hash, created_at, updated_at
             FROM local_identity WHERE id = 1",
            [],
            |row| {
                Ok(Identity {
                    stake_address: row.get(0)?,
                    payment_address: row.get(1)?,
                    display_name: row.get(2)?,
                    bio: row.get(3)?,
                    avatar_cid: row.get(4)?,
                    profile_hash: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// Publish the local user's profile to iroh.
///
/// Reads the current profile from the database, signs it with the
/// wallet's Ed25519 key, stores the signed JSON on iroh, and saves
/// the resulting BLAKE3 hash in the database.
///
/// Requires the vault to be unlocked (wallet key needed for signing).
#[tauri::command]
pub async fn publish_profile(state: State<'_, AppState>) -> Result<PublishProfileResult, String> {
    // Get the wallet signing key from the vault
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(keystore);

    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;

    // Read the current profile from the database
    let (stake_address, display_name, bio, avatar_cid, created_at_str): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.conn()
            .query_row(
                "SELECT stake_address, display_name, bio, avatar_cid, created_at
                 FROM local_identity WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(|e| e.to_string())?
    }; // db guard dropped here — before any .await

    // Parse created_at to unix timestamp
    let created_at = parse_datetime_to_unix(&created_at_str);
    let updated_at = chrono::Utc::now().timestamp();

    // Build the profile payload
    let payload = ProfilePayload {
        version: 1,
        stake_address,
        name: display_name,
        bio,
        avatar_hash: avatar_cid,
        created_at,
        updated_at,
    };

    // Sign the profile
    let signed = ipfs_profile::sign_profile(&payload, &w.signing_key).map_err(|e| e.to_string())?;

    // Publish to iroh
    let result = ipfs_profile::publish_profile(&state.content_node, &signed)
        .await
        .map_err(|e| e.to_string())?;

    // Save the profile_hash in the database
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db.conn()
        .execute(
            "UPDATE local_identity SET profile_hash = ?1, updated_at = datetime('now') WHERE id = 1",
            params![result.profile_hash],
        )
        .map_err(|e| e.to_string())?;

    Ok(result)
}

/// Resolve a profile from iroh by BLAKE3 hash.
///
/// Fetches the signed profile document, verifies the Ed25519 signature,
/// and returns the verified profile. Used for viewing other users'
/// profiles (received via P2P in Phase 3).
#[tauri::command]
pub async fn resolve_profile(
    state: State<'_, AppState>,
    hash: String,
) -> Result<SignedProfile, String> {
    ipfs_profile::resolve_profile(&state.content_node, &hash)
        .await
        .map_err(|e| e.to_string())
}

/// Parse a SQLite datetime string to a Unix timestamp.
/// Falls back to current time if parsing fails.
fn parse_datetime_to_unix(datetime_str: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or_else(|_| chrono::Utc::now().timestamp())
}
