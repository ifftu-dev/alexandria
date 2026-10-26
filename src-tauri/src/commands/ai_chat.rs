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
//!   depends on changed), and streams the answer over a request-scoped
//!   channel while saving checkpoints; the turn completes the answer.
//!
//! Every entry point refuses while an assessment is in progress.

use std::time::{Duration, Instant};

use alexandria_ai::endpoint;
use alexandria_ai::model::Limits;
use alexandria_ai::Adapter;
use alexandria_ai::StreamEvent;
use alexandria_studio::access::{visible_lesson, Reader};
use alexandria_studio::{gate, provider as studio_provider, store as studio_store};
use rusqlite::Connection;
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use crate::ai_access::{self, GuardianApproval, Surface};
use crate::ai_chat::store::{self, ChatBinding, ChatMessage, ChatThread, Finish};
use crate::ai_chat::turn::{self, PreparedMessage, StartedRun};
use crate::ai_chat::{action, bank_guard, error_code, memory, now, ChatRuntime, RunGuard};
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::profile::scope::ProfileLease;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// Saved at least this often while streaming.
const CHECKPOINT_INTERVAL: Duration = Duration::from_millis(500);
const CHECKPOINT_BYTES: usize = 2_048;
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
        store::search_threads(conn, &query, 50).map_err(|e| e.to_string())
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
        ai_access::no_attempt_in_progress(conn, Surface::Chat)?;
        let id = uuid::Uuid::new_v4().to_string();
        store::create_thread(
            conn,
            &id,
            connection_id.as_deref(),
            model_id.as_deref(),
            &now(),
        )
        .map_err(|e| e.to_string())
    })
    .await
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
        recover(conn, &runtime)?;
        let e = |e: store::StoreError| e.to_string();
        Ok(ThreadDetail {
            thread: store::thread(conn, &thread_id).map_err(e)?,
            messages: store::messages(conn, &thread_id).map_err(e)?,
            bindings: store::bindings(conn, &thread_id).map_err(e)?,
            practice: store::practice_for_thread(conn, &thread_id).map_err(e)?,
            memory_proposals: store::memory_proposals_for_thread(conn, &thread_id).map_err(e)?,
            artifacts: store::artifacts_for_thread(conn, &thread_id).map_err(e)?,
            active_run_id: store::active_run(conn, &thread_id).map_err(e)?,
        })
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
        store::rename_thread(conn, &thread_id, &title, &now()).map_err(|e| e.to_string())
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
        if let Some(id) = &connection_id {
            studio_store::connection(conn, id).map_err(|e| e.to_string())?;
        }
        let model_id = model_id.filter(|m| !m.trim().is_empty());
        if model_id.as_ref().is_some_and(|m| m.len() > 200) {
            return Err("invalid_input: model identifier is too long".into());
        }
        store::set_thread_model(
            conn,
            &thread_id,
            connection_id.as_deref(),
            model_id.as_deref(),
            &now(),
        )
        .map_err(|e| e.to_string())
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
        if let Some(run) = store::active_run(conn, &thread_id).map_err(|e| e.to_string())? {
            runtime.cancel(&run);
        }
        store::delete_thread(conn, &thread_id).map_err(|e| e.to_string())
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
    ai_access::no_attempt_in_progress(conn, Surface::Chat)?;
    let target = visible_lesson(conn, Reader::Chat, course_id, element_id)?;
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
    ai_access::no_attempt_in_progress(conn, Surface::Chat)?;
    let label = crate::ai_chat::goal::label(conn, goal_id)?
        .ok_or_else(|| "not_found: that goal no longer exists".to_string())?;
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
        store::remove_binding(conn, &thread_id, &binding_id, &now()).map_err(|e| e.to_string())
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
        let doc = studio_store::connection(conn, &connection_id).map_err(|e| e.to_string())?;
        let key = studio_store::secret(conn, &connection_id).map_err(|e| e.to_string())?;
        Ok((doc.value, key))
    })
    .await?;
    let location = studio_provider::location(&connection).map_err(|e| e.to_string())?;
    let endpoint = endpoint::resolve(&connection.endpoint, location)
        .await
        .map_err(|e| format!("{}: {e}", error_code(&e)))?;
    let key = crate::ai_auth::credential::bearer_for(
        &state.db_executor,
        &state.profile_lease(),
        &connection,
        key,
    )
    .await?;
    Adapter::new(connection.protocol, endpoint, key)
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
        Ok(ai_access::request_approval(conn, &connection_id)?)
    })
    .await
}

