//! Package attempts: open or resume an attempt at a package element, hand a
//! launch its initialisation data, persist the wrapper's coalesced commits,
//! end a launch and start over. The logic is in
//! [`crate::packages::attempts`]; these wrappers run it on the executor.

use crate::credentials::Issuer;
use crate::db::executor::DatabaseWorkload;
use crate::domain::package_attempt::{
    PackageAttempt, PackageAttemptProgress, PackageCommit, PackageLaunchInit,
};
use crate::packages::{attempts, ExecutorDb, PackageDirs};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

fn db<'a>(state: &'a State<'_, AppState>) -> ExecutorDb<'a> {
    ExecutorDb {
        executor: &state.db_executor,
        lease: state.profile_lease(),
        workload: DatabaseWorkload::Learner,
    }
}

/// Opens the learner's current attempt at a package element of an
/// enrolment, starting the first one when there is none.
#[tauri::command]
pub async fn package_attempt_open(
    state: State<'_, AppState>,
    enrollment_id: String,
    element_id: String,
) -> Result<PackageAttempt, String> {
    attempts::open(&db(&state), &enrollment_id, &element_id)
        .await
        .map_err(|e| e.to_string())
}

/// Closes the current attempt at a package element and starts a new one.
#[tauri::command]
pub async fn package_attempt_fresh(
    state: State<'_, AppState>,
    enrollment_id: String,
    element_id: String,
) -> Result<PackageAttempt, String> {
    attempts::fresh(&db(&state), &enrollment_id, &element_id)
        .await
        .map_err(|e| e.to_string())
}

/// The initialisation data for one launch of one activity: the pseudonymous
/// learner identifier, display name, launch values and prior state. Needs
/// the unlocked vault, because the identifier is derived from the learner's
/// key.
#[tauri::command]
pub async fn package_attempt_initialise(
    state: State<'_, AppState>,
    attempt_id: String,
    activity_id: String,
) -> Result<PackageLaunchInit, String> {
    let (key, _) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    attempts::initialise(&db(&state), &key.to_bytes(), &attempt_id, &activity_id)
        .await
        .map_err(|e| e.to_string())
}

/// Persists one coalesced commit from the wrapper in one write.
#[tauri::command]
pub async fn package_attempt_commit(
    state: State<'_, AppState>,
    commit: PackageCommit,
) -> Result<PackageAttemptProgress, String> {
    attempts::commit(&db(&state), commit)
        .await
        .map_err(|e| e.to_string())
}

/// Ends a launch of an attempt; drops the package's extracted files when the
/// package is complete and no launch of it is open.
#[tauri::command]
pub async fn package_attempt_terminate(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<PackageAttemptProgress, String> {
    let dirs = PackageDirs::for_profile(&state.active_paths()?);
    attempts::terminate(&db(&state), &dirs, &state.package_launches, &attempt_id)
        .await
        .map_err(|e| e.to_string())
}
