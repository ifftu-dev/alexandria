//! Global AI chat commands.
//!
//! The flow is prepare → review → start, and the chat turn
//! ([`crate::ai_chat::turn`]) owns it:
//!
//! * `chat_prepare_message` gathers the turn, fetches lesson bodies outside
//!   the database, and prepares the request as a stored snapshot. Nothing is
//!   sent. The result tells the UI where the request would go, what it
//!   contains, and whether the learner must review it.
//! * `chat_start_run` takes the snapshot id and the digest the learner saw,
//!   begins the turn (the request is rebuilt and refused if anything it
//!   depends on changed), and hands it to a run
//!   ([`crate::ai_chat::run`]) that streams the answer over a
//!   request-scoped channel while saving checkpoints; the turn completes
//!   the answer.
//!
//! Every entry point refuses while an assessment is in progress.

use std::time::Duration;

use alexandria_ai::model::Limits;
use alexandria_studio::{gate, provider as studio_provider};
use rusqlite::Connection;
use serde::Serialize;
use tauri::ipc::Channel;

use crate::ai_access::{self, GuardianApproval};
use crate::ai_chat::run::{self, ChatEvent};
use crate::ai_chat::store::{self, ChatBinding, ChatThread};
use crate::ai_chat::turn::{self, PreparedMessage, StartedRun};
use crate::ai_chat::vocab::{refuse, refused, ChatErrorCode};
use crate::ai_chat::{action, error_code, now, ops};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

const CONTENT_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

async fn with_conn<T, F>(
    state: &State<'_, AppState>,
    label: &'static str,
    f: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            label,
            move |db| f(db.conn()),
        )
        .await
}

#[derive(Debug, Serialize)]
pub struct ChatStatus {
    pub assessment_active: bool,
}

#[tauri::command]
pub async fn chat_status(state: State<'_, AppState>) -> Result<ChatStatus, String> {
    with_conn(&state, "chat.status", |conn| {
        Ok(ChatStatus {
            assessment_active: gate::assessment_in_progress(conn).map_err(|e| e.to_string())?,
        })
    })
    .await
}

#[tauri::command]
pub async fn chat_list_threads(state: State<'_, AppState>) -> Result<Vec<ChatThread>, String> {
    let runtime = state.ai_chat.clone();
    with_conn(&state, "chat.list-threads", move |conn| {
        ops::recover(conn, &runtime)?;
        store::list_threads(conn, 200).map_err(|e| e.to_string())
    })
    .await
}

