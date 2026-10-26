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
use alexandria_studio::access::{visible_lesson, Reader};
use alexandria_studio::{gate, provider as studio_provider, store as studio_store};
use rusqlite::Connection;
use serde::Serialize;
use tauri::ipc::Channel;

use crate::ai_access::{self, GuardianApproval, Surface};
use crate::ai_chat::run::{self, ChatEvent};
use crate::ai_chat::store::{self, ChatBinding, ChatMessage, ChatThread};
use crate::ai_chat::turn::{self, PreparedMessage, StartedRun};
use crate::ai_chat::vocab::{refuse, refused, ChatErrorCode};
use crate::ai_chat::{action, bank_guard, error_code, memory, now, ChatRuntime};
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

fn recover(conn: &Connection, runtime: &ChatRuntime) -> Result<(), String> {
    store::recover_interrupted(conn, &|id| runtime.is_live(id), &now())
        .map(|_| ())
        .map_err(|e| e.to_string())
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
        recover(conn, &runtime)?;
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
        search_threads(conn, &query)
    })
    .await
}

/// Search returns message text, so it is refused during an attempt.
pub(crate) fn search_threads(
    conn: &Connection,
    query: &str,
) -> Result<Vec<store::SearchHit>, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::search_threads(conn, query, 50).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_create_thread(
    state: State<'_, AppState>,
    connection_id: Option<String>,
    model_id: Option<String>,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.create-thread", move |conn| {
        create_thread(conn, connection_id.as_deref(), model_id.as_deref())
    })
    .await
}

pub(crate) fn create_thread(
    conn: &Connection,
    connection_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<ChatThread, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    let id = uuid::Uuid::new_v4().to_string();
    store::create_thread(conn, &id, connection_id, model_id, &now()).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct ThreadDetail {
    pub thread: ChatThread,
    pub messages: Vec<ChatMessage>,
    pub bindings: Vec<ChatBinding>,
    pub practice: Vec<store::PracticeView>,
    /// Memory suggestions from this chat that wait for the learner.
    pub memory_proposals: Vec<store::MemoryItem>,
    /// Notes and flashcards from this chat, proposed or saved.
    pub artifacts: Vec<store::Artifact>,
    pub active_run_id: Option<String>,
}

#[tauri::command]
pub async fn chat_get_thread(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<ThreadDetail, String> {
    let runtime = state.ai_chat.clone();
    with_conn(&state, "chat.get-thread", move |conn| {
        thread_detail(conn, &runtime, &thread_id)
    })
    .await
}

/// A thread's contents. Refused during an attempt but not flagged: the app
/// reloads the open thread by itself when starting an attempt cancels its
/// run.
pub(crate) fn thread_detail(
    conn: &Connection,
    runtime: &ChatRuntime,
    thread_id: &str,
) -> Result<ThreadDetail, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    recover(conn, runtime)?;
    let e = |e: store::StoreError| e.to_string();
    Ok(ThreadDetail {
        thread: store::thread(conn, thread_id).map_err(e)?,
        messages: store::messages(conn, thread_id).map_err(e)?,
        bindings: store::bindings(conn, thread_id).map_err(e)?,
        practice: store::practice_for_thread(conn, thread_id).map_err(e)?,
        memory_proposals: store::memory_proposals_for_thread(conn, thread_id).map_err(e)?,
        artifacts: store::artifacts_for_thread(conn, thread_id).map_err(e)?,
        active_run_id: store::active_run(conn, thread_id).map_err(e)?,
    })
}

#[tauri::command]
pub async fn chat_rename_thread(
    state: State<'_, AppState>,
    thread_id: String,
    title: String,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.rename-thread", move |conn| {
        rename_thread(conn, &thread_id, &title)
    })
    .await
}

pub(crate) fn rename_thread(
    conn: &Connection,
    thread_id: &str,
    title: &str,
) -> Result<ChatThread, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::rename_thread(conn, thread_id, title, &now()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_set_thread_model(
    state: State<'_, AppState>,
    thread_id: String,
    connection_id: Option<String>,
    model_id: Option<String>,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.set-thread-model", move |conn| {
        set_thread_model(conn, &thread_id, connection_id.as_deref(), model_id)
    })
    .await
}

