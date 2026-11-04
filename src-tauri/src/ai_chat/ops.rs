//! What the learner does with their chats, outside a turn: threads, attached
//! lessons and goals, practice, memory, and saved notes.
//!
//! Each operation applies the assessment gate first, through
//! [`crate::ai_access::no_attempt_in_progress`], with the surface that says
//! whether a refusal is also flagged, and then reads or writes the chat
//! store. The commands in `commands::ai_chat` are thin wrappers that run
//! these on the profile's database; sending a message is the turn's
//! ([`super::turn`]) and streaming the answer is the run's ([`super::run`]).

use alexandria_studio::access::{visible_lesson, Reader};
use alexandria_studio::store as studio_store;
use rusqlite::Connection;
use serde::Serialize;

use super::store::{self, ChatBinding, ChatMessage, ChatThread, StoreError};
use super::vocab::{denial_code, ChatErrorCode};
use super::{bank_guard, memory, now, ChatRuntime};
use crate::ai_access::{self, AccessDenied, GuardianApproval, Surface};

/// Why a chat operation failed. Its display is the `code: reason` string
/// the webview reads; commands return it as that string.
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    /// The AI access policy refused it.
    #[error("{code}: {0}", code = denial_code(.0))]
    Denied(AccessDenied),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A Studio connection or key could not be read.
    #[error(transparent)]
    Studio(#[from] alexandria_studio::Error),
    /// The goal store's error, already in the chat vocabulary.
    #[error("{0}")]
    Goal(String),
    #[error("{}: that goal no longer exists", ChatErrorCode::NotFound)]
    GoalGone,
    #[error("{}: model identifier is too long", ChatErrorCode::InvalidInput)]
    ModelIdTooLong,
    #[error("{}: unknown memory kind", ChatErrorCode::InvalidInput)]
    UnknownMemoryKind,
    #[error(
        "{}: memory cannot hold assessment questions",
        ChatErrorCode::AssessmentItem
    )]
    AssessmentItemInMemory,
    #[error(
        "{}: notes cannot hold assessment questions",
        ChatErrorCode::AssessmentItem
    )]
    AssessmentItemInNote,
}

/// The assessment gate, applied first by every operation here.
fn gate(conn: &Connection, surface: Surface) -> Result<(), OpError> {
    ai_access::no_attempt_in_progress(conn, surface).map_err(OpError::Denied)
}

pub(crate) fn recover(conn: &Connection, runtime: &ChatRuntime) -> Result<(), OpError> {
    store::recover_interrupted(conn, &|id| runtime.is_live(id), &now())
        .map(|_| ())
        .map_err(OpError::from)
}

/// Search returns message text, so it is refused during an attempt.
pub(crate) fn search_threads(
    conn: &Connection,
    query: &str,
) -> Result<Vec<store::SearchHit>, OpError> {
    gate(conn, Surface::ChatState)?;
    store::search_threads(conn, query, 50).map_err(OpError::from)
}