/// Searches the learner's own threads on this device. Nothing is sent.
#[tauri::command]
pub async fn chat_search_threads(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<store::SearchHit>, String> {
    with_conn(&state, "chat.search", move |conn| {
        ops::search_threads(conn, &query)
    })
    .await
}

#[tauri::command]
pub async fn chat_create_thread(
    state: State<'_, AppState>,
    connection_id: Option<String>,
    model_id: Option<String>,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.create-thread", move |conn| {
        ops::create_thread(conn, connection_id.as_deref(), model_id.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn chat_get_thread(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<ops::ThreadDetail, String> {
    let runtime = state.ai_chat.clone();
    with_conn(&state, "chat.get-thread", move |conn| {
        ops::thread_detail(conn, &runtime, &thread_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_rename_thread(
    state: State<'_, AppState>,
    thread_id: String,
    title: String,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.rename-thread", move |conn| {
        ops::rename_thread(conn, &thread_id, &title)
    })
    .await
}

#[tauri::command]
pub async fn chat_set_thread_model(
    state: State<'_, AppState>,
    thread_id: String,
    connection_id: Option<String>,
    model_id: Option<String>,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.set-thread-model", move |conn| {
        ops::set_thread_model(conn, &thread_id, connection_id.as_deref(), model_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_delete_thread(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), String> {
    let runtime = state.ai_chat.clone();
    with_conn(&state, "chat.delete-thread", move |conn| {
        ops::delete_thread(conn, &runtime, &thread_id)
    })
    .await
}

/// Attaches a published text lesson. Quizzes are monitored like assessments
/// in the course player, so their questions are never attached; neither are
/// assessments, interactive elements, or plugins.
#[tauri::command]
pub async fn chat_attach_lesson(
    state: State<'_, AppState>,
    thread_id: String,
    course_id: String,
    element_id: String,
) -> Result<ChatBinding, String> {
    with_conn(&state, "chat.attach-lesson", move |conn| {
        ops::attach(conn, &thread_id, &course_id, &element_id)
    })
    .await
}

/// Attaches one of the learner's goals. Its path is recomputed at every
/// prepare, so the model sees proven skills as they are then.
#[tauri::command]
pub async fn chat_attach_goal(
    state: State<'_, AppState>,
    thread_id: String,
    goal_id: String,
) -> Result<ChatBinding, String> {
    with_conn(&state, "chat.attach-goal", move |conn| {
        ops::attach_goal(conn, &thread_id, &goal_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_detach(
    state: State<'_, AppState>,
    thread_id: String,
    binding_id: String,
) -> Result<(), String> {
    with_conn(&state, "chat.detach", move |conn| {
        ops::detach(conn, &thread_id, &binding_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_get_draft(state: State<'_, AppState>, scope: String) -> Result<String, String> {
    with_conn(&state, "chat.get-draft", move |conn| {
        store::draft(conn, &scope).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn chat_save_draft(
    state: State<'_, AppState>,
    scope: String,
    text: String,
) -> Result<(), String> {
    with_conn(&state, "chat.save-draft", move |conn| {
        store::save_draft(conn, &scope, &text, &now()).map_err(|e| e.to_string())
    })
    .await
}

/// Lists models offered by a connection. Sends no learning data.
#[tauri::command]
pub async fn chat_list_models(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    let (connection, key) = with_conn(&state, "chat.list-models", move |conn| {
        ops::models_source(conn, &connection_id)
    })
    .await?;
    let target = studio_provider::connection(&connection).map_err(|e| {
        let reason = match &e {
            alexandria_studio::Error::Invalid(m) | alexandria_studio::Error::Unavailable(m) => {
                m.clone()
            }
            other => other.to_string(),
        };
        refuse(ChatErrorCode::of_connection(&e), reason)
    })?;
    // A bad endpoint is refused before a sign-in is renewed.
    let resolved = target
        .resolve()
        .await
        .map_err(|e| format!("{}: {e}", error_code(&e)))?;
    let key = crate::ai_auth::credential::bearer_for(
        &state.db_executor,
        &state.profile_lease(),
        &connection,
        key,
        || Ok(()),
    )
    .await?;
    let adapter = resolved
        .connect(key.as_deref(), Limits::default())
        .map_err(|e| format!("{}: {e}", error_code(&e)))?;
    adapter
        .list_models()
        .await
        .map_err(|e| format!("{}: {e}", error_code(&e)))
}

#[tauri::command]
pub async fn chat_request_guardian_approval(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<GuardianApproval, String> {
    with_conn(&state, "chat.request-approval", move |conn| {
        ops::request_approval(conn, &connection_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_list_guardian_approvals(
    state: State<'_, AppState>,
) -> Result<Vec<GuardianApproval>, String> {
    with_conn(&state, "chat.list-approvals", |conn| {
        ai_access::approvals(conn).map_err(refused)
    })
    .await
}

// ─── prepare ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn chat_prepare_message(
    state: State<'_, AppState>,
    thread_id: String,
    text: Option<String>,
    regenerate_of: Option<String>,
    excluded_bindings: Option<Vec<String>>,
    excluded_memory: Option<Vec<String>>,
    include_history: Option<bool>,
) -> Result<PreparedMessage, String> {
    let gathered = with_conn(&state, "chat.prepare.gather", move |conn| {
        turn::gather(
            conn,
            turn::Inputs {
                thread_id: &thread_id,
                text,
                regenerate_of,
                excluded_bindings: &excluded_bindings.unwrap_or_default(),
                excluded_memory: &excluded_memory.unwrap_or_default(),
                include_history: include_history.unwrap_or(true),
            },
        )
    })
    .await?;

    // Fetch lesson bodies that are not stored inline, outside the database.
    let mut fetched: Vec<turn::Fetched> = Vec::new();
    for blob in gathered.blobs_to_fetch() {
        let Some(blob) = blob else {
            fetched.push(None);
            continue;
        };
        let resolver = state.resolver.lock().await.as_ref().cloned();
        let content = match resolver {
            None => Err("content store unavailable".to_string()),
            Some(resolver) => tokio::time::timeout(CONTENT_FETCH_TIMEOUT, resolver.resolve(blob))
                .await
                .map_err(|_| "timed out".to_string())
                .and_then(|r| {
                    r.map(|r| r.bytes.to_vec())
                        .map_err(|_| "unavailable".to_string())
                }),
        };
        fetched.push(Some(content));
    }

    with_conn(&state, "chat.prepare.snapshot", move |conn| {
        turn::prepare(conn, gathered, &fetched)
    })
    .await
}

// ─── start ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn chat_start_run(
    state: State<'_, AppState>,
    prepare_id: String,
    digest: String,
    client_request_id: String,
    on_event: Channel<ChatEvent>,
) -> Result<StartedRun, String> {
    let runtime = state.ai_chat.clone();
    let started = {
        let runtime = runtime.clone();
        let client_request_id = client_request_id.clone();
        with_conn(&state, "chat.start", move |conn| {
            turn::begin(conn, &runtime, &prepare_id, &digest, &client_request_id)
        })
        .await
    };
    let started = match started {
        Ok(started) => started,
        Err(error) if error.starts_with("duplicate:") => {
            // The same send arrived twice. Report the existing run; send nothing.
            let run_id = error.trim_start_matches("duplicate:").to_string();
            return with_conn(&state, "chat.start.existing", move |conn| {
                let run = store::run_by_client_request(conn, &client_request_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| refuse(ChatErrorCode::NotFound, "run"))?;
                Ok(StartedRun {
                    run_id,
                    thread_id: run.thread_id,
                    user_message: store::message(conn, &run.user_message_id)
                        .map_err(|e| e.to_string())?,
                    assistant_message: store::message(conn, &run.assistant_message_id)
                        .map_err(|e| e.to_string())?,
                    existing: true,
                })
            })
            .await;
        }
        Err(error) => return Err(error),
    };
    let db = run::ExecutorStore {
        executor: state.db_executor.clone(),
        lease: state.profile_lease(),
    };
    let slot = match runtime.register(&started.run.run_id) {
        Ok(slot) => slot,
        Err(reason) => {
            run::abandon(&db, &started.run, "too_many_runs").await;
            return Err(reason.into());
        }
    };
    let response = started.run.clone();
    let (source, begun) = run::ProviderStream::new(db.clone(), started);
    let sink = move |event: ChatEvent| {
        // The panel may be closed; the answer still completes and is saved.
        let _ = on_event.send(event);
    };
    let lease = db.lease.clone();
    let task = run::Run::new(begun, db, source, sink, slot);
    state
        .profile_operations
        .spawn_job(async move {
            let _ = lease.run_until_closed(task.drive()).await;
        })
        .await;
    Ok(response)
}

#[tauri::command]
pub async fn chat_cancel_run(state: State<'_, AppState>, run_id: String) -> Result<bool, String> {
    Ok(state.ai_chat.cancel(&run_id))
}

#[tauri::command]
pub async fn chat_set_thread_mode(
    state: State<'_, AppState>,
    thread_id: String,
    mode: String,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.set-mode", move |conn| {
        ops::set_thread_mode(conn, &thread_id, &mode)
    })
    .await
}

/// Checks a practice answer here, without asking a model.
#[tauri::command]
pub async fn chat_practice_attempt(
    state: State<'_, AppState>,
    item_id: String,
    answer: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-attempt", move |conn| {
        ops::practice_attempt(conn, &item_id, &answer)
    })
    .await
}

#[tauri::command]
pub async fn chat_practice_hint(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-hint", move |conn| {
        ops::practice_hint(conn, &item_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_practice_reveal(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-reveal", move |conn| {
        ops::practice_reveal(conn, &item_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_list_memory(
    state: State<'_, AppState>,
) -> Result<Vec<store::MemoryItem>, String> {
    with_conn(&state, "chat.list-memory", |conn| {
        store::list_memory(conn).map_err(|e| e.to_string())
    })
    .await
}

/// The learner adds an item directly. It is approved: they wrote it.
#[tauri::command]
pub async fn chat_add_memory(
    state: State<'_, AppState>,
    kind: String,
    content: String,
) -> Result<store::MemoryItem, String> {
    with_conn(&state, "chat.add-memory", move |conn| {
        ops::add_memory(conn, &kind, &content)
    })
    .await
}

#[tauri::command]
pub async fn chat_approve_memory(
    state: State<'_, AppState>,
    memory_id: String,
    content: Option<String>,
) -> Result<store::MemoryItem, String> {
    with_conn(&state, "chat.approve-memory", move |conn| {
        ops::approve_memory(conn, &memory_id, content.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn chat_update_memory(
    state: State<'_, AppState>,
    memory_id: String,
    content: Option<String>,
    active: Option<bool>,
) -> Result<store::MemoryItem, String> {
    with_conn(&state, "chat.update-memory", move |conn| {
        ops::update_memory(conn, &memory_id, content.as_deref(), active)
    })
    .await
}

/// Forgets an item or dismisses a suggestion.
#[tauri::command]
pub async fn chat_forget_memory(
    state: State<'_, AppState>,
    memory_id: String,
) -> Result<(), String> {
    with_conn(&state, "chat.forget-memory", move |conn| {
        ops::forget_memory(conn, &memory_id)
    })
    .await
}

/// Saves a finished answer as a note.
#[tauri::command]
pub async fn chat_save_note(
    state: State<'_, AppState>,
    message_id: String,
    title: String,
) -> Result<store::Artifact, String> {
    with_conn(&state, "chat.save-note", move |conn| {
        ops::save_note(conn, &message_id, &title)
    })
    .await
}

/// Carries out a proposal from the chat, once the learner allows it. A new
/// goal carries the skills the learner picked. See [`crate::ai_chat::action`].
#[tauri::command]
pub async fn chat_allow_action(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    artifact_id: String,
    skill_ids: Option<Vec<String>>,
) -> Result<store::Artifact, String> {
    let allowed = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "chat.allow-action",
            move |db| action::allow(db, &artifact_id, skill_ids.as_deref(), &now()),
        )
        .await?;
    if allowed.goals_changed {
        use tauri::Emitter;
        let _ = app.emit(
            "settings-changed",
            serde_json::json!({ "key": "learner.targets" }),
        );
    }
    Ok(allowed.artifact)
}

/// Records that the learner declined a proposal from the chat.
#[tauri::command]
pub async fn chat_decline_action(
    state: State<'_, AppState>,
    artifact_id: String,
) -> Result<store::Artifact, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "chat.decline-action",
            move |db| action::decline(db, &artifact_id, &now()),
        )
        .await
}

/// Deletes a saved note or flashcard set.
#[tauri::command]
pub async fn chat_delete_artifact(
    state: State<'_, AppState>,
    artifact_id: String,
) -> Result<(), String> {
    with_conn(&state, "chat.delete-artifact", move |conn| {
        ops::delete_artifact(conn, &artifact_id)
    })
    .await
}

/// Every saved note and flashcard set, newest first.
#[tauri::command]
pub async fn chat_list_artifacts(
    state: State<'_, AppState>,
) -> Result<Vec<store::Artifact>, String> {
    with_conn(&state, "chat.list-artifacts", ops::saved_artifacts).await
}
