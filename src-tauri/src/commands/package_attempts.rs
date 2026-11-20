//! Package attempts: open or resume an attempt at a package element, hand a
//! launch its initialisation data, persist the wrapper's coalesced commits,
//! end a launch and start over. The logic is in
//! [`crate::packages::attempts`]; these wrappers run it on the executor.

use crate::credentials::Issuer;
use crate::db::executor::DatabaseWorkload;
use crate::domain::package_attempt::{
    PackageActivityResult, PackageAttempt, PackageAttemptProgress, PackageCommit,
    PackageLaunchInit, PackageSequencingState,
};
use crate::packages::{attempts, ExecutorDb, PackageDirs};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

fn db<'a>(state: &'a State<'_, AppState>) -> ExecutorDb<'a> {
    super::executor_db(state, DatabaseWorkload::Learner)
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
/// key. `resumed` (SCORM 2004 only) says the sequencing engine delivered the
/// activity by resuming it.
#[tauri::command]
pub async fn package_attempt_initialise(
    state: State<'_, AppState>,
    attempt_id: String,
    activity_id: String,
    resumed: Option<bool>,
) -> Result<PackageLaunchInit, String> {
    let (key, _) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    attempts::initialise_delivery(
        &db(&state),
        &key.to_bytes(),
        &attempt_id,
        &activity_id,
        resumed.unwrap_or(false),
    )
    .await
    .map_err(|e| e.to_string())
}

/// The SCORM 2004 sequencing engine's snapshot stored for an attempt, if
/// any.
#[tauri::command]
pub async fn package_attempt_sequencing(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<Option<String>, String> {
    attempts::sequencing(&db(&state), &attempt_id)
        .await
        .map_err(|e| e.to_string())
}

/// Stores the SCORM 2004 sequencing engine's state for an attempt and rolls
/// the attempt up from the root activity's status.
#[tauri::command]
pub async fn package_attempt_sequencing_save(
    state: State<'_, AppState>,
    sequencing: PackageSequencingState,
) -> Result<PackageAttemptProgress, String> {
    attempts::save_sequencing(&db(&state), sequencing)
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

/// What every lesson of an attempt has reported: the player's table of
/// contents.
#[tauri::command]
pub async fn package_attempt_activities(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<Vec<PackageActivityResult>, String> {
    attempts::activities(&db(&state), &attempt_id)
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
    // Any cmi5 session still open ends first (abandoned unless terminated).
    attempts::cmi5::end_attempt(
        &db(&state),
        &state.cmi5_sessions,
        &state.package_launches,
        &attempt_id,
    )
    .await
    .map_err(|e| e.to_string())?;
    attempts::terminate(&db(&state), &dirs, &state.package_launches, &attempt_id)
        .await
        .map_err(|e| e.to_string())
}