pub(crate) fn set_thread_model(
    conn: &Connection,
    thread_id: &str,
    connection_id: Option<&str>,
    model_id: Option<String>,
) -> Result<ChatThread, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    if let Some(id) = connection_id {
        studio_store::connection(conn, id).map_err(|e| e.to_string())?;
    }
    let model_id = model_id.filter(|m| !m.trim().is_empty());
    if model_id.as_ref().is_some_and(|m| m.len() > 200) {
        return Err(refuse(
            ChatErrorCode::InvalidInput,
            "model identifier is too long",
        ));
    }
    store::set_thread_model(conn, thread_id, connection_id, model_id.as_deref(), &now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_delete_thread(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), String> {
    let runtime = state.ai_chat.clone();
    with_conn(&state, "chat.delete-thread", move |conn| {
        delete_thread(conn, &runtime, &thread_id)
    })
    .await
}

pub(crate) fn delete_thread(
    conn: &Connection,
    runtime: &ChatRuntime,
    thread_id: &str,
) -> Result<(), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    if let Some(run) = store::active_run(conn, thread_id).map_err(|e| e.to_string())? {
        runtime.cancel(&run);
    }
    store::delete_thread(conn, thread_id).map_err(|e| e.to_string())
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
        attach(conn, &thread_id, &course_id, &element_id)
    })
    .await
}

pub(crate) fn attach(
    conn: &Connection,
    thread_id: &str,
    course_id: &str,
    element_id: &str,
) -> Result<ChatBinding, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    let target = visible_lesson(conn, Reader::Chat, course_id, element_id).map_err(refused)?;
    let id = uuid::Uuid::new_v4().to_string();
    store::add_binding(
        conn,
        &id,
        thread_id,
        course_id,
        element_id,
        &target.title,
        &now(),
    )
    .map_err(|e| e.to_string())
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
        attach_goal(conn, &thread_id, &goal_id)
    })
    .await
}

pub(crate) fn attach_goal(
    conn: &Connection,
    thread_id: &str,
    goal_id: &str,
) -> Result<ChatBinding, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    let label = crate::ai_chat::goal::label(conn, goal_id)?
        .ok_or_else(|| refuse(ChatErrorCode::NotFound, "that goal no longer exists"))?;
    store::add_goal_binding(
        conn,
        &uuid::Uuid::new_v4().to_string(),
        thread_id,
        goal_id,
        &label,
        &now(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_detach(
    state: State<'_, AppState>,
    thread_id: String,
    binding_id: String,
) -> Result<(), String> {
    with_conn(&state, "chat.detach", move |conn| {
        detach(conn, &thread_id, &binding_id)
    })
    .await
}

pub(crate) fn detach(conn: &Connection, thread_id: &str, binding_id: &str) -> Result<(), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::remove_binding(conn, thread_id, binding_id, &now()).map_err(|e| e.to_string())
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
        models_source(conn, &connection_id)
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
        request_approval(conn, &connection_id)
    })
    .await
}

pub(crate) fn request_approval(
    conn: &Connection,
    connection_id: &str,
) -> Result<GuardianApproval, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    ai_access::request_approval(conn, connection_id).map_err(refused)
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

/// The connection and key to ask for models. Asking contacts the provider,
/// so it is refused during an attempt.
pub(crate) fn models_source(
    conn: &Connection,
    connection_id: &str,
) -> Result<(alexandria_studio::model::StudioConnection, Option<String>), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    let doc = studio_store::connection(conn, connection_id).map_err(|e| e.to_string())?;
    // Listing models contacts the provider with the stored key: never one
    // the learner may not connect at all.
    ai_access::may_connect(conn, &doc.value).map_err(refused)?;
    let key = studio_store::secret(conn, connection_id).map_err(|e| e.to_string())?;
    Ok((doc.value, key))
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
        set_thread_mode(conn, &thread_id, &mode)
    })
    .await
}

