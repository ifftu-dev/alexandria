//! IPC commands that manage user profiles.
//!
//! The profile lifecycle:
//!
//! 1. `list_profiles` is called when the app starts.
//! 2. If empty → onboarding (`create_profile` or `restore_profile_with_mnemonic`).
//! 3. If non-empty → picker (`unlock_profile`).
//! 4. `lock_profile` returns to the picker; `delete_profile` removes one.

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::crypto::keystore::Keystore;
use crate::crypto::wallet;
use crate::domain::identity::{Identity, WalletInfo};
use crate::profile::{Avatar, ProfileId, ProfileSummary};
use crate::AppState;

/// Mirror of `commands::identity` constant. NIST SP 800-63B guidance.
const MIN_PASSWORD_LENGTH: usize = 12;

fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(format!(
            "Your password must be at least {MIN_PASSWORD_LENGTH} characters long."
        ));
    }
    Ok(())
}

#[derive(Clone, Serialize)]
struct VaultProgress {
    step: String,
    detail: String,
}

fn emit_progress(app: &AppHandle, step: &str, detail: &str) {
    let _ = app.emit(
        "vault-progress",
        VaultProgress {
            step: step.to_string(),
            detail: detail.to_string(),
        },
    );
}

#[derive(Debug, Serialize)]
pub struct CreateProfileResponse {
    pub summary: ProfileSummary,
    pub mnemonic: String,
    pub wallet: WalletInfo,
    pub profile: Option<Identity>,
}

#[derive(Debug, Serialize)]
pub struct UnlockProfileResponse {
    pub wallet: WalletInfo,
    pub profile: Option<Identity>,
}

/// Snapshot all on-disk profiles. Safe to call before any profile is unlocked.
#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ProfileSummary>, String> {
    Ok(state.profile_manager.list())
}

/// Returns the id of the currently-unlocked profile, if any. Used by the
/// frontend router to decide whether to route into the app or the picker.
#[tauri::command]
pub async fn get_active_profile_id(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.active_id().map(|id| id.as_str().to_string()))
}

/// Create a brand-new profile: reserves an index entry, creates the vault,
/// generates a wallet, opens the DB, and marks the profile active. Returns
/// the mnemonic once so the user can write it down.
#[tauri::command]
pub async fn create_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    display_name: String,
    password: String,
    #[allow(non_snake_case)] avatar: Option<Avatar>,
) -> Result<CreateProfileResponse, String> {
    validate_password(&password)?;

    // Refuse if another profile is already active — caller must lock first.
    if state.active_id().is_some() {
        return Err("lock the active profile before creating a new one".to_string());
    }

    emit_progress(&app, "profile", "Reserving profile slot...");
    let avatar = avatar.unwrap_or_default();
    let paths = state
        .profile_manager
        .create(&display_name, avatar)
        .map_err(|e| e.to_string())?;

    // All cryptographic work happens on a blocking thread.
    let paths_for_blocking = paths.clone();
    let password_for_blocking = password.clone();
    emit_progress(&app, "vault", "Creating encrypted vault...");
    let (ks, w) = tokio::task::spawn_blocking(move || {
        #[allow(unused_mut)]
        let mut ks = Keystore::create(&paths_for_blocking.vault_dir, &password_for_blocking)
            .map_err(|e| e.to_string())?;
        let w = wallet::generate_wallet().map_err(|e| e.to_string())?;
        ks.store_mnemonic(&w.mnemonic).map_err(|e| e.to_string())?;
        Ok::<_, String>((ks, w))
    })
    .await
    .map_err(|e| format!("blocking task failed: {e}"))?
    .map_err(|e: String| e)?;

    emit_progress(&app, "db", "Opening encrypted database...");
    state
        .start_active_profile(paths.clone(), ks)
        .await
        .map_err(|e| format!("failed to bring profile online: {e}"))?;

    // Insert the wallet identity row.
    emit_progress(&app, "db", "Storing identity in local database...");
    {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "INSERT OR REPLACE INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                params![w.stake_address.clone(), w.payment_address.clone()],
            )
            .map_err(|e| e.to_string())?;

        #[cfg(feature = "dev-seed")]
        {
            let did = crate::crypto::did::derive_did_key(&w.signing_key);
            let _ =
                crate::db::seed::bind_current_user_to_seed_with_did(db.conn(), Some(did.as_str()));
        }
    }

    state
        .profile_manager
        .touch_unlocked(&paths.id)
        .map_err(|e| e.to_string())?;

    let summary = state
        .profile_manager
        .get(&paths.id)
        .ok_or("just-created profile vanished from index")?;

    emit_progress(&app, "done", "Profile ready");

    Ok(CreateProfileResponse {
        summary,
        mnemonic: w.mnemonic.clone(),
        wallet: WalletInfo {
            stake_address: w.stake_address.clone(),
            payment_address: w.payment_address.clone(),
            has_mnemonic_backup: true,
        },
        profile: read_profile_from_db(&state)?,
    })
}

