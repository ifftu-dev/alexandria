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

use super::store::{self, ChatBinding, ChatMessage, ChatThread};
use super::vocab::{refuse, refused, ChatErrorCode};
use super::{bank_guard, memory, now, ChatRuntime};
use crate::ai_access::{self, GuardianApproval, Surface};

pub(crate) fn recover(conn: &Connection, runtime: &ChatRuntime) -> Result<(), String> {
    store::recover_interrupted(conn, &|id| runtime.is_live(id), &now())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Search returns message text, so it is refused during an attempt.
pub(crate) fn search_threads(
    conn: &Connection,
    query: &str,
) -> Result<Vec<store::SearchHit>, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::search_threads(conn, query, 50).map_err(|e| e.to_string())
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

pub(crate) fn rename_thread(
    conn: &Connection,
    thread_id: &str,
    title: &str,
) -> Result<ChatThread, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::rename_thread(conn, thread_id, title, &now()).map_err(|e| e.to_string())
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

pub(crate) fn detach(conn: &Connection, thread_id: &str, binding_id: &str) -> Result<(), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::remove_binding(conn, thread_id, binding_id, &now()).map_err(|e| e.to_string())
}

pub(crate) fn request_approval(
    conn: &Connection,
    connection_id: &str,
) -> Result<GuardianApproval, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    ai_access::request_approval(conn, connection_id).map_err(refused)
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

pub(crate) fn set_thread_mode(
    conn: &Connection,
    thread_id: &str,
    mode: &str,
) -> Result<ChatThread, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::set_thread_mode(conn, thread_id, mode, &now()).map_err(|e| e.to_string())
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

pub(crate) fn practice_hint(
    conn: &Connection,
    item_id: &str,
) -> Result<store::PracticeView, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatModel).map_err(refused)?;
    store::practice_hint(conn, item_id).map_err(|e| e.to_string())
}

pub(crate) fn practice_reveal(
    conn: &Connection,
    item_id: &str,
) -> Result<store::PracticeView, String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatModel).map_err(refused)?;
    store::practice_reveal(conn, item_id).map_err(|e| e.to_string())
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

pub(crate) fn forget_memory(conn: &Connection, memory_id: &str) -> Result<(), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::forget_memory(conn, memory_id).map_err(|e| e.to_string())
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

pub(crate) fn delete_artifact(conn: &Connection, artifact_id: &str) -> Result<(), String> {
    ai_access::no_attempt_in_progress(conn, Surface::ChatState).map_err(refused)?;
    store::delete_artifact(conn, artifact_id).map_err(|e| e.to_string())
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
    use crate::ai_chat::action;
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
