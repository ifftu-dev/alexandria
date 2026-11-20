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
            move |db| enrollment::list(db, status).map_err(|e| e.to_string()),
        )
        .await
}

/// Enroll in a course. The archives of the course's standards packages are
/// then fetched in the background, so they play offline later; the player
/// fetches any still missing when it opens one.
#[tauri::command]
pub async fn enroll(state: State<'_, AppState>, course_id: String) -> Result<Enrollment, String> {
    let for_db = course_id.clone();
    let enrolled = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.enroll",
            move |db| enrollment::enroll(db, &for_db).map_err(|e| e.to_string()),
        )
        .await?;
    prefetch_packages(&state, course_id).await;
    Ok(enrolled)
}

/// Best effort: fetches and records the package archives a course carries.
async fn prefetch_packages(state: &State<'_, AppState>, course_id: String) {
    let Some(resolver) = state.resolver.lock().await.as_ref().cloned() else {
        return;
    };
    let Ok(paths) = state.active_paths() else {
        return;
    };
    let dirs = crate::packages::PackageDirs::for_profile(&paths);
    let db = crate::packages::OwnedExecutorDb {
        executor: state.db_executor.clone(),
        lease: state.profile_lease(),
        workload: DatabaseWorkload::Background,
    };
    let node = state.content_node.clone();
    tauri::async_runtime::spawn(async move {
        let hashes = crate::packages::course_packages(&db, &course_id)
            .await
            .unwrap_or_default();
        if hashes.is_empty() || dirs.ensure().is_err() {
            return;
        }
        for hash in hashes {
            if let Err(error) =
                crate::packages::get_or_receive(&db, &node, &dirs, &resolver, &hash).await
            {
                log::info!("package {hash} not fetched at enrolment: {error}");
            }
        }
    });
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
            move |db| {
                enrollment::update_progress(db, enrollment_id, req).map_err(|e| e.to_string())
            },
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
            move |db| enrollment::progress(db, &enrollment_id).map_err(|e| e.to_string()),
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
                .map_err(|e| e.to_string())
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
            move |db| {
                enrollment::latest_submission(db, &enrollment_id, &element_id)
                    .map_err(|e| e.to_string())
            },
        )
        .await
}
