//! The activity log: the learning records the app wrote about the learner.
//! The logic is in [`crate::learning_records`].

use crate::db::executor::DatabaseWorkload;
use crate::domain::learning_record::LearningRecordUsage;
use crate::learning_records;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

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