pub(crate) fn create_thread(
    conn: &Connection,
    connection_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<ChatThread, OpError> {
    gate(conn, Surface::ChatState)?;
    let id = uuid::Uuid::new_v4().to_string();
    store::create_thread(conn, &id, connection_id, model_id, &now()).map_err(OpError::from)
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

/// A thread's contents. Refused during an attempt but not flagged: the app
/// reloads the open thread by itself when starting an attempt cancels its
/// run.
pub(crate) fn thread_detail(
    conn: &Connection,
    runtime: &ChatRuntime,
    thread_id: &str,
) -> Result<ThreadDetail, OpError> {
    gate(conn, Surface::ChatState)?;
    recover(conn, runtime)?;
    Ok(ThreadDetail {
        thread: store::thread(conn, thread_id)?,
        messages: store::messages(conn, thread_id)?,
        bindings: store::bindings(conn, thread_id)?,
        practice: store::practice_for_thread(conn, thread_id)?,
        memory_proposals: store::memory_proposals_for_thread(conn, thread_id)?,
        artifacts: store::artifacts_for_thread(conn, thread_id)?,
        active_run_id: store::active_run(conn, thread_id)?,
    })
}

pub(crate) fn rename_thread(
    conn: &Connection,
    thread_id: &str,
    title: &str,
) -> Result<ChatThread, OpError> {
    gate(conn, Surface::ChatState)?;
    store::rename_thread(conn, thread_id, title, &now()).map_err(OpError::from)
}

pub(crate) fn set_thread_model(
    conn: &Connection,
    thread_id: &str,
    connection_id: Option<&str>,
    model_id: Option<String>,
) -> Result<ChatThread, OpError> {
    gate(conn, Surface::ChatState)?;
    if let Some(id) = connection_id {
        studio_store::connection(conn, id)?;
    }
    let model_id = model_id.filter(|m| !m.trim().is_empty());
    if model_id.as_ref().is_some_and(|m| m.len() > 200) {
        return Err(OpError::ModelIdTooLong);
    }
    store::set_thread_model(conn, thread_id, connection_id, model_id.as_deref(), &now())
        .map_err(OpError::from)
}

pub(crate) fn delete_thread(
    conn: &Connection,
    runtime: &ChatRuntime,
    thread_id: &str,
) -> Result<(), OpError> {
    gate(conn, Surface::ChatState)?;
    if let Some(run) = store::active_run(conn, thread_id)? {
        runtime.cancel(&run);
    }
    store::delete_thread(conn, thread_id).map_err(OpError::from)
}

pub(crate) fn attach(
    conn: &Connection,
    thread_id: &str,
    course_id: &str,
    element_id: &str,
) -> Result<ChatBinding, OpError> {
    gate(conn, Surface::ChatState)?;
    let target =
        visible_lesson(conn, Reader::Chat, course_id, element_id).map_err(OpError::Denied)?;
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
    .map_err(OpError::from)
}

pub(crate) fn attach_goal(
    conn: &Connection,
    thread_id: &str,
    goal_id: &str,
) -> Result<ChatBinding, OpError> {
    gate(conn, Surface::ChatState)?;
    let label = crate::ai_chat::goal::label(conn, goal_id)
        .map_err(OpError::Goal)?
        .ok_or(OpError::GoalGone)?;
    store::add_goal_binding(
        conn,
        &uuid::Uuid::new_v4().to_string(),
        thread_id,
        goal_id,
        &label,
        &now(),
    )
    .map_err(OpError::from)
}

pub(crate) fn detach(conn: &Connection, thread_id: &str, binding_id: &str) -> Result<(), OpError> {
    gate(conn, Surface::ChatState)?;
    store::remove_binding(conn, thread_id, binding_id, &now()).map_err(OpError::from)
}

pub(crate) fn request_approval(
    conn: &Connection,
    connection_id: &str,
) -> Result<GuardianApproval, OpError> {
    gate(conn, Surface::ChatState)?;
    let doc = studio_store::connection(conn, connection_id)?;
    ai_access::request_approval(conn, &doc).map_err(OpError::Denied)
}

/// The connection and key to ask for models. Asking contacts the provider,
/// so it is refused during an attempt.
pub(crate) fn models_source(
    conn: &Connection,
    connection_id: &str,
) -> Result<(alexandria_studio::model::StudioConnection, Option<String>), OpError> {
    gate(conn, Surface::ChatState)?;
    let doc = studio_store::connection(conn, connection_id)?;
    // Listing models contacts the provider with the stored key: never one
    // the learner may not connect at all.
    ai_access::may_connect(conn, &doc.value).map_err(OpError::Denied)?;
    let key = studio_store::secret(conn, connection_id)?;
    Ok((doc.value, key))
}

pub(crate) fn set_thread_mode(
    conn: &Connection,
    thread_id: &str,
    mode: &str,
) -> Result<ChatThread, OpError> {
    gate(conn, Surface::ChatState)?;
    store::set_thread_mode(conn, thread_id, mode, &now()).map_err(OpError::from)
}

pub(crate) fn practice_attempt(
    conn: &Connection,
    item_id: &str,
    answer: &str,
) -> Result<store::PracticeView, OpError> {
    gate(conn, Surface::ChatModel)?;
    let id = uuid::Uuid::new_v4().to_string();
    store::practice_attempt(conn, item_id, &id, answer, &now()).map_err(OpError::from)
}

pub(crate) fn practice_hint(
    conn: &Connection,
    item_id: &str,
) -> Result<store::PracticeView, OpError> {
    gate(conn, Surface::ChatModel)?;
    store::practice_hint(conn, item_id).map_err(OpError::from)
}

pub(crate) fn practice_reveal(
    conn: &Connection,
    item_id: &str,
) -> Result<store::PracticeView, OpError> {
    gate(conn, Surface::ChatModel)?;
    store::practice_reveal(conn, item_id).map_err(OpError::from)
}

pub(crate) fn add_memory(
    conn: &Connection,
    kind: &str,
    content: &str,
) -> Result<store::MemoryItem, OpError> {
    gate(conn, Surface::ChatState)?;
    let kind = memory::Kind::parse(kind).ok_or(OpError::UnknownMemoryKind)?;
    if bank_guard::matches_assessment_item(conn, content) {
        return Err(OpError::AssessmentItemInMemory);
    }
    let id = uuid::Uuid::new_v4().to_string();
    store::add_memory(conn, &id, kind, content, &now()).map_err(OpError::from)
}

pub(crate) fn approve_memory(
    conn: &Connection,
    memory_id: &str,
    content: Option<&str>,
) -> Result<store::MemoryItem, OpError> {
    gate(conn, Surface::ChatState)?;
    if content.is_some_and(|c| bank_guard::matches_assessment_item(conn, c)) {
        return Err(OpError::AssessmentItemInMemory);
    }
    store::approve_memory(conn, memory_id, content, &now()).map_err(OpError::from)
}

pub(crate) fn update_memory(
    conn: &Connection,
    memory_id: &str,
    content: Option<&str>,
    active: Option<bool>,
) -> Result<store::MemoryItem, OpError> {
    gate(conn, Surface::ChatState)?;
    if content.is_some_and(|c| bank_guard::matches_assessment_item(conn, c)) {
        return Err(OpError::AssessmentItemInMemory);
    }
    store::update_memory(conn, memory_id, content, active, &now()).map_err(OpError::from)
}

pub(crate) fn forget_memory(conn: &Connection, memory_id: &str) -> Result<(), OpError> {
    gate(conn, Surface::ChatState)?;
    store::forget_memory(conn, memory_id).map_err(OpError::from)
}

pub(crate) fn save_note(
    conn: &Connection,
    message_id: &str,
    title: &str,
) -> Result<store::Artifact, OpError> {
    gate(conn, Surface::ChatState)?;
    let origin = store::message(conn, message_id)?;
    if bank_guard::matches_assessment_item(conn, &origin.content) {
        return Err(OpError::AssessmentItemInNote);
    }
    store::save_message_as_note(
        conn,
        &uuid::Uuid::new_v4().to_string(),
        message_id,
        title,
        &now(),
    )
    .map_err(OpError::from)
}

pub(crate) fn delete_artifact(conn: &Connection, artifact_id: &str) -> Result<(), OpError> {
    gate(conn, Surface::ChatState)?;
    store::delete_artifact(conn, artifact_id).map_err(OpError::from)
}

/// Saved notes and flashcards hold model-written text, so, like a thread's
/// contents, they are refused during an attempt without a flag.
pub(crate) fn saved_artifacts(conn: &Connection) -> Result<Vec<store::Artifact>, OpError> {
    gate(conn, Surface::ChatState)?;
    store::saved_artifacts(conn).map_err(OpError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_access::AccessDenied;
    use crate::ai_chat::action;
    use crate::ai_chat::turn::fixtures::*;
    use crate::db::Database;

    #[test]
    fn assessments_can_never_be_attached() {
        let conn = db();
        thread(&conn);
        assert!(matches!(
            attach(&conn, "t", "course", "exam"),
            Err(OpError::Denied(AccessDenied::LessonWithheld))
        ));
        // Quizzes are monitored like assessments in the player.
        assert!(matches!(
            attach(&conn, "t", "course", "quiz"),
            Err(OpError::Denied(AccessDenied::LessonWithheld))
        ));
        assert!(store::bindings(&conn, "t").unwrap().is_empty());
        // A malformed identifier reads as it did when the policy passed on
        // Studio's error.
        let malformed = attach(&conn, "t", "course", "").unwrap_err();
        assert!(matches!(
            malformed,
            OpError::Denied(AccessDenied::Invalid(_))
        ));
        assert_eq!(malformed.to_string(), "invalid_input: invalid identifier");
    }

    #[test]
    fn chat_operations_refuse_what_they_cannot_hold() {
        let conn = db();
        thread(&conn);
        assert!(matches!(
            set_thread_model(&conn, "t", Some("c1"), Some("m".repeat(201))),
            Err(OpError::ModelIdTooLong)
        ));
        assert!(matches!(
            add_memory(&conn, "secret", "Short answers"),
            Err(OpError::UnknownMemoryKind)
        ));
        let gone = attach_goal(&conn, "t", "missing").unwrap_err();
        assert!(matches!(gone, OpError::GoalGone), "{gone:?}");
        assert_eq!(gone.to_string(), "not_found: that goal no longer exists");
        assert!(matches!(
            rename_thread(&conn, "missing", "Renamed"),
            Err(OpError::Store(store::StoreError::NotFound))
        ));
        assert!(matches!(
            request_approval(&conn, "missing"),
            Err(OpError::Studio(alexandria_studio::Error::Unavailable(_)))
        ));
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
        type Call<'a> = Box<dyn Fn() -> Result<(), OpError> + 'a>;
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
            assert!(
                matches!(err, OpError::Denied(AccessDenied::AttemptInProgress)),
                "{name}: {err:?}"
            );
            assert!(err.to_string().starts_with("assessment_active"), "{name}");
            assert_eq!(flagged(conn, name), *flags, "{name}");
        }
        // The turn, course notes and chat actions are their own modules,
        // with string errors.
        type OtherCall<'a> = Box<dyn Fn() -> Result<(), String> + 'a>;
        let others: Vec<(&str, bool, OtherCall)> = vec![
            (
                "chat_prepare_message",
                true,
                Box::new(|| prepare(conn, "Hello").map(drop)),
            ),
            (
                "chat_save_course_note",
                false,
                Box::new(|| crate::course_notes::save_from_chat(conn, "artifact").map(drop)),
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
        ];
        for (name, flags, call) in &others {
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
        assert!(
            matches!(err, OpError::Denied(AccessDenied::AdultsOnly(_))),
            "{err:?}"
        );
        connection(&conn, "c1", "cloud", "https://api.openai.com/v1", None);
        assert!(
            models_source(&conn, "c1").is_ok(),
            "approval is for sending"
        );
        born_years_ago(&conn, 12);
        let err = models_source(&conn, "c1").unwrap_err();
        assert!(
            matches!(err, OpError::Denied(AccessDenied::TooYoungForRemote)),
            "{err:?}"
        );
        connection(&conn, "c1", "local", "http://127.0.0.1:11434/v1", None);
        assert!(models_source(&conn, "c1").is_ok());
    }
}
