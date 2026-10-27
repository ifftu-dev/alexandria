//! IPC commands for Verifiable Credentials.
//!
//! The handlers are thin adapters over [`crate::credentials`]: they read
//! the profile's [`Issuer`] from the keystore where one is needed, run the
//! credential operation on the profile database, and publish status lists
//! and sync organisations afterwards.

use crate::clock::now_rfc3339;
use crate::profile::scope::ProfileState as State;

use crate::credentials::{
    self, bundle, lifecycle, status_list, store, IssueCredentialRequest, Issuer,
};
use crate::db::executor::DatabaseWorkload;
use crate::domain::vc::{VerifiableCredential, VerificationResult};
use crate::AppState;

/// Best-effort publication after a command that touched a list, with the
/// issuer that command already read from the keystore. The command's own
/// result is already decided; a host that is down is a warning here and a
/// pending list for the next pass. `after` names the flow in the log
/// ("after grading").
pub(crate) async fn publish_status_lists_quietly(
    state: &State<'_, AppState>,
    issuer: &Issuer,
    after: &str,
) {
    match status_list::publish(&state.db_executor, &state.profile_lease(), issuer).await {
        Ok(report) if report.errors.is_empty() => {}
        Ok(report) => log::warn!("status list publication {after}: {:?}", report.errors),
        Err(e) => log::warn!("status list publication {after}: {e}"),
    }
}

// --- tauri command handlers ----------------------------------------------

#[tauri::command]
pub async fn issue_credential(
    state: State<'_, AppState>,
    req: IssueCredentialRequest,
) -> Result<VerifiableCredential, String> {
    let issuer = Issuer::from_keystore(&state.keystore).await?;
    let signer = issuer.clone();
    let now = now_rfc3339();
    let issued = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "credentials.issue",
            move |db| credentials::issue(db.conn(), &signer, &req, &now),
        )
        .await?;
    publish_status_lists_quietly(&state, &issuer, "after issue").await;
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
    let issuer = Issuer::from_keystore(&state.keystore).await?;
    let issuer_did = issuer.did().clone();
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
    publish_status_lists_quietly(&state, &issuer, "after revoke").await;
    // A revoked credential stops counting, so what a synced grant shows changes.
    super::organisations::sync_quietly(&state).await;
    Ok(())
}

/// Push every status list that is ahead of what its host serves.
#[tauri::command]
pub async fn publish_status_lists(
    state: State<'_, AppState>,
) -> Result<status_list::StatusPublishReport, String> {
    let issuer = Issuer::from_keystore(&state.keystore).await?;
    status_list::publish(&state.db_executor, &state.profile_lease(), &issuer).await
}

#[tauri::command]
pub async fn suspend_credential(
    state: State<'_, AppState>,
    credential_id: String,
    until: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let issuer_did = Issuer::from_keystore(&state.keystore).await?.did().clone();
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
    let issuer_did = Issuer::from_keystore(&state.keystore).await?.did().clone();
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
    let issuer = Issuer::from_keystore(&state.keystore).await?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "credentials.export-bundle",
            move |db| bundle::export_signed(db.conn(), Some(&issuer)),
        )
        .await
}
