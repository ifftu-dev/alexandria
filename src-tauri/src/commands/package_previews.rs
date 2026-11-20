//! Package previews: an author runs a package element of their own course
//! as a learner would see it, without an enrolment and without storing
//! anything. The same lifecycle as the attempt commands; the logic is in
//! [`crate::packages::preview`].

use crate::credentials::Issuer;
use crate::db::executor::DatabaseWorkload;
use crate::domain::package_attempt::{
    PackageActivityResult, PackageAttempt, PackageAttemptProgress, PackageCommit, PackageLaunchInit,
};
use crate::packages::{preview, ExecutorDb};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

fn db<'a>(state: &'a State<'_, AppState>) -> ExecutorDb<'a> {
    ExecutorDb {
        executor: &state.db_executor,
        lease: state.profile_lease(),
        workload: DatabaseWorkload::Instructor,
    }
}

/// Opens a preview of a package element of a course the active profile wrote.
#[tauri::command]
pub async fn package_preview_open(
    state: State<'_, AppState>,
    element_id: String,
) -> Result<PackageAttempt, String> {
    preview::open(&db(&state), &state.package_previews, &element_id)
        .await
        .map_err(|e| e.to_string())
}

/// Replaces the element's preview with a new one holding no state.
#[tauri::command]
pub async fn package_preview_fresh(
    state: State<'_, AppState>,
    element_id: String,
) -> Result<PackageAttempt, String> {
    preview::fresh(&db(&state), &state.package_previews, &element_id)
        .await
        .map_err(|e| e.to_string())
}

/// The initialisation data for one lesson of a preview. Needs the unlocked
/// vault: the learner identifier is derived from the profile's key, as for
/// an attempt.
#[tauri::command]
pub async fn package_preview_initialise(
    state: State<'_, AppState>,
    attempt_id: String,
    activity_id: String,
) -> Result<PackageLaunchInit, String> {
    let (key, _) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    preview::initialise(
        &db(&state),
        &state.package_previews,
        &key.to_bytes(),
        &attempt_id,
        &activity_id,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Holds one coalesced commit in the preview; nothing is stored.
#[tauri::command]
pub async fn package_preview_commit(
    state: State<'_, AppState>,
    commit: PackageCommit,
) -> Result<PackageAttemptProgress, String> {
    preview::commit(&state.package_previews, commit).map_err(|e| e.to_string())
}

/// What every lesson of a preview has reported.
#[tauri::command]
pub async fn package_preview_activities(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<Vec<PackageActivityResult>, String> {
    preview::activities(&state.package_previews, &attempt_id).map_err(|e| e.to_string())
}

/// Ends a preview and forgets its state.
#[tauri::command]
pub async fn package_preview_terminate(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<PackageAttemptProgress, String> {
    preview::terminate(&state.package_previews, &attempt_id).map_err(|e| e.to_string())
}
