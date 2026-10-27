//! IPC commands for Verifiable Credentials.
//!
//! The public `#[tauri::command]` handlers are thin adapters — they
//! unlock the keystore, derive the issuer's signing key, and delegate
//! to pure functions that take `&Connection` + `&SigningKey` +
//! `&Did`. This split keeps the business logic unit-testable without
//! constructing a full `State<AppState>`.

use crate::clock::now_rfc3339;
use crate::profile::scope::ProfileState as State;
use ed25519_dalek::SigningKey;

use crate::credentials::{self, bundle, lifecycle, status_list, store, IssueCredentialRequest};
use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::domain::vc::{VerifiableCredential, VerificationResult};
use crate::AppState;

// --- internal helpers -----------------------------------------------------

/// Publish every pending list this identity owns, and record what landed.
///
/// Called after anything that creates or changes a list, and on demand. A
/// failed push is reported and left pending; the local revocation already
/// happened and stands regardless.
pub async fn publish_status_lists_for(
    state: &State<'_, AppState>,
) -> Result<status_list::StatusPublishReport, String> {
    let (key, issuer_did) = load_issuer_key(state).await?;
    let issuer_for_read = issuer_did.clone();
    let pending = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.status_lists.pending",
            move |db| status_list::pending(db.conn(), &issuer_for_read),
        )
        .await?;
    if pending.is_empty() {
        return Ok(status_list::StatusPublishReport::default());
    }
    let (accepted, errors) = status_list::push(&pending, &key, &issuer_did, &now_rfc3339()).await;
    let published = accepted.iter().map(|(id, _)| id.clone()).collect();
    if !accepted.is_empty() {
        state
            .db_executor
            .execute(
                DatabaseWorkload::Instructor,
                state.profile_lease(),
                "credentials.status_lists.mark",
                move |db| status_list::mark_published(db.conn(), &accepted),
            )
            .await?;
    }
    Ok(status_list::StatusPublishReport { published, errors })
}

/// Best-effort publication after a command that touched a list. The command's
/// own result is already decided; a host that is down is a warning here and a
/// pending list for the next pass.
async fn publish_status_lists_quietly(state: &State<'_, AppState>) {
    match publish_status_lists_for(state).await {
        Ok(report) if report.errors.is_empty() => {}
        Ok(report) => log::warn!("status list publication: {:?}", report.errors),
        Err(e) => log::warn!("status list publication: {e}"),
    }
}

pub(crate) async fn load_issuer_key(
    state: &State<'_, AppState>,
) -> Result<(SigningKey, Did), String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    // `Wallet` implements `Drop` (zeroize) so we can't move out — clone
    // the signing key bytes instead.
    let signing_key = SigningKey::from_bytes(&w.signing_key.to_bytes());
    let issuer_did = derive_did_key(&signing_key);
    Ok((signing_key, issuer_did))
}

// --- tauri command handlers ----------------------------------------------

#[tauri::command]
pub async fn issue_credential(
    state: State<'_, AppState>,
    req: IssueCredentialRequest,
) -> Result<VerifiableCredential, String> {
    let (signing_key, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    let issued = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.issue",
            move |db| credentials::issue(db.conn(), &signing_key, &issuer_did, &req, &now),
        )
        .await?;
    publish_status_lists_quietly(&state).await;
    super::organisations::sync_quietly(&state).await;
    Ok(issued)
}

#[tauri::command]
pub async fn list_credentials(
    state: State<'_, AppState>,
    subject: Option<String>,
    skill_id: Option<String>,
    active_only: Option<bool>,
) -> Result<Vec<VerifiableCredential>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.list",
            move |db| {
                store::list_filtered(
                    db.conn(),
                    subject.as_deref(),
                    skill_id.as_deref(),
                    active_only.unwrap_or(false),
                )
            },
        )
        .await
}

#[tauri::command]
pub async fn get_credential(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<Option<VerifiableCredential>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.get",
            move |db| store::get(db.conn(), &credential_id),
        )
        .await
}

#[tauri::command]
pub async fn revoke_credential(
    state: State<'_, AppState>,
    credential_id: String,
    reason: String,
) -> Result<(), String> {
    let (_, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.revoke",
            move |db| lifecycle::revoke(db.conn(), &issuer_did, &credential_id, &reason, &now),
        )
        .await?;
    publish_status_lists_quietly(&state).await;
    // A revoked credential stops counting, so what a synced grant shows changes.
    super::organisations::sync_quietly(&state).await;
    Ok(())
}

/// Push every status list that is ahead of what its host serves.
#[tauri::command]
pub async fn publish_status_lists(
    state: State<'_, AppState>,
) -> Result<status_list::StatusPublishReport, String> {
    publish_status_lists_for(&state).await
}

#[tauri::command]
pub async fn suspend_credential(
    state: State<'_, AppState>,
    credential_id: String,
    until: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let (_, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.suspend",
            move |db| {
                lifecycle::suspend(
                    db.conn(),
                    &issuer_did,
                    &credential_id,
                    until.as_deref(),
                    reason.as_deref(),
                    &now,
                )
            },
        )
        .await
}

#[tauri::command]
pub async fn reinstate_credential(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<(), String> {
    let (_, issuer_did) = load_issuer_key(&state).await?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.reinstate",
            move |db| lifecycle::reinstate(db.conn(), &issuer_did, &credential_id),
        )
        .await
}

/// Add a (credential_id, requestor_did) entry to the per-credential
/// vc-fetch allowlist. Pass the literal string `"public"` to mark
/// the credential as world-fetchable.
#[tauri::command]
pub async fn allow_credential_fetch(
    state: State<'_, AppState>,
    credential_id: String,
    requestor_did: String,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.allow_fetch",
            move |db| crate::p2p::vc_fetch::allow_fetch(db.conn(), &credential_id, &requestor_did),
        )
        .await
}

/// Remove a (credential_id, requestor_did) entry from the
/// allowlist. Idempotent.
#[tauri::command]
pub async fn disallow_credential_fetch(
    state: State<'_, AppState>,
    credential_id: String,
    requestor_did: String,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.disallow_fetch",
            move |db| {
                crate::p2p::vc_fetch::disallow_fetch(db.conn(), &credential_id, &requestor_did)
            },
        )
        .await
}

#[tauri::command]
pub async fn verify_credential_cmd(
    state: State<'_, AppState>,
    credential: VerifiableCredential,
) -> Result<VerificationResult, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.verify",
            move |db| {
                Ok(crate::domain::vc::verify_credential_db(
                    db.conn(),
                    &credential,
                    &now,
                    &crate::domain::vc::VerificationPolicy::default(),
                ))
            },
        )
        .await
}

#[tauri::command]
pub async fn export_credentials_bundle(state: State<'_, AppState>) -> Result<String, String> {
    let (key, did) = load_issuer_key(&state).await?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.export-bundle",
            move |db| bundle::export_signed(db.conn(), Some((&key, &did))),
        )
        .await
}