#[tauri::command]
pub async fn chat_list_guardian_approvals(
    state: State<'_, AppState>,
) -> Result<Vec<GuardianApproval>, String> {
    with_conn(&state, "chat.list-approvals", |conn| {
        Ok(ai_access::approvals(conn)?)
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

#[derive(Debug, Clone, Serialize)]
pub struct ChatEvent {
    pub run_id: String,
    pub thread_id: String,
    pub seq: u64,
    #[serde(flatten)]
    pub body: ChatEventBody,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEventBody {
    RunStarted,
    TextDelta {
        text: String,
    },
    UsageUpdated {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    RunCompleted {
        finish: String,
    },
    RunFailed {
        error: String,
        partial: bool,
    },
    RunCancelled {
        error: Option<String>,
    },
}

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
                    .ok_or("not_found: run")?;
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
    let lease = state.profile_lease();
    let executor = state.db_executor.clone();
    let response = started.run.clone();
    let registered = runtime.register(&started.run.run_id);
    let (guard, cancel) = match registered {
        Ok(pair) => pair,
        Err(reason) => {
            let run = started.run.clone();
            finish(
                &executor,
                &lease,
                &run.run_id,
                &run.assistant_message.id,
                "failed",
                "",
                Some("too_many_runs"),
                None,
            )
            .await;
            return Err(reason.into());
        }
    };
    let task = Stream {
        executor,
        lease: lease.clone(),
        channel: on_event,
        started,
        cancel,
        _guard: guard,
    };
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

#[allow(clippy::too_many_arguments)]
async fn finish(
    executor: &DatabaseExecutor,
    lease: &ProfileLease,
    run_id: &str,
    assistant_message_id: &str,
    status: &str,
    content: &str,
    error: Option<&str>,
    usage: Option<alexandria_ai::Usage>,
) -> bool {
    let (run_id, message_id, status, content, error) = (
        run_id.to_string(),
        assistant_message_id.to_string(),
        status.to_string(),
        content.to_string(),
        error.map(str::to_owned),
    );
    executor
        .execute(
            DatabaseWorkload::Background,
            lease.clone(),
            "chat.finish",
            move |db| {
                store::finish_run(
                    db.conn(),
                    &Finish {
                        run_id: &run_id,
                        assistant_message_id: &message_id,
                        state: &status,
                        content: &content,
                        error: error.as_deref(),
                        input_tokens: usage.and_then(|u| u.input_tokens),
                        output_tokens: usage.and_then(|u| u.output_tokens),
                        now: &now(),
                    },
                )
                .map_err(|e| e.to_string())
            },
        )
        .await
        .unwrap_or(false)
}

struct Stream {
    executor: DatabaseExecutor,
    lease: ProfileLease,
    channel: Channel<ChatEvent>,
    started: turn::Started,
    cancel: tokio::sync::oneshot::Receiver<()>,
    _guard: RunGuard,
}

enum Outcome {
    Done(alexandria_ai::Result<alexandria_ai::Completion>),
    Stopped(Option<&'static str>),
}

impl Stream {
    fn emit(&self, seq: &mut u64, body: ChatEventBody) {
        *seq += 1;
        // The panel may be closed; the answer still completes and is saved.
        let _ = self.channel.send(ChatEvent {
            run_id: self.started.run.run_id.clone(),
            thread_id: self.started.run.thread_id.clone(),
            seq: *seq,
            body,
        });
    }

    /// Saves text so far. Returns false when an assessment has started and
    /// the answer must stop.
    async fn checkpoint(&self, text: &str) -> bool {
        let id = self.started.run.assistant_message.id.clone();
        let text = text.to_string();
        self.executor
            .execute(
                DatabaseWorkload::Background,
                self.lease.clone(),
                "chat.checkpoint",
                move |db| {
                    if gate::assessment_in_progress(db.conn()).unwrap_or(true) {
                        return Ok(false);
                    }
                    store::checkpoint(db.conn(), &id, &text).map_err(|e| e.to_string())?;
                    Ok(true)
                },
            )
            .await
            .unwrap_or(false)
    }

    /// Completes the answer through the turn, which stores it with its cards
    /// in one transaction.
    async fn finish_completion(&self, reply: &str, usage: Option<alexandria_ai::Usage>) {
        let started = self.started.clone();
        let reply = reply.to_string();
        let saved = self
            .executor
            .execute(
                DatabaseWorkload::Background,
                self.lease.clone(),
                "chat.finish",
                move |db| turn::complete(db.conn(), &started, &reply, usage),
            )
            .await;
        if let Err(error) = saved {
            log::warn!("chat: could not save an answer: {error}");
        }
    }

    async fn drive(mut self) {
        let mut seq = 0;
        self.emit(&mut seq, ChatEventBody::RunStarted);
        let run_id = self.started.run.run_id.clone();
        let message_id = self.started.run.assistant_message.id.clone();

        let endpoint = match endpoint::resolve(&self.started.endpoint, self.started.location).await
        {
            Ok(endpoint) => endpoint,
            Err(error) => {
                let code = error_code(&error);
                finish(
                    &self.executor,
                    &self.lease,
                    &run_id,
                    &message_id,
                    "failed",
                    "",
                    Some(&code),
                    None,
                )
                .await;
                self.emit(
                    &mut seq,
                    ChatEventBody::RunFailed {
                        error: code,
                        partial: false,
                    },
                );
                return;
            }
        };
        // A ChatGPT sign-in renews its hour-long token here, before sending.
        let key = match crate::ai_auth::credential::bearer_for(
            &self.executor,
            &self.lease,
            &self.started.connection,
            self.started.key.take(),
        )
        .await
        {
            Ok(key) => key,
            Err(error) => {
                let code = error.split(':').next().unwrap_or("auth").to_string();
                finish(
                    &self.executor,
                    &self.lease,
                    &run_id,
                    &message_id,
                    "failed",
                    "",
                    Some(&code),
                    None,
                )
                .await;
                self.emit(
                    &mut seq,
                    ChatEventBody::RunFailed {
                        error: code,
                        partial: false,
                    },
                );
                return;
            }
        };
        let adapter = Adapter::new(self.started.protocol, endpoint, key);
        let adapter = adapter.with_limits(Limits::default());
        let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);
        let request = self.started.request.clone();
        let stream = adapter.stream(&request, tx);
        tokio::pin!(stream);

        let mut text = String::new();
        let mut usage: Option<alexandria_ai::Usage> = None;
        let mut unsaved = 0usize;
        let mut last_save = Instant::now();
        let mut events_open = true;
        let outcome = loop {
            tokio::select! {
                biased;
                _ = &mut self.cancel => break Outcome::Stopped(None),
                event = rx.recv(), if events_open => match event {
                    Some(StreamEvent::TextDelta { text: delta }) => {
                        unsaved += delta.len();
                        text.push_str(&delta);
                        self.emit(&mut seq, ChatEventBody::TextDelta { text: delta });
                        if unsaved >= CHECKPOINT_BYTES || last_save.elapsed() >= CHECKPOINT_INTERVAL {
                            unsaved = 0;
                            last_save = Instant::now();
                            if !self.checkpoint(&text).await {
                                break Outcome::Stopped(Some("assessment_active"));
                            }
                        }
                    }
                    Some(StreamEvent::Usage { usage: u }) => {
                        usage = Some(u);
                        self.emit(&mut seq, ChatEventBody::UsageUpdated {
                            input_tokens: u.input_tokens,
                            output_tokens: u.output_tokens,
                        });
                    }
                    Some(StreamEvent::Finished { .. }) => {}
                    None => events_open = false,
                },
                result = &mut stream => {
                    // Deliver anything decoded before the stream returned.
                    while let Ok(event) = rx.try_recv() {
                        match event {
                            StreamEvent::TextDelta { text: delta } => {
                                text.push_str(&delta);
                                self.emit(&mut seq, ChatEventBody::TextDelta { text: delta });
                            }
                            StreamEvent::Usage { usage: u } => {
                                usage = Some(u);
                                self.emit(&mut seq, ChatEventBody::UsageUpdated {
                                    input_tokens: u.input_tokens,
                                    output_tokens: u.output_tokens,
                                });
                            }
                            StreamEvent::Finished { .. } => {}
                        }
                    }
                    break Outcome::Done(result);
                }
            }
        };

        match outcome {
            Outcome::Done(Ok(completion)) => {
                let usage = completion.usage.or(usage);
                let finish_reason = serde_json::to_value(&completion.finish)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".into());
                self.finish_completion(&completion.text, usage).await;
                self.emit(
                    &mut seq,
                    ChatEventBody::RunCompleted {
                        finish: finish_reason,
                    },
                );
            }
            Outcome::Done(Err(error)) => {
                let code = error_code(&error);
                let partial = !text.trim().is_empty();
                let status = if partial && error.may_have_been_accepted() {
                    "interrupted"
                } else {
                    "failed"
                };
                finish(
                    &self.executor,
                    &self.lease,
                    &run_id,
                    &message_id,
                    status,
                    &text,
                    Some(&code),
                    usage,
                )
                .await;
                self.emit(
                    &mut seq,
                    ChatEventBody::RunFailed {
                        error: code,
                        partial,
                    },
                );
            }
            Outcome::Stopped(reason) => {
                // Dropping the stream ends the local request. The provider may
                // still have computed (and billed) the answer.
                finish(
                    &self.executor,
                    &self.lease,
                    &run_id,
                    &message_id,
                    "cancelled",
                    &text,
                    Some(reason.unwrap_or("cancelled")),
                    usage,
                )
                .await;
                self.emit(
                    &mut seq,
                    ChatEventBody::RunCancelled {
                        error: reason.map(str::to_owned),
                    },
                );
            }
        }
    }
}

#[tauri::command]
pub async fn chat_set_thread_mode(
    state: State<'_, AppState>,
    thread_id: String,
    mode: String,
) -> Result<ChatThread, String> {
    with_conn(&state, "chat.set-mode", move |conn| {
        store::set_thread_mode(conn, &thread_id, &mode, &now()).map_err(|e| e.to_string())
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
        ai_access::no_attempt_in_progress(conn, Surface::Chat)?;
        let id = uuid::Uuid::new_v4().to_string();
        store::practice_attempt(conn, &item_id, &id, &answer, &now()).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn chat_practice_hint(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-hint", move |conn| {
        ai_access::no_attempt_in_progress(conn, Surface::Chat)?;
        store::practice_hint(conn, &item_id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn chat_practice_reveal(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<store::PracticeView, String> {
    with_conn(&state, "chat.practice-reveal", move |conn| {
        ai_access::no_attempt_in_progress(conn, Surface::Chat)?;
        store::practice_reveal(conn, &item_id).map_err(|e| e.to_string())
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
        let kind = memory::Kind::parse(&kind).ok_or("invalid_input: unknown memory kind")?;
        if bank_guard::matches_assessment_item(conn, &content) {
            return Err("assessment_item: memory cannot hold assessment questions".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        store::add_memory(conn, &id, kind, &content, &now()).map_err(|e| e.to_string())
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
        if content
            .as_deref()
            .is_some_and(|c| bank_guard::matches_assessment_item(conn, c))
        {
            return Err("assessment_item: memory cannot hold assessment questions".into());
        }
        store::approve_memory(conn, &memory_id, content.as_deref(), &now())
            .map_err(|e| e.to_string())
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
        if content
            .as_deref()
            .is_some_and(|c| bank_guard::matches_assessment_item(conn, c))
        {
            return Err("assessment_item: memory cannot hold assessment questions".into());
        }
        store::update_memory(conn, &memory_id, content.as_deref(), active, &now())
            .map_err(|e| e.to_string())
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
        store::forget_memory(conn, &memory_id).map_err(|e| e.to_string())
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
        save_note(conn, &message_id, &title)
    })
    .await
}

pub(crate) fn save_note(
    conn: &Connection,
    message_id: &str,
    title: &str,
) -> Result<store::Artifact, String> {
    let origin = store::message(conn, message_id).map_err(|e| e.to_string())?;
    if bank_guard::matches_assessment_item(conn, &origin.content) {
        return Err("assessment_item: notes cannot hold assessment questions".into());
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
        store::delete_artifact(conn, &artifact_id).map_err(|e| e.to_string())
    })
    .await
}

/// Every saved note and flashcard set, newest first.
#[tauri::command]
pub async fn chat_list_artifacts(
    state: State<'_, AppState>,
) -> Result<Vec<store::Artifact>, String> {
    with_conn(&state, "chat.list-artifacts", move |conn| {
        store::saved_artifacts(conn).map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::artifact;
    use crate::ai_chat::turn::fixtures::*;
    use alexandria_ai::Protocol;
    use std::sync::Arc;

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

    // ─── streaming, end to end ──────────────────────────────────────────

    use crate::db::Database;
    use crate::profile::scope::Admission;
    use tauri::ipc::InvokeResponseBody;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Harness {
        _dir: tempfile::TempDir,
        shared: Arc<std::sync::Mutex<Option<Database>>>,
        executor: DatabaseExecutor,
        lease: ProfileLease,
        _admission: Admission,
        runtime: Arc<ChatRuntime>,
    }

    impl Harness {
        fn new(endpoint: &str) -> Self {
            let dir = tempfile::TempDir::new().unwrap();
            let database = Database::open(&dir.path().join("profile.db")).unwrap();
            seed(database.conn());
            connection(database.conn(), "c1", "local", endpoint, None);
            let shared = Arc::new(std::sync::Mutex::new(Some(database)));
            let admission = Admission::default();
            let session = admission.close();
            assert!(admission.open(&session));
            let lease = admission.admit(&session).unwrap();
            Self {
                _dir: dir,
                executor: DatabaseExecutor::new(shared.clone()),
                shared,
                lease,
                _admission: admission,
                runtime: Arc::new(ChatRuntime::default()),
            }
        }

        fn conn<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
            let guard = self.shared.lock().unwrap();
            f(guard.as_ref().unwrap().conn())
        }

        fn database<T>(&self, f: impl FnOnce(&Database) -> T) -> T {
            let guard = self.shared.lock().unwrap();
            f(guard.as_ref().unwrap())
        }

        /// Prepares and begins a run, and returns the stream task with the
        /// events it will emit.
        fn begin(&self, text: &str) -> (Stream, Arc<std::sync::Mutex<Vec<serde_json::Value>>>) {
            self.begin_in("ask", text)
        }

        fn begin_in(
            &self,
            mode: &str,
            text: &str,
        ) -> (Stream, Arc<std::sync::Mutex<Vec<serde_json::Value>>>) {
            let started = self.conn(|conn| {
                if store::thread(conn, "t").is_err() {
                    thread(conn);
                    attach(conn, "t", "course", "lesson").unwrap();
                }
                store::set_thread_mode(conn, "t", mode, &now()).unwrap();
                let prepared = prepare(conn, text).unwrap();
                turn::begin(
                    conn,
                    &self.runtime,
                    &prepared.prepare_id,
                    &prepared.digest,
                    &uuid::Uuid::new_v4().to_string(),
                )
                .unwrap()
            });
            let (guard, cancel) = self.runtime.register(&started.run.run_id).unwrap();
            let events = Arc::new(std::sync::Mutex::new(Vec::new()));
            let sink = events.clone();
            let channel = Channel::new(move |body| {
                if let InvokeResponseBody::Json(json) = body {
                    sink.lock()
                        .unwrap()
                        .push(serde_json::from_str(&json).unwrap());
                }
                Ok(())
            });
            (
                Stream {
                    executor: self.executor.clone(),
                    lease: self.lease.clone(),
                    channel,
                    started,
                    cancel,
                    _guard: guard,
                },
                events,
            )
        }
    }

    /// Serves one streamed answer, word by word, pausing between words.
    async fn model(words: &'static [&'static str], pause_ms: u64) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1 << 16];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            for word in words {
                let delta = serde_json::json!({"choices": [{"delta": {"content": word}}]});
                let frame = format!(
                    "data: {delta}

"
                );
                if socket.write_all(frame.as_bytes()).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(pause_ms)).await;
            }
            let _ = socket
                .write_all(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":40,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n")
                .await;
        });
        endpoint
    }

    /// Serves one answer over the Anthropic Messages protocol, with a
    /// thinking block first, and reports the raw request.
    async fn anthropic_model(
        words: &'static [&'static str],
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (seen, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let _ = seen.send(String::from_utf8_lossy(&request).into_owned());
            let mut body = String::from(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":30,\"cache_read_input_tokens\":10}}}\n\n\
                 event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hidden\"}}\n\n",
            );
            for word in words {
                let delta = serde_json::json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": word}});
                body.push_str(&format!("event: content_block_delta\ndata: {delta}\n\n"));
            }
            body.push_str(
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n\
                 event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            );
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            let _ = socket.write_all(body.as_bytes()).await;
        });
        (endpoint, rx)
    }

    /// Serves one answer over the Responses protocol and reports the raw
    /// request.
    async fn responses_model() -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (seen, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let _ = seen.send(String::from_utf8_lossy(&request).into_owned());
            let body = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"On \"}\n\n\
                        data: {\"type\":\"response.output_text.delta\",\"delta\":\"plan.\"}\n\n\
                        data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":2}}}\n\n";
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            let _ = socket.write_all(body.as_bytes()).await;
        });
        (endpoint, rx)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_chatgpt_sign_in_streams_through_the_responses_route_with_its_token() {
        let (endpoint, seen) = responses_model().await;
        let harness = Harness::new(&endpoint);
        harness.conn(|conn| {
            let mut doc = studio_store::connection(conn, "c1").unwrap();
            doc.value.protocol = Protocol::Responses;
            doc.value.auth = alexandria_studio::model::ConnectionAuth::ChatgptPlan;
            let credential = crate::ai_auth::chatgpt::Credential {
                client_id: "oaiapp_9".into(),
                sub: "u".into(),
                email: None,
                host_id: "h".into(),
                access_token: "plan-token".into(),
                refresh_token: "r".into(),
                id_token: "i".into(),
                expires_at: crate::ai_auth::now_secs() + 3000,
                scopes: vec!["chatgpt.tokens.use.direct".into()],
            };
            studio_store::save_signed_in_connection(conn, &doc, &credential.encode()).unwrap();
            // The generic editor cannot swap the sign-in for a key.
            let doc = studio_store::connection(conn, "c1").unwrap();
            assert!(studio_store::save_connection(conn, &doc, Some("sk-x")).is_err());
        });
        let (task, _events) = harness.begin("Why?");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;

        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "complete");
        assert_eq!(saved.content, "On plan.");
        assert_eq!(
            (saved.input_tokens, saved.output_tokens),
            (Some(12), Some(2))
        );
        let raw = seen.await.unwrap();
        let head = raw[..raw.find("\r\n\r\n").unwrap()].to_ascii_lowercase();
        assert!(head.starts_with("post /v1/responses "));
        assert!(head.contains("authorization: bearer plan-token"));
        let body: serde_json::Value =
            serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["store"], false);
        assert!(body["instructions"]
            .as_str()
            .unwrap()
            .contains("study assistant"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_anthropic_connection_streams_text_and_lifts_the_instructions() {
        static WORDS: [&str; 2] = ["Rare ", "events."];
        let (endpoint, seen) = anthropic_model(&WORDS).await;
        let harness = Harness::new(&endpoint);
        harness.conn(|conn| {
            let mut doc = studio_store::connection(conn, "c1").unwrap();
            doc.value.protocol = Protocol::Anthropic;
            studio_store::save_connection(conn, &doc, Some("sk-ant-test")).unwrap();
        });
        let (task, _events) = harness.begin("Why?");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;

        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "complete");
        assert_eq!(saved.content, "Rare events.");
        assert_eq!(
            (saved.input_tokens, saved.output_tokens),
            (Some(40), Some(3))
        );
        let raw = seen.await.unwrap();
        let head = raw[..raw.find("\r\n\r\n").unwrap()].to_ascii_lowercase();
        assert!(head.starts_with("post /v1/messages "));
        assert!(head.contains("x-api-key: sk-ant-test"));
        let body: serde_json::Value =
            serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert!(
            body["system"]
                .as_str()
                .unwrap()
                .contains("<<<SOURCE id=\"S1\""),
            "instructions and sources are the system prompt"
        );
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.last().unwrap()["role"], "user");
        assert!(messages.iter().all(|m| m["role"] != "system"));
    }

    fn kinds(events: &[serde_json::Value]) -> Vec<String> {
        events
            .iter()
            .map(|e| e["kind"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_proposed_flashcard_set_waits_to_be_saved_and_outlives_its_chat() {
        static WORDS: [&str; 2] = [
            "Here are your cards.\n",
            "```artifact\n{\"type\":\"flashcards\",\"title\":\"Bayes terms\",\"cards\":[{\"front\":\"Prior\",\"back\":\"Belief before evidence\"}]}\n```",
        ];
        let endpoint = model(&WORDS, 1).await;
        let harness = Harness::new(&endpoint);
        let (task, _events) = harness.begin("Make flashcards");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;

        harness.conn(|conn| {
            let saved = store::message(conn, &message_id).unwrap();
            assert_eq!(saved.content, "Here are your cards.");
            let artifacts = store::artifacts_for_thread(conn, "t").unwrap();
            assert_eq!(artifacts.len(), 1);
            let proposal = &artifacts[0];
            assert_eq!(
                (proposal.status.as_str(), proposal.title.as_str()),
                ("proposed", "Bayes terms")
            );
            assert_eq!(
                proposal.sources.len(),
                1,
                "keeps the sources the answer carried"
            );
            assert!(store::saved_artifacts(conn).unwrap().is_empty());

            // A note from the same answer, and the proposal saved.
            let note = save_note(conn, &message_id, "Why rare events mislead").unwrap();
            assert_eq!(note.status, "saved");
            store::save_artifact(conn, &proposal.id, &now()).unwrap();
            assert_eq!(store::saved_artifacts(conn).unwrap().len(), 2);

            store::delete_thread(conn, "t").unwrap();
            let kept = store::saved_artifacts(conn).unwrap();
            assert_eq!(kept.len(), 2, "saved artifacts outlive the chat");
            assert!(kept
                .iter()
                .all(|a| a.thread_id.is_none() && a.message_id.is_none()));
        });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_proposed_goal_is_added_only_when_the_learner_allows_it() {
        static WORDS: [&str; 1] = [
            "A goal could be:\n```artifact\n{\"type\":\"goal\",\"title\":\"Data analyst\",\"text\":\"Analyse data with probability\"}\n```",
        ];
        let endpoint = model(&WORDS, 1).await;
        let harness = Harness::new(&endpoint);
        let (task, _events) = harness.begin("Help me pick a goal");
        task.drive().await;

        let id = harness.conn(|conn| {
            let artifacts = store::artifacts_for_thread(conn, "t").unwrap();
            assert_eq!(artifacts.len(), 1);
            assert_eq!(artifacts[0].status, "proposed");
            goal(conn);
            artifacts[0].id.clone()
        });
        harness.database(|db| {
            let picked = ["sk-counting".to_string()];
            let allowed = action::allow(db, &id, Some(&picked), &now()).unwrap();
            assert_eq!(allowed.artifact.status, "added");
            assert!(crate::ai_chat::goal::label(db.conn(), "g1")
                .unwrap()
                .is_some());
        });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_goal_change_resolves_skill_names_and_applies_only_when_allowed() {
        static WORDS: [&str; 1] = [
            "Try this:\n```artifact\n{\"type\":\"goal_change\",\"title\":\"data analyst\",\"add\":[\"counting outcomes\",\"Astrology\"],\"remove\":[],\"text\":\"Start from counting.\"}\n```",
        ];
        let endpoint = model(&WORDS, 1).await;
        let harness = Harness::new(&endpoint);
        harness.conn(|conn| {
            goal(conn);
            thread(conn);
            attach_goal(conn, "t", "g1").unwrap();
        });
        let (task, _events) = harness.begin("Add counting to my goal");
        task.drive().await;

        let id = harness.conn(|conn| {
            let artifacts = store::artifacts_for_thread(conn, "t").unwrap();
            assert_eq!(artifacts.len(), 1);
            let change = &artifacts[0];
            assert_eq!(change.title, "Data analyst");
            assert_eq!(
                change.body,
                artifact::Body::GoalChange {
                    goal_id: "g1".into(),
                    rename: None,
                    add: vec![artifact::SkillRef {
                        id: "sk-counting".into(),
                        name: "Counting outcomes".into()
                    }],
                    remove: vec![],
                    text: "Start from counting.".into(),
                },
                "unknown skill names are dropped"
            );
            assert!(store::save_artifact(conn, &change.id, &now()).is_err());
            change.id.clone()
        });
        harness.database(|db| {
            assert_eq!(
                action::allow(db, &id, None, &now()).unwrap().artifact.status,
                "added"
            );
            let skills: String = db
                .conn()
                .query_row(
                    "SELECT json_extract(value, '$[0].goal_skill_ids') FROM app_settings WHERE key = 'learner.targets'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(skills, r#"["sk-bayes","sk-counting"]"#);
        });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_course_note_goes_to_the_attached_lesson_and_is_dropped_without_one() {
        static WORDS: [&str; 1] = [
            "Here:\n```artifact\n{\"type\":\"course_note\",\"title\":\"Bayes\",\"text\":\"Update a prior.\"}\n```",
        ];
        let endpoint = model(&WORDS, 1).await;
        let harness = Harness::new(&endpoint);
        let (task, _events) = harness.begin("Add that to my notes");
        task.drive().await;
        harness.conn(|conn| {
            let artifacts = store::artifacts_for_thread(conn, "t").unwrap();
            assert_eq!(artifacts.len(), 1);
            assert_eq!(artifacts[0].title, "Bayes");
            assert!(matches!(
                &artifacts[0].body,
                artifact::Body::CourseNote { course_id, element_id, text, .. }
                    if course_id == "course" && element_id == "lesson" && text == "Update a prior."
            ));
        });

        let conn = db();
        thread(&conn);
        let draft = artifact::Draft::CourseNote {
            title: "Bayes".into(),
            text: "Update a prior.".into(),
        };
        assert!(
            turn::resolve_draft(&conn, "t", draft).unwrap().is_none(),
            "no lesson attached"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answer_streams_in_order_and_is_saved_with_its_usage() {
        let endpoint = model(&["Because ", "it is ", "rare."], 5).await;
        let harness = Harness::new(&endpoint);
        let (task, events) = harness.begin("Why?");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;

        let events = events.lock().unwrap().clone();
        assert_eq!(
            kinds(&events),
            [
                "run_started",
                "text_delta",
                "text_delta",
                "text_delta",
                "usage_updated",
                "run_completed"
            ]
        );
        let seqs: Vec<u64> = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
        assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1));
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "complete");
        assert_eq!(saved.content, "Because it is rare.");
        assert_eq!(
            (saved.input_tokens, saved.output_tokens),
            (Some(40), Some(3))
        );
        assert_eq!(saved.sources.len(), 1);
        assert!(harness
            .conn(|c| store::active_run(c, "t").unwrap())
            .is_none());
        // The task left the registry, so recovery treats nothing as live.
        assert!(harness.runtime.has_capacity());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stopping_keeps_the_partial_answer_and_records_the_stop() {
        let endpoint = model(&["One ", "two ", "three ", "four ", "five "], 150).await;
        let harness = Harness::new(&endpoint);
        let (task, events) = harness.begin("Count");
        let run_id = task.started.run.run_id.clone();
        let message_id = task.started.run.assistant_message.id.clone();
        let runtime = harness.runtime.clone();
        let stopper = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(220)).await;
            assert!(runtime.cancel(&run_id));
        });
        task.drive().await;
        stopper.await.unwrap();
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "cancelled");
        assert!(saved.content.starts_with("One "));
        assert!(!saved.content.contains("five"));
        assert_eq!(
            kinds(&events.lock().unwrap()).last().unwrap(),
            "run_cancelled"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_assessment_starting_mid_answer_stops_it() {
        // A large first chunk forces a checkpoint, which checks the gate.
        static WORDS: [&str; 3] = ["Start ", "middle ", "end"];
        let endpoint = model(&WORDS, 700).await;
        let harness = Harness::new(&endpoint);
        let (task, events) = harness.begin("Explain");
        let message_id = task.started.run.assistant_message.id.clone();
        harness.conn(|c| {
            c.execute("INSERT INTO integrity_sessions(id) VALUES ('attempt')", [])
                .unwrap()
        });
        task.drive().await;
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "cancelled");
        assert_eq!(saved.error.as_deref(), Some("assessment_active"));
        let last = events.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last["kind"], "run_cancelled");
        assert_eq!(last["error"], "assessment_active");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreachable_local_model_fails_without_text() {
        let port = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let harness = Harness::new(&format!("http://127.0.0.1:{port}/v1"));
        let (task, events) = harness.begin("Hello");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(
            (saved.status.as_str(), saved.error.as_deref()),
            ("failed", Some("unreachable"))
        );
        let last = events.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            (last["kind"].as_str(), last["partial"].as_bool()),
            (Some("run_failed"), Some(false))
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_practice_answer_becomes_a_checkable_question_without_its_answer() {
        static WORDS: [&str; 3] = [
            "Here is one.\n\n",
            "```practice\n{\"skill\":\"Conditional probability\",\"kind\":\"numeric\",",
            "\"prompt\":\"A die shows an even number. What is P(greater than 3)?\",\"answer\":\"2/3\",\"hints\":[\"List the even faces.\"],\"explanation\":\"4 and 6.\"}\n```",
        ];
        let endpoint = model(&WORDS, 5).await;
        let harness = Harness::new(&endpoint);
        let (task, _) = harness.begin_in("practice", "Give me a question");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;
        let (message, items) = harness.conn(|c| {
            (
                store::message(c, &message_id).unwrap(),
                store::practice_for_thread(c, "t").unwrap(),
            )
        });
        assert_eq!(message.content, "Here is one.");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].message_id, message_id);
        assert_eq!(items[0].kind, "numeric");
        assert_eq!(items[0].answer, None);
        let solved = harness
            .conn(|c| store::practice_attempt(c, &items[0].id, "x", "0.667", &now()).unwrap());
        assert!(solved.solved_independently);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_generated_question_that_resembles_an_assessment_item_is_dropped() {
        static WORDS: [&str; 2] = [
            "Try this.\n```practice\n{\"kind\":\"multiple_choice\",\"prompt\":\"A bag has 4 red and 6 blue marbles. Two are drawn without replacement. What is the probability the second is red given the first was red?\",",
            "\"options\":[\"4/10\",\"3/9\"],\"answer\":1}\n```",
        ];
        let endpoint = model(&WORDS, 5).await;
        let harness = Harness::new(&endpoint);
        harness.conn(add_bank_item);
        let (task, _) = harness.begin_in("practice", "Another one please");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;
        let (message, items) = harness.conn(|c| {
            (
                store::message(c, &message_id).unwrap(),
                store::practice_for_thread(c, "t").unwrap(),
            )
        });
        assert!(items.is_empty());
        assert_eq!(message.error.as_deref(), Some("practice_rejected"));
        assert!(!message.content.contains("4 red and 6 blue"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_malformed_practice_block_leaves_the_reply_as_written() {
        static WORDS: [&str; 1] = ["Sure! What is 2+2? (answer: 4)"];
        let endpoint = model(&WORDS, 5).await;
        let harness = Harness::new(&endpoint);
        let (task, _) = harness.begin_in("practice", "Quiz me");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;
        let message = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(message.content, "Sure! What is 2+2? (answer: 4)");
        assert_eq!(message.error, None);
        assert!(harness
            .conn(|c| store::practice_for_thread(c, "t").unwrap())
            .is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_memory_suggestion_waits_for_the_learner_and_is_not_shown_as_text() {
        static WORDS: [&str; 2] = [
            "Sure, here it is with numbers.\n",
            "```memory\n{\"kind\":\"preference\",\"content\":\"Prefers worked numbers before formulas\"}\n```",
        ];
        let endpoint = model(&WORDS, 5).await;
        let harness = Harness::new(&endpoint);
        let (task, _) = harness.begin("Use numbers please");
        let message_id = task.started.run.assistant_message.id.clone();
        task.drive().await;
        let (message, proposals, sendable) = harness.conn(|c| {
            (
                store::message(c, &message_id).unwrap(),
                store::memory_proposals_for_thread(c, "t").unwrap(),
                store::sendable_memory(c).unwrap(),
            )
        });
        assert_eq!(message.content, "Sure, here it is with numbers.");
        assert_eq!(proposals.len(), 1);
        assert_eq!(
            proposals[0].source_message_id.as_deref(),
            Some(message_id.as_str())
        );
        assert!(
            sendable.is_empty(),
            "nothing is sent until the learner saves it"
        );
    }
}