pub(crate) fn set_thread_mode(
    conn: &Connection,
    thread_id: &str,
    mode: &str,
) -> Result<ChatThread, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::set_thread_mode(conn, thread_id, mode, &now()).map_err(|e| e.to_string())
}

/// Checks a practice answer here, without asking a model.
#[tauri::command]
pub async fn chat_practice_attempt(
    state: State<'_, AppState>,
    item_id: String,
    answer: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-attempt", move |conn| {
        practice_attempt(conn, &item_id, &answer)
    })
    .await
}

pub(crate) fn practice_attempt(
    conn: &Connection,
    item_id: &str,
    answer: &str,
) -> Result<store::PracticeView, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatModel).map_err(refused)?;
    let id = uuid::Uuid::new_v4().to_string();
    store::practice_attempt(conn, item_id, &id, answer, &now()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_practice_hint(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-hint", move |conn| {
        practice_hint(conn, &item_id)
    })
    .await
}

pub(crate) fn practice_hint(
    conn: &Connection,
    item_id: &str,
) -> Result<store::PracticeView, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatModel).map_err(refused)?;
    store::practice_hint(conn, item_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_practice_reveal(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-reveal", move |conn| {
        practice_reveal(conn, &item_id)
    })
    .await
}

pub(crate) fn practice_reveal(
    conn: &Connection,
    item_id: &str,
) -> Result<store::PracticeView, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatModel).map_err(refused)?;
    store::practice_reveal(conn, item_id).map_err(|e| e.to_string())
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
        add_memory(conn, &kind, &content)
    })
    .await
}

pub(crate) fn add_memory(
    conn: &Connection,
    kind: &str,
    content: &str,
) -> Result<store::MemoryItem, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    let kind = memory::Kind::parse(kind)
        .ok_or_else(|| refuse(ChatErrorCode::InvalidInput, "unknown memory kind"))?;
    if bank_guard::matches_assessment_item(conn, content) {
        return Err(refuse(
            ChatErrorCode::AssessmentItem,
            "memory cannot hold assessment questions",
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    store::add_memory(conn, &id, kind, content, &now()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_approve_memory(
    state: State<'_, AppState>,
    memory_id: String,
    content: Option<String>,
) -> Result<store::MemoryItem, String> {
    with_conn(&state, "chat.approve-memory", move |conn| {
        approve_memory(conn, &memory_id, content.as_deref())
    })
    .await
}

pub(crate) fn approve_memory(
    conn: &Connection,
    memory_id: &str,
    content: Option<&str>,
) -> Result<store::MemoryItem, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    if content.is_some_and(|c| bank_guard::matches_assessment_item(conn, c)) {
        return Err(refuse(
            ChatErrorCode::AssessmentItem,
            "memory cannot hold assessment questions",
        ));
    }
    store::approve_memory(conn, memory_id, content, &now()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_update_memory(
    state: State<'_, AppState>,
    memory_id: String,
    content: Option<String>,
    active: Option<bool>,
) -> Result<store::MemoryItem, String> {
    with_conn(&state, "chat.update-memory", move |conn| {
        update_memory(conn, &memory_id, content.as_deref(), active)
    })
    .await
}

pub(crate) fn update_memory(
    conn: &Connection,
    memory_id: &str,
    content: Option<&str>,
    active: Option<bool>,
) -> Result<store::MemoryItem, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    if content.is_some_and(|c| bank_guard::matches_assessment_item(conn, c)) {
        return Err(refuse(
            ChatErrorCode::AssessmentItem,
            "memory cannot hold assessment questions",
        ));
    }
    store::update_memory(conn, memory_id, content, active, &now()).map_err(|e| e.to_string())
}

/// Forgets an item or dismisses a suggestion.
#[tauri::command]
pub async fn chat_forget_memory(
    state: State<'_, AppState>,
    memory_id: String,
) -> Result<(), String> {
    with_conn(&state, "chat.forget-memory", move |conn| {
        forget_memory(conn, &memory_id)
    })
    .await
}

pub(crate) fn forget_memory(conn: &Connection, memory_id: &str) -> Result<(), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::forget_memory(conn, memory_id).map_err(|e| e.to_string())
}

/// Saves a finished answer as a note.
#[tauri::command]
pub async fn chat_save_note(
    state: State<'_, AppState>,
    message_id: String,
    title: String,
) -> Result<store::Artifact, String> {
    with_conn(&state, "chat.save-note", move |conn| {
        save_note(conn, &message_id, &title)
    })
    .await
}

pub(crate) fn save_note(
    conn: &Connection,
    message_id: &str,
    title: &str,
) -> Result<store::Artifact, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    let origin = store::message(conn, message_id).map_err(|e| e.to_string())?;
    if bank_guard::matches_assessment_item(conn, &origin.content) {
        return Err(refuse(
            ChatErrorCode::AssessmentItem,
            "notes cannot hold assessment questions",
        ));
    }
    store::save_message_as_note(
        conn,
        &uuid::Uuid::new_v4().to_string(),
        message_id,
        title,
        &now(),
    )
    .map_err(|e| e.to_string())
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
        delete_artifact(conn, &artifact_id)
    })
    .await
}

