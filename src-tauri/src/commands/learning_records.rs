//! The activity log: the learning records the app wrote about the learner.
//! The logic is in [`crate::learning_records`].

use crate::credentials::issuer::Issuer;
use crate::db::executor::DatabaseWorkload;
use crate::domain::learning_record::{
    LearningRecordEntry, LearningRecordExport, LearningRecordUsage,
};
use crate::learning_records::{self, activity_log};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// `limit` rows of the activity log from `offset`, newest first (at most
/// [`activity_log::MAX_PAGE`]).
#[tauri::command]
pub async fn learning_records_list(
    state: State<'_, AppState>,
    offset: u32,
    limit: u32,
) -> Result<Vec<LearningRecordEntry>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "learning-records.list",
            move |db| activity_log::page(db.conn(), offset, limit).map_err(|e| e.to_string()),
        )
        .await
}

/// Every record as xAPI statements, with a detached signature by the
/// learner's key over the statement file's exact bytes. Needs the vault
/// unlocked.
#[tauri::command]
pub async fn learning_records_export(
    state: State<'_, AppState>,
) -> Result<LearningRecordExport, String> {
    let (key, _) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    let export = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "learning-records.export",
            move |db| activity_log::export(db.conn(), &key).map_err(|e| e.to_string()),
        )
        .await?;
    Ok(LearningRecordExport {
        statements: String::from_utf8(export.statements).map_err(|e| e.to_string())?,
        signature: String::from_utf8(export.signature).map_err(|e| e.to_string())?,
    })
}

/// Deletes the records with these ids from this device. A deleted record
/// waiting to be sent is not sent. Returns how many were removed.
#[tauri::command]
pub async fn learning_records_delete(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<usize, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "learning-records.delete",
            move |db| activity_log::delete(db.conn(), &ids).map_err(|e| e.to_string()),
        )
        .await
}

/// Deletes every record in the activity log from this device.
#[tauri::command]
pub async fn learning_records_delete_all(state: State<'_, AppState>) -> Result<usize, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "learning-records.delete-all",
            move |db| activity_log::delete_all(db.conn()).map_err(|e| e.to_string()),
        )
        .await
}

/// The activity log's size and its visible limit.
#[tauri::command]
pub async fn learning_records_usage(
    state: State<'_, AppState>,
) -> Result<LearningRecordUsage, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "learning-records.usage",
            move |db| learning_records::usage(db.conn()).map_err(|e| e.to_string()),
        )
        .await
}