/// Create a profile from an existing BIP-39 mnemonic. Same flow as
/// [`create_profile`] but reuses caller-supplied recovery words.
#[tauri::command]
pub async fn restore_profile_with_mnemonic(
    app: AppHandle,
    state: State<'_, AppState>,
    display_name: String,
    mnemonic: String,
    password: String,
    #[allow(non_snake_case)] avatar: Option<Avatar>,
) -> Result<UnlockProfileResponse, String> {
    validate_password(&password)?;
    if state.active_id().is_some() {
        return Err("lock the active profile before restoring a new one".to_string());
    }

    emit_progress(&app, "profile", "Reserving profile slot...");
    let avatar = avatar.unwrap_or_default();
    let paths = state
        .profile_manager
        .create(&display_name, avatar)
        .map_err(|e| e.to_string())?;

    emit_progress(&app, "validate", "Validating recovery phrase...");
    let paths_for_blocking = paths.clone();
    let password_for_blocking = password.clone();
    let mnemonic_for_blocking = mnemonic.clone();
    let (ks, w) = tokio::task::spawn_blocking(move || {
        let w = wallet::wallet_from_mnemonic(&mnemonic_for_blocking).map_err(|e| e.to_string())?;
        #[allow(unused_mut)]
        let mut ks = Keystore::create(&paths_for_blocking.vault_dir, &password_for_blocking)
            .map_err(|e| e.to_string())?;
        ks.store_mnemonic(&mnemonic_for_blocking)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((ks, w))
    })
    .await
    .map_err(|e| format!("blocking task failed: {e}"))?
    .map_err(|e: String| e)?;

    emit_progress(&app, "db", "Opening encrypted database...");
    state
        .start_active_profile(paths.clone(), ks)
        .await
        .map_err(|e| format!("failed to bring profile online: {e}"))?;

    {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "INSERT OR REPLACE INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                params![w.stake_address.clone(), w.payment_address.clone()],
            )
            .map_err(|e| e.to_string())?;

        #[cfg(feature = "dev-seed")]
        {
            let did = crate::crypto::did::derive_did_key(&w.signing_key);
            let _ =
                crate::db::seed::bind_current_user_to_seed_with_did(db.conn(), Some(did.as_str()));
        }
    }

    state
        .profile_manager
        .touch_unlocked(&paths.id)
        .map_err(|e| e.to_string())?;

    emit_progress(&app, "done", "Profile restored");

    Ok(UnlockProfileResponse {
        wallet: WalletInfo {
            stake_address: w.stake_address.clone(),
            payment_address: w.payment_address.clone(),
            has_mnemonic_backup: true,
        },
        profile: read_profile_from_db(&state)?,
    })
}

