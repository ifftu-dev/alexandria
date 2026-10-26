//! Commands for enrolment, progress and recorded answers. The behaviour
//! lives in [`crate::enrollment`].

use crate::profile::scope::ProfileState as State;

use crate::db::executor::DatabaseWorkload;
use crate::domain::enrollment::{ElementProgress, Enrollment, UpdateProgressRequest};
use crate::enrollment;
use crate::AppState;

/// List all enrollments for the local user.
#[tauri::command]
pub async fn list_enrollments(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Enrollment>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.list",
            move |db| enrollment::list(db, status),
        )
        .await
}

/// Enroll in a course.
#[tauri::command]
pub async fn enroll(state: State<'_, AppState>, course_id: String) -> Result<Enrollment, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.enroll",
            move |db| enrollment::enroll(db, &course_id),
        )
        .await
}

/// Update progress on a course element.
#[tauri::command]
pub async fn update_progress(
    state: State<'_, AppState>,
    enrollment_id: String,
    req: UpdateProgressRequest,
) -> Result<ElementProgress, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.update_progress",
            move |db| enrollment::update_progress(db, enrollment_id, req),
        )
        .await
}

/// Get all progress for an enrollment.
#[tauri::command]
pub async fn get_progress(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<Vec<ElementProgress>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.get_progress",
            move |db| enrollment::progress(db, &enrollment_id),
        )
        .await
}

/// Record a built-in assessment response. See
/// [`enrollment::record_submission`].
#[tauri::command]
pub async fn record_element_submission(
    state: State<'_, AppState>,
    enrollment_id: String,
    element_id: String,
    element_type: String,
    answers_json: String,
    score: f64,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.record_submission",
            move |db| {
                enrollment::record_submission(
                    db,
                    enrollment_id,
                    element_id,
                    element_type,
                    answers_json,
                    score,
                )
            },
        )
        .await
}

/// Return the learner's latest recorded response for an element so the
/// player can restore and display it on revisit. `None` if never submitted.
#[tauri::command]
pub async fn get_element_submission(
    state: State<'_, AppState>,
    enrollment_id: String,
    element_id: String,
) -> Result<Option<enrollment::ElementSubmissionRecord>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.get_submission",
            move |db| enrollment::latest_submission(db, &enrollment_id, &element_id),
        )
        .await
}
