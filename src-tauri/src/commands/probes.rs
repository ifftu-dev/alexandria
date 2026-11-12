//! Probe commands: thin wrappers over [`crate::assessment::probes`] and the
//! verification read path in [`crate::sentinel::verification`].
//!
//! The learner's runner calls, in order, after submitting answers and before
//! ending monitoring: `assessment_probes_start`, then per probe
//! `assessment_probe_render` and `assessment_probe_answer`, then
//! `assessment_probes_finish`. Instructors author follow-ups and seeded
//! variants with the `assessment_item_probe_*` commands.

use crate::assessment::items::GradeEngine;
use crate::assessment::probes::{self, ItemProbe, ItemProbeInput, ProbeAnswerReceipt, ServedProbe};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::sentinel::verification::{session_verification_summary, VerificationSummary};
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn actor(db: &crate::db::Database) -> String {
    SettingsStore::get(db.conn(), keys::IDENTITY_LOCAL_DID)
}

/// Generate and serve the probes for a submitted attempt.
#[tauri::command]
pub async fn assessment_probes_start(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<Vec<ServedProbe>, String> {
    #[cfg(desktop)]
    let grader_runtime = state.grader_runtime.clone();
    let now = now_ms();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.probes.start",
            move |db| {
                #[cfg(desktop)]
                let engine = GradeEngine {
                    runtime: &grader_runtime,
                    budgets: Default::default(),
                };
                #[cfg(not(desktop))]
                let engine = GradeEngine::default();
                probes::start_probes_db(db, &engine, &attempt_id, &actor(db), now)
            },
        )
        .await
}

/// Mark a probe as shown; its time box starts now.
#[tauri::command]
pub async fn assessment_probe_render(
    state: State<'_, AppState>,
    probe_id: String,
) -> Result<ServedProbe, String> {
    let now = now_ms();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.probes.render",
            move |db| probes::render_probe_db(db, &probe_id, &actor(db), now),
        )
        .await
}

/// Answer a rendered probe. `first_input_ms` is the client's time from
/// render to the learner's first input.
#[tauri::command]
pub async fn assessment_probe_answer(
    state: State<'_, AppState>,
    probe_id: String,
    response: serde_json::Value,
    first_input_ms: Option<u64>,
) -> Result<ProbeAnswerReceipt, String> {
    #[cfg(desktop)]
    let grader_runtime = state.grader_runtime.clone();
    let now = now_ms();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.probes.answer",
            move |db| {
                #[cfg(desktop)]
                let engine = GradeEngine {
                    runtime: &grader_runtime,
                    budgets: Default::default(),
                };
                #[cfg(not(desktop))]
                let engine = GradeEngine::default();
                probes::answer_probe_db(
                    db,
                    &engine,
                    &probe_id,
                    &actor(db),
                    &response,
                    first_input_ms,
                    now,
                )
            },
        )
        .await
}

/// Close the probe step; unanswered probes are recorded as skipped.
#[tauri::command]
pub async fn assessment_probes_finish(
    state: State<'_, AppState>,
    attempt_id: String,
) -> Result<VerificationSummary, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.probes.finish",
            move |db| probes::finish_probes_db(db, &attempt_id, &actor(db)),
        )
        .await
}

/// Verification results recorded in a session's commitment chain.
#[tauri::command]
pub async fn integrity_verification_summary(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<VerificationSummary, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.verification-summary",
            move |db| session_verification_summary(db.conn(), &session_id),
        )
        .await
}

/// Authored follow-ups and variants for an item (instructors only).
#[tauri::command]
pub async fn assessment_item_probes_list(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Vec<ItemProbe>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.item-probes.list",
            move |db| probes::list_item_probes_db(db.conn(), &actor(db), &item_id),
        )
        .await
}

/// Create or edit an authored follow-up or variant (instructors only).
#[tauri::command]
pub async fn assessment_item_probe_save(
    state: State<'_, AppState>,
    input: ItemProbeInput,
) -> Result<ItemProbe, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.item-probes.save",
            move |db| probes::save_item_probe_db(db, &actor(db), &input),
        )
        .await
}

/// Delete an authored probe the instructor wrote.
#[tauri::command]
pub async fn assessment_item_probe_delete(
    state: State<'_, AppState>,
    probe_id: String,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "assessment.item-probes.delete",
            move |db| probes::delete_item_probe_db(db.conn(), &actor(db), &probe_id),
        )
        .await
}