pub(crate) fn delete_artifact(conn: &Connection, artifact_id: &str) -> Result<(), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::delete_artifact(conn, artifact_id).map_err(|e| e.to_string())
}

/// Every saved note and flashcard set, newest first.
#[tauri::command]
pub async fn chat_list_artifacts(
    state: State<'_, AppState>,
) -> Result<Vec<store::Artifact>, String> {
    with_conn(&state, "chat.list-artifacts", saved_artifacts).await
}

/// Saved notes and flashcards hold model-written text, so, like a thread's
/// contents, they are refused during an attempt without a flag.
pub(crate) fn saved_artifacts(conn: &Connection) -> Result<Vec<store::Artifact>, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::saved_artifacts(conn).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::turn::fixtures::*;
    use crate::db::Database;

    #[test]
    fn assessments_can_never_be_attached() {
        let conn = db();
        thread(&conn);
        let error = attach(&conn, "t", "course", "exam").unwrap_err();
        assert!(error.starts_with("invalid_input"));
        // Quizzes are monitored like assessments in the player.
        assert!(attach(&conn, "t", "course", "quiz")
            .unwrap_err()
            .starts_with("invalid_input"));
        assert!(store::bindings(&conn, "t").unwrap().is_empty());
    }

    /// Every chat command that reads a thread's contents or changes chat
    /// state is refused during an attempt. Those only the open panel sends
    /// are also flagged; reads the app may send by itself as the attempt
    /// starts are not. Status, thread and approval lists, drafts, memory
    /// listing and cancelling a run stay open.
    #[test]
    fn every_chat_command_that_touches_a_thread_is_refused_during_an_attempt() {
        let db = Database::open_in_memory().unwrap();
        seed(db.conn());
        let conn = db.conn();
        thread(conn);
        let before = store::thread(conn, "t").unwrap();
        let runtime = ChatRuntime::default();
        type Call<'a> = Box<dyn Fn() -> Result<(), String> + 'a>;
        let commands: Vec<(&str, bool, Call)> = vec![
            (
                "chat_create_thread",
                false,
                Box::new(|| create_thread(conn, Some("c1"), None).map(drop)),
            ),
            (
                "chat_get_thread",
                false,
                Box::new(|| thread_detail(conn, &runtime, "t").map(drop)),
            ),
            (
                "chat_search_threads",
                false,
                Box::new(|| search_threads(conn, "probability").map(drop)),
            ),
            (
                "chat_rename_thread",
                false,
                Box::new(|| rename_thread(conn, "t", "Renamed").map(drop)),
            ),
            (
                "chat_set_thread_model",
                false,
                Box::new(|| set_thread_model(conn, "t", Some("c1"), None).map(drop)),
            ),
            (
                "chat_set_thread_mode",
                false,
                Box::new(|| set_thread_mode(conn, "t", "practice").map(drop)),
            ),
            (
                "chat_delete_thread",
                false,
                Box::new(|| delete_thread(conn, &runtime, "t")),
            ),
            (
                "chat_attach_lesson",
                false,
                Box::new(|| attach(conn, "t", "course", "lesson").map(drop)),
            ),
            (
                "chat_attach_goal",
                false,
                Box::new(|| attach_goal(conn, "t", "g1").map(drop)),
            ),
            (
                "chat_detach",
                false,
                Box::new(|| detach(conn, "t", "binding")),
            ),
            (
                "chat_list_models",
                false,
                Box::new(|| models_source(conn, "c1").map(drop)),
            ),
            (
                "chat_request_guardian_approval",
                false,
                Box::new(|| request_approval(conn, "c1").map(drop)),
            ),
            (
                "chat_prepare_message",
                true,
                Box::new(|| prepare(conn, "Hello").map(drop)),
            ),
            (
                "chat_practice_attempt",
                true,
                Box::new(|| practice_attempt(conn, "item", "42").map(drop)),
            ),
            (
                "chat_practice_hint",
                true,
                Box::new(|| practice_hint(conn, "item").map(drop)),
            ),
            (
                "chat_practice_reveal",
                true,
                Box::new(|| practice_reveal(conn, "item").map(drop)),
            ),
            (
                "chat_add_memory",
                false,
                Box::new(|| add_memory(conn, "preference", "Short answers").map(drop)),
            ),
            (
                "chat_approve_memory",
                false,
                Box::new(|| approve_memory(conn, "memory", None).map(drop)),
            ),
            (
                "chat_update_memory",
                false,
                Box::new(|| update_memory(conn, "memory", None, Some(false)).map(drop)),
            ),
            (
                "chat_forget_memory",
                false,
                Box::new(|| forget_memory(conn, "memory")),
            ),
            (
                "chat_save_note",
                false,
                Box::new(|| save_note(conn, "message", "Note").map(drop)),
            ),
            (
                "chat_save_course_note",
                false,
                Box::new(|| {
                    crate::commands::course_notes::save_from_chat(conn, "artifact").map(drop)
                }),
            ),
            (
                "chat_allow_action",
                true,
                Box::new(|| action::allow(&db, "artifact", None, &now()).map(drop)),
            ),
            (
                "chat_decline_action",
                false,
                Box::new(|| action::decline(&db, "artifact", &now()).map(drop)),
            ),
            (
                "chat_delete_artifact",
                false,
                Box::new(|| delete_artifact(conn, "artifact")),
            ),
            (
                "chat_list_artifacts",
                false,
                Box::new(|| saved_artifacts(conn).map(drop)),
            ),
        ];
        for (name, flags, call) in &commands {
            attempt(conn, name);
            let Err(err) = call() else {
                panic!("{name} is not refused during an attempt");
            };
            assert!(err.starts_with("assessment_active"), "{name}: {err}");
            assert_eq!(flagged(conn, name), *flags, "{name}");
        }
        let after = store::thread(conn, "t").unwrap();
        assert_eq!(
            (after.title, after.mode, after.connection_id),
            (before.title, before.mode, before.connection_id),
            "nothing changed"
        );
        // chat_start_run begins through the turn, which the turn's tests cover.
    }

    #[test]
    fn listing_models_never_reaches_a_provider_the_learner_may_not_connect() {
        let conn = db();
        connection(&conn, "c1", "cloud", "https://openrouter.ai/api/v1", None);
        assert!(models_source(&conn, "c1").is_ok());
        born_years_ago(&conn, 16);
        let err = models_source(&conn, "c1").unwrap_err();
        assert!(err.starts_with("adults_only"), "{err}");
        connection(&conn, "c1", "cloud", "https://api.openai.com/v1", None);
        assert!(
            models_source(&conn, "c1").is_ok(),
            "approval is for sending"
        );
        born_years_ago(&conn, 12);
        let err = models_source(&conn, "c1").unwrap_err();
        assert!(err.starts_with("under_13"), "{err}");
        connection(&conn, "c1", "local", "http://127.0.0.1:11434/v1", None);
        assert!(models_source(&conn, "c1").is_ok());
    }
}