/// Unlock an existing profile with its password.
#[tauri::command]
pub async fn unlock_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    password: String,
) -> Result<UnlockProfileResponse, String> {
    let id = ProfileId::parse(&id).map_err(|e| e.to_string())?;

    // Lock any currently-active profile first.
    if state.active_id().is_some() {
        state.stop_active_profile().await?;
    }

    let paths = state.profile_manager.paths_for(&id);
    if state.profile_manager.get(&id).is_none() {
        return Err(format!("profile {id} not found"));
    }

    emit_progress(&app, "vault", "Decrypting vault...");
    let paths_for_blocking = paths.clone();
    let password_for_blocking = password.clone();
    let (ks, w) = tokio::task::spawn_blocking(move || {
        let ks = Keystore::open(&paths_for_blocking.vault_dir, &password_for_blocking)
            .map_err(|e| e.to_string())?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        Ok::<_, String>((ks, w))
    })
    .await
    .map_err(|e| format!("blocking task failed: {e}"))?
    .map_err(|e: String| e)?;

    emit_progress(&app, "db", "Opening encrypted database...");
    state.start_active_profile(paths.clone(), ks).await?;

    // Re-create identity row if missing (recovery scenario).
    {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let exists: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) > 0 FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if !exists {
            db.conn()
                .execute(
                    "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, ?1, ?2)",
                    params![w.stake_address.clone(), w.payment_address.clone()],
                )
                .map_err(|e| e.to_string())?;
        }

        #[cfg(feature = "dev-seed")]
        {
            let did = crate::crypto::did::derive_did_key(&w.signing_key);
            let _ =
                crate::db::seed::bind_current_user_to_seed_with_did(db.conn(), Some(did.as_str()));
        }
    }

    state
        .profile_manager
        .touch_unlocked(&paths.id)
        .map_err(|e| e.to_string())?;

    emit_progress(&app, "done", "Profile unlocked");

    Ok(UnlockProfileResponse {
        wallet: WalletInfo {
            stake_address: w.stake_address.clone(),
            payment_address: w.payment_address.clone(),
            has_mnemonic_backup: true,
        },
        profile: read_profile_from_db(&state)?,
    })
}

/// Lock the currently-active profile. Idempotent.
#[tauri::command]
pub async fn lock_profile(state: State<'_, AppState>) -> Result<(), String> {
    state.stop_active_profile().await
}

/// Rename a profile. Works on any profile, active or not.
#[tauri::command]
pub async fn rename_profile(
    state: State<'_, AppState>,
    id: String,
    display_name: String,
) -> Result<ProfileSummary, String> {
    let id = ProfileId::parse(&id).map_err(|e| e.to_string())?;
    state
        .profile_manager
        .rename(&id, &display_name)
        .map_err(|e| e.to_string())?;
    state
        .profile_manager
        .get(&id)
        .ok_or_else(|| format!("profile {id} disappeared after rename"))
}

/// Update a profile's avatar. Works on any profile, active or not.
#[tauri::command]
pub async fn set_profile_avatar(
    state: State<'_, AppState>,
    id: String,
    avatar: Avatar,
) -> Result<ProfileSummary, String> {
    let id = ProfileId::parse(&id).map_err(|e| e.to_string())?;
    state
        .profile_manager
        .set_avatar(&id, avatar)
        .map_err(|e| e.to_string())?;
    state
        .profile_manager
        .get(&id)
        .ok_or_else(|| format!("profile {id} disappeared after avatar update"))
}

/// Delete a profile. Refuses to delete the currently-active profile —
/// caller must lock first. Verifies the supplied password by attempting
/// a vault open; this prevents accidental deletion by a different user.
#[tauri::command]
pub async fn delete_profile(
    state: State<'_, AppState>,
    id: String,
    password: String,
) -> Result<(), String> {
    let id = ProfileId::parse(&id).map_err(|e| e.to_string())?;

    if let Some(active) = state.active_id() {
        if active == id {
            return Err("cannot delete the active profile — lock it first".to_string());
        }
    }

    // Verify the password actually unlocks the vault. If the directory
    // is corrupt or the password is wrong we refuse the delete.
    let paths = state.profile_manager.paths_for(&id);
    let paths_for_blocking = paths.clone();
    let password_for_blocking = password.clone();
    let verify = tokio::task::spawn_blocking(move || {
        let ks = Keystore::open(&paths_for_blocking.vault_dir, &password_for_blocking)
            .map_err(|e| e.to_string())?;
        let _ = ks.lock();
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("blocking task failed: {e}"))?;
    verify?;

    state.profile_manager.delete(&id).map_err(|e| e.to_string())
}

fn read_profile_from_db(state: &State<'_, AppState>) -> Result<Option<Identity>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let Some(db) = db_guard.as_ref() else {
        return Ok(None);
    };
    Ok(db
        .conn()
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
        .ok())
}
