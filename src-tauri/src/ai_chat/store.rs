//! Chat persistence in the active profile's encrypted database.
//!
//! Threads are linear in Phase 1. A user turn has one user message and one
//! or more assistant versions: a retry adds a version instead of a branch, and
//! the original answer keeps its own model attribution.
//!
//! Lesson excerpts sent to a model are stored once, by content hash, and
//! referenced from every snapshot that used them. Completed message content is
//! never rewritten.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// The chat tables, applied as migration 19. Kept in a SQL file beside the
/// baseline so test crates that include `db/schema.rs` by path can apply it.
pub const SCHEMA: &str = include_str!("../db/ai_chat.sql");

/// Most messages returned for one thread.
pub const MAX_THREAD_MESSAGES: i64 = 1000;
pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_DRAFT_BYTES: usize = 32_000;

#[derive(Debug)]
pub enum StoreError {
    NotFound,
    Conflict,
    Invalid(String),
    Sql(rusqlite::Error),
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound => f.write_str("not_found: this chat no longer exists"),
            StoreError::Conflict => f.write_str("conflict: this chat changed; reload it"),
            StoreError::Invalid(m) => write!(f, "invalid_input: {m}"),
            // Never surface SQL text; it can echo stored content.
            StoreError::Sql(_) => f.write_str("storage_error: chat data could not be accessed"),
        }
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatThread {
    pub id: String,
    pub title: String,
    pub mode: String,
    pub connection_id: Option<String>,
    pub model_id: Option<String>,
    pub revision: i64,
    pub reviewed_scope: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A source the request carried, numbered as the model was told to cite it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRef {
    /// `S1`, `S2`, ...
    pub id: String,
    pub kind: String,
    pub label: String,
    pub course_id: String,
    pub element_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub thread_id: String,
    pub position: i64,
    pub role: String,
    pub reply_to: Option<String>,
    pub version: i64,
    pub content: String,
    pub status: String,
    pub error: Option<String>,
    pub connection_id: Option<String>,
    pub connection_name: Option<String>,
    pub model_id: Option<String>,
    pub location: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub sources: Vec<SourceRef>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatBinding {
    pub id: String,
    pub thread_id: String,
    pub kind: String,
    pub course_id: String,
    pub element_id: String,
    pub label: String,
    pub created_at: String,
}

const THREAD_COLUMNS: &str =
    "id, title, mode, connection_id, model_id, revision, reviewed_scope, created_at, updated_at";

fn thread_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChatThread> {
    Ok(ChatThread {
        id: r.get(0)?,
        title: r.get(1)?,
        mode: r.get(2)?,
        connection_id: r.get(3)?,
        model_id: r.get(4)?,
        revision: r.get(5)?,
        reviewed_scope: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

const MESSAGE_COLUMNS: &str = "id, thread_id, position, role, reply_to, version, content, status, error, connection_id, connection_name, model_id, location, input_tokens, output_tokens, sources_json, created_at, completed_at";

fn message_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
    let sources: String = r.get(15)?;
    Ok(ChatMessage {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        position: r.get(2)?,
        role: r.get(3)?,
        reply_to: r.get(4)?,
        version: r.get(5)?,
        content: r.get(6)?,
        status: r.get(7)?,
        error: r.get(8)?,
        connection_id: r.get(9)?,
        connection_name: r.get(10)?,
        model_id: r.get(11)?,
        location: r.get(12)?,
        input_tokens: r.get(13)?,
        output_tokens: r.get(14)?,
        sources: serde_json::from_str(&sources).unwrap_or_default(),
        created_at: r.get(16)?,
        completed_at: r.get(17)?,
    })
}

fn bounded_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        Err(StoreError::Invalid(
            "identifier is empty or too long".into(),
        ))
    } else {
        Ok(())
    }
}

pub fn create_thread(
    conn: &Connection,
    id: &str,
    connection_id: Option<&str>,
    model_id: Option<&str>,
    now: &str,
) -> Result<ChatThread> {
    bounded_id(id)?;
    conn.execute(
        "INSERT INTO chat_threads (id, connection_id, model_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![id, connection_id, model_id, now],
    )?;
    thread(conn, id)
}

pub fn thread(conn: &Connection, id: &str) -> Result<ChatThread> {
    conn.query_row(
        &format!("SELECT {THREAD_COLUMNS} FROM chat_threads WHERE id = ?1"),
        [id],
        thread_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

pub fn list_threads(conn: &Connection, limit: i64) -> Result<Vec<ChatThread>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {THREAD_COLUMNS} FROM chat_threads ORDER BY updated_at DESC, id LIMIT ?1"
    ))?;
    let rows = stmt
        .query_map([limit.clamp(1, 500)], thread_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn rename_thread(conn: &Connection, id: &str, title: &str, now: &str) -> Result<ChatThread> {
    let title = title.trim();
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(StoreError::Invalid("title is too long".into()));
    }
    let changed = conn.execute(
        "UPDATE chat_threads SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, title, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    thread(conn, id)
}

pub fn set_thread_model(
    conn: &Connection,
    id: &str,
    connection_id: Option<&str>,
    model_id: Option<&str>,
    now: &str,
) -> Result<ChatThread> {
    let changed = conn.execute(
        "UPDATE chat_threads SET connection_id = ?2, model_id = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, connection_id, model_id, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    thread(conn, id)
}

pub fn set_thread_mode(conn: &Connection, id: &str, mode: &str, now: &str) -> Result<ChatThread> {
    if !super::context::is_mode(mode) {
        return Err(StoreError::Invalid("unknown chat mode".into()));
    }
    let changed = conn.execute(
        "UPDATE chat_threads SET mode = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, mode, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    thread(conn, id)
}

/// Deletes a thread with its messages, attachments, snapshots, runs and
/// draft, then drops excerpts no remaining snapshot references.
/// One thread found by search, with the best-matching message when the match
/// was in a message rather than the title.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    pub thread: ChatThread,
    pub message_id: Option<String>,
    /// Text around the match. Matched terms are wrapped in U+0002 and
    /// U+0003 so the UI can mark them without parsing markup.
    pub snippet: Option<String>,
}

/// Longest query accepted, in characters, and most terms used.
const MAX_QUERY_CHARS: usize = 200;
const MAX_QUERY_TERMS: usize = 8;

/// The query as an FTS5 expression: every term a quoted prefix phrase, so
/// nothing the learner types is read as query syntax.
fn match_expression(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .chars()
        .take(MAX_QUERY_CHARS)
        .collect::<String>()
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .take(MAX_QUERY_TERMS)
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// Searches the learner's threads: complete messages through the full-text
/// index, best match first, then threads whose title contains the query.
/// Each thread appears once.
pub fn search_threads(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let Some(expression) = match_expression(query) else {
        return Ok(Vec::new());
    };
    let limit = limit.clamp(1, 100);
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT m.thread_id, m.id,
                    snippet(chat_message_search, 1, char(2), char(3), '…', 12)
               FROM chat_message_search
               JOIN chat_messages m ON m.id = chat_message_search.message_id
              WHERE chat_message_search MATCH ?1
              ORDER BY bm25(chat_message_search)
              LIMIT 500",
        )?;
        let rows = stmt
            .query_map([&expression], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (thread_id, message_id, snippet) in rows {
            if hits.len() >= limit {
                break;
            }
            if seen.insert(thread_id.clone()) {
                hits.push(SearchHit {
                    thread: thread(conn, &thread_id)?,
                    message_id: Some(message_id),
                    snippet: Some(snippet),
                });
            }
        }
    }
    if hits.len() < limit {
        let needle = query
            .trim()
            .chars()
            .take(MAX_QUERY_CHARS)
            .collect::<String>();
        let pattern = format!(
            "%{}%",
            needle
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = conn.prepare(&format!(
            "SELECT {THREAD_COLUMNS} FROM chat_threads
              WHERE title LIKE ?1 ESCAPE '\\'
              ORDER BY updated_at DESC, id LIMIT 200"
        ))?;
        let titled = stmt
            .query_map([&pattern], thread_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for thread in titled {
            if hits.len() >= limit {
                break;
            }
            if seen.insert(thread.id.clone()) {
                hits.push(SearchHit {
                    thread,
                    message_id: None,
                    snippet: None,
                });
            }
        }
    }
    Ok(hits)
}

pub fn delete_thread(conn: &Connection, id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM learning_memory WHERE status = 'proposed' AND thread_id = ?1",
        [id],
    )?;
    // Proposals (and goal proposals already added) go with the chat; saved
    // artifacts stay.
    tx.execute(
        "DELETE FROM chat_artifacts WHERE status IN ('proposed', 'added') AND thread_id = ?1",
        [id],
    )?;
    let changed = tx.execute("DELETE FROM chat_threads WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    tx.execute("DELETE FROM chat_drafts WHERE scope = ?1", [id])?;

    collect_excerpts(&tx)?;
    tx.commit()?;
    Ok(())
}

fn collect_excerpts(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM chat_excerpts
          WHERE hash NOT IN (SELECT hash FROM chat_snapshot_excerpts)",
        [],
    )?;
    Ok(())
}

pub fn messages(conn: &Connection, thread_id: &str) -> Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM (
             SELECT * FROM chat_messages WHERE thread_id = ?1
              ORDER BY position DESC, role DESC, version DESC LIMIT ?2)
         ORDER BY position, role DESC, version"
    ))?;
    let rows = stmt
        .query_map(params![thread_id, MAX_THREAD_MESSAGES], message_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn message(conn: &Connection, id: &str) -> Result<ChatMessage> {
    conn.query_row(
        &format!("SELECT {MESSAGE_COLUMNS} FROM chat_messages WHERE id = ?1"),
        [id],
        message_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

/// The conversation a new request continues: each complete user turn with
/// its newest complete answer, oldest first. Turns whose answer failed or was
/// cut off are left out rather than sent half-finished.
pub fn history(conn: &Connection, thread_id: &str) -> Result<Vec<(ChatMessage, ChatMessage)>> {
    let all = messages(conn, thread_id)?;
    let mut turns = Vec::new();
    for user in all.iter().filter(|m| m.role == "user") {
        let answer = all
            .iter()
            .filter(|m| {
                m.role == "assistant"
                    && m.reply_to.as_deref() == Some(user.id.as_str())
                    && m.status == "complete"
            })
            .max_by_key(|m| m.version);
        if let Some(answer) = answer {
            turns.push((user.clone(), answer.clone()));
        }
    }
    Ok(turns)
}

pub fn bindings(conn: &Connection, thread_id: &str) -> Result<Vec<ChatBinding>> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, kind, course_id, element_id, label, created_at
           FROM chat_bindings WHERE thread_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map([thread_id], |r| {
            Ok(ChatBinding {
                id: r.get(0)?,
                thread_id: r.get(1)?,
                kind: r.get(2)?,
                course_id: r.get(3)?,
                element_id: r.get(4)?,
                label: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Attaches a lesson. Attaching the same lesson twice returns the existing
/// attachment. The caller has already authorized the lesson.
pub fn add_binding(
    conn: &Connection,
    id: &str,
    thread_id: &str,
    course_id: &str,
    element_id: &str,
    label: &str,
    now: &str,
) -> Result<ChatBinding> {
    attach_binding(
        conn, "lesson", id, thread_id, course_id, element_id, label, now,
    )
}

/// Attaches one of the learner's goals. The caller has checked it exists.
pub fn add_goal_binding(
    conn: &Connection,
    id: &str,
    thread_id: &str,
    goal_id: &str,
    label: &str,
    now: &str,
) -> Result<ChatBinding> {
    attach_binding(conn, "goal", id, thread_id, "", goal_id, label, now)
}

#[allow(clippy::too_many_arguments)]
fn attach_binding(
    conn: &Connection,
    kind: &str,
    id: &str,
    thread_id: &str,
    course_id: &str,
    element_id: &str,
    label: &str,
    now: &str,
) -> Result<ChatBinding> {
    let tx = conn.unchecked_transaction()?;
    thread(&tx, thread_id)?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM chat_bindings
              WHERE thread_id = ?1 AND kind = ?4 AND course_id = ?2 AND element_id = ?3",
            params![thread_id, course_id, element_id, kind],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_none() {
        if bindings(&tx, thread_id)?.len() >= 8 {
            return Err(StoreError::Invalid(
                "a chat can have at most 8 attachments".into(),
            ));
        }
        tx.execute(
            "INSERT INTO chat_bindings (id, thread_id, kind, course_id, element_id, label, created_at)
             VALUES (?1, ?2, ?7, ?3, ?4, ?5, ?6)",
            params![id, thread_id, course_id, element_id, label, now, kind],
        )?;
        bump(&tx, thread_id, now)?;
    }
    tx.commit()?;
    let id = existing.unwrap_or_else(|| id.to_string());
    bindings(conn, thread_id)?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or(StoreError::NotFound)
}

/// Stops sending an attachment with future messages. Text already quoted in
/// earlier messages, and what a provider already received, are unaffected.
pub fn remove_binding(
    conn: &Connection,
    thread_id: &str,
    binding_id: &str,
    now: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute(
        "DELETE FROM chat_bindings WHERE id = ?1 AND thread_id = ?2",
        params![binding_id, thread_id],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    bump(&tx, thread_id, now)?;
    tx.commit()?;
    Ok(())
}

fn bump(conn: &Connection, thread_id: &str, now: &str) -> Result<()> {
    conn.execute(
        "UPDATE chat_threads SET revision = revision + 1, updated_at = ?2 WHERE id = ?1",
        params![thread_id, now],
    )?;
    Ok(())
}

pub fn draft(conn: &Connection, scope: &str) -> Result<String> {
    Ok(conn
        .query_row(
            "SELECT text FROM chat_drafts WHERE scope = ?1",
            [scope],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default())
}

pub fn save_draft(conn: &Connection, scope: &str, text: &str, now: &str) -> Result<()> {
    bounded_id(scope)?;
    if text.len() > MAX_DRAFT_BYTES {
        return Err(StoreError::Invalid("draft is too long".into()));
    }
    if text.is_empty() {
        conn.execute("DELETE FROM chat_drafts WHERE scope = ?1", [scope])?;
    } else {
        conn.execute(
            "INSERT INTO chat_drafts (scope, text, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(scope) DO UPDATE SET text = excluded.text, updated_at = excluded.updated_at",
            params![scope, text, now],
        )?;
    }
    Ok(())
}

// ─── snapshots ─────────────────────────────────────────────────────────

/// Stores an excerpt once, by content hash.
pub fn put_excerpt(conn: &Connection, text: &str) -> Result<String> {
    let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
    conn.execute(
        "INSERT INTO chat_excerpts (hash, text) VALUES (?1, ?2) ON CONFLICT(hash) DO NOTHING",
        params![hash, text],
    )?;
    Ok(hash)
}

pub fn excerpt(conn: &Connection, hash: &str) -> Result<String> {
    conn.query_row(
        "SELECT text FROM chat_excerpts WHERE hash = ?1",
        [hash],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

pub struct StoredSnapshot {
    pub id: String,
    pub thread_id: String,
    pub digest: String,
    pub manifest_json: String,
    pub expires_at: String,
    pub consumed_at: Option<String>,
}

/// Saves a prepared request. Unused snapshots past their expiry are dropped
/// first so abandoned previews do not accumulate.
pub fn insert_snapshot(
    conn: &Connection,
    snapshot: &StoredSnapshot,
    excerpt_hashes: &[String],
    now: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM chat_snapshots WHERE consumed_at IS NULL AND expires_at < ?1",
        [now],
    )?;
    tx.execute(
        "INSERT INTO chat_snapshots (id, thread_id, digest, manifest_json, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            snapshot.id,
            snapshot.thread_id,
            snapshot.digest,
            snapshot.manifest_json,
            now,
            snapshot.expires_at
        ],
    )?;
    for hash in excerpt_hashes {
        tx.execute(
            "INSERT OR IGNORE INTO chat_snapshot_excerpts (snapshot_id, hash) VALUES (?1, ?2)",
            params![snapshot.id, hash],
        )?;
    }
    collect_excerpts(&tx)?;
    tx.commit()?;
    Ok(())
}

pub fn snapshot(conn: &Connection, id: &str) -> Result<StoredSnapshot> {
    conn.query_row(
        "SELECT id, thread_id, digest, manifest_json, expires_at, consumed_at
           FROM chat_snapshots WHERE id = ?1",
        [id],
        |r| {
            Ok(StoredSnapshot {
                id: r.get(0)?,
                thread_id: r.get(1)?,
                digest: r.get(2)?,
                manifest_json: r.get(3)?,
                expires_at: r.get(4)?,
                consumed_at: r.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

// ─── runs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRun {
    pub id: String,
    pub thread_id: String,
    pub client_request_id: String,
    pub user_message_id: String,
    pub assistant_message_id: String,
    pub state: String,
}

pub fn run_by_client_request(
    conn: &Connection,
    client_request_id: &str,
) -> Result<Option<ChatRun>> {
    Ok(conn
        .query_row(
            "SELECT id, thread_id, client_request_id, user_message_id, assistant_message_id, state
               FROM chat_runs WHERE client_request_id = ?1",
            [client_request_id],
            |r| {
                Ok(ChatRun {
                    id: r.get(0)?,
                    thread_id: r.get(1)?,
                    client_request_id: r.get(2)?,
                    user_message_id: r.get(3)?,
                    assistant_message_id: r.get(4)?,
                    state: r.get(5)?,
                })
            },
        )
        .optional()?)
}

pub fn active_run(conn: &Connection, thread_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM chat_runs WHERE thread_id = ?1 AND state = 'streaming'",
            [thread_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Who the answer is attributed to.
pub struct Attribution<'a> {
    pub connection_id: &'a str,
    pub connection_name: &'a str,
    pub model_id: &'a str,
    pub location: &'a str,
}

pub struct NewRun<'a> {
    pub run_id: &'a str,
    pub client_request_id: &'a str,
    pub snapshot_id: &'a str,
    pub thread_id: &'a str,
    /// The turn to answer again, or `None` for a new user message.
    pub regenerate_of: Option<&'a str>,
    pub user_message_id: &'a str,
    pub user_text: Option<&'a str>,
    pub assistant_message_id: &'a str,
    pub attribution: Attribution<'a>,
    pub sources: &'a [SourceRef],
    pub reviewed_scope: &'a str,
    pub expected_revision: i64,
    pub now: &'a str,
}

/// In one transaction: consume the snapshot, record the user message (unless
/// regenerating), add a streaming assistant placeholder and the run, and move
/// the thread forward. Fails with `Conflict` when the thread changed since the
/// request was prepared or another answer is already streaming.
pub fn begin_run(conn: &Connection, run: &NewRun<'_>) -> Result<ChatRun> {
    let tx = conn.unchecked_transaction()?;
    let current = thread(&tx, run.thread_id)?;
    if current.revision != run.expected_revision {
        return Err(StoreError::Conflict);
    }
    if active_run(&tx, run.thread_id)?.is_some() {
        return Err(StoreError::Conflict);
    }
    let consumed = tx.execute(
        "UPDATE chat_snapshots SET consumed_at = ?2
          WHERE id = ?1 AND thread_id = ?3 AND consumed_at IS NULL",
        params![run.snapshot_id, run.now, run.thread_id],
    )?;
    if consumed == 0 {
        return Err(StoreError::Conflict);
    }
    let (user_id, position, version) = match run.regenerate_of {
        Some(user_id) => {
            let user = message(&tx, user_id)?;
            if user.thread_id != run.thread_id || user.role != "user" {
                return Err(StoreError::Invalid("not a message in this chat".into()));
            }
            let version: i64 = tx.query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM chat_messages
                  WHERE reply_to = ?1 AND role = 'assistant'",
                [user_id],
                |r| r.get(0),
            )?;
            (user.id, user.position, version)
        }
        None => {
            let text = run
                .user_text
                .ok_or_else(|| StoreError::Invalid("a message is required".into()))?;
            let position: i64 = tx.query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM chat_messages WHERE thread_id = ?1",
                [run.thread_id],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO chat_messages (id, thread_id, position, role, content, status, created_at, completed_at)
                 VALUES (?1, ?2, ?3, 'user', ?4, 'complete', ?5, ?5)",
                params![run.user_message_id, run.thread_id, position, text, run.now],
            )?;
            if current.title.is_empty() {
                let title: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let title: String = title.chars().take(60).collect();
                tx.execute(
                    "UPDATE chat_threads SET title = ?2 WHERE id = ?1",
                    params![run.thread_id, title],
                )?;
            }
            (run.user_message_id.to_string(), position, 1)
        }
    };
    let sources =
        serde_json::to_string(run.sources).map_err(|_| StoreError::Invalid("sources".into()))?;
    tx.execute(
        "INSERT INTO chat_messages (id, thread_id, position, role, reply_to, version, status,
                                    connection_id, connection_name, model_id, location, sources_json, created_at)
         VALUES (?1, ?2, ?3, 'assistant', ?4, ?5, 'streaming', ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            run.assistant_message_id,
            run.thread_id,
            position,
            user_id,
            version,
            run.attribution.connection_id,
            run.attribution.connection_name,
            run.attribution.model_id,
            run.attribution.location,
            sources,
            run.now
        ],
    )?;
    tx.execute(
        "INSERT INTO chat_runs (id, thread_id, client_request_id, user_message_id, assistant_message_id,
                                snapshot_id, state, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'streaming', ?7)",
        params![
            run.run_id,
            run.thread_id,
            run.client_request_id,
            user_id,
            run.assistant_message_id,
            run.snapshot_id,
            run.now
        ],
    )?;
    tx.execute(
        "UPDATE chat_threads SET revision = revision + 1, reviewed_scope = ?2, updated_at = ?3
          WHERE id = ?1",
        params![run.thread_id, run.reviewed_scope, run.now],
    )?;
    tx.commit()?;
    Ok(ChatRun {
        id: run.run_id.to_string(),
        thread_id: run.thread_id.to_string(),
        client_request_id: run.client_request_id.to_string(),
        user_message_id: user_id,
        assistant_message_id: run.assistant_message_id.to_string(),
        state: "streaming".into(),
    })
}

/// Saves streamed text so far. Only a streaming message can be checkpointed.
pub fn checkpoint(conn: &Connection, assistant_message_id: &str, content: &str) -> Result<()> {
    conn.execute(
        "UPDATE chat_messages SET content = ?2 WHERE id = ?1 AND status = 'streaming'",
        params![assistant_message_id, content],
    )?;
    Ok(())
}

pub struct Finish<'a> {
    pub run_id: &'a str,
    pub assistant_message_id: &'a str,
    pub state: &'a str,
    pub content: &'a str,
    pub error: Option<&'a str>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub now: &'a str,
}

/// Records the outcome of a run atomically. A run that is no longer streaming
/// (already recovered as interrupted) is left as it is.
pub fn finish_run(conn: &Connection, finish: &Finish<'_>) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let finished = finish_run_in(&tx, finish)?;
    tx.commit()?;
    Ok(finished)
}

/// [`finish_run`] inside a transaction the caller owns, so related writes
/// (a practice question) commit with it.
pub fn finish_run_in(conn: &Connection, finish: &Finish<'_>) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE chat_runs SET state = ?2, finished_at = ?3 WHERE id = ?1 AND state = 'streaming'",
        params![finish.run_id, finish.state, finish.now],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    conn.execute(
        "UPDATE chat_messages
            SET content = ?2, status = ?3, error = ?4, input_tokens = ?5, output_tokens = ?6, completed_at = ?7
          WHERE id = ?1",
        params![
            finish.assistant_message_id,
            finish.content,
            finish.state,
            finish.error,
            finish.input_tokens.map(|v| v as i64),
            finish.output_tokens.map(|v| v as i64),
            finish.now
        ],
    )?;
    Ok(true)
}

/// Marks runs that claim to be streaming but have no live task (the app
/// exited, or the profile was locked mid-answer) as interrupted, keeping the
/// text saved so far. Returns how many were recovered.
pub fn recover_interrupted(
    conn: &Connection,
    is_live: &dyn Fn(&str) -> bool,
    now: &str,
) -> Result<usize> {
    let mut stmt =
        conn.prepare("SELECT id, assistant_message_id FROM chat_runs WHERE state = 'streaming'")?;
    let stale: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|(id, _)| !is_live(id))
        .collect();
    drop(stmt);
    let tx = conn.unchecked_transaction()?;
    for (run_id, message_id) in &stale {
        tx.execute(
            "UPDATE chat_runs SET state = 'interrupted', finished_at = ?2 WHERE id = ?1",
            params![run_id, now],
        )?;
        tx.execute(
            "UPDATE chat_messages SET status = 'interrupted', error = 'interrupted', completed_at = ?2
              WHERE id = ?1 AND status = 'streaming'",
            params![message_id, now],
        )?;
    }
    tx.commit()?;
    Ok(stale.len())
}

// ─── practice ──────────────────────────────────────────────────────────

use super::practice::{self, Question};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PracticeAttempt {
    pub answer: String,
    pub correct: bool,
    pub assisted: bool,
    pub created_at: String,
}

/// What the learner sees of a practice question. The answer and the
/// explanation appear only once it is solved or the answer was shown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PracticeView {
    pub id: String,
    pub message_id: String,
    pub skill: String,
    pub kind: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub hints: Vec<String>,
    pub hints_total: usize,
    pub revealed: bool,
    pub solved: bool,
    pub solved_independently: bool,
    pub answer: Option<String>,
    pub explanation: Option<String>,
    pub attempts: Vec<PracticeAttempt>,
}

struct PracticeRow {
    id: String,
    message_id: String,
    question: Question,
    hints_used: usize,
    revealed: bool,
    solved: bool,
    solved_independently: bool,
}

fn practice_row(conn: &Connection, id: &str) -> Result<PracticeRow> {
    let (message_id, json, hints_used, revealed, solved, independent): (
        String,
        String,
        i64,
        bool,
        bool,
        bool,
    ) = conn
        .query_row(
            "SELECT message_id, question_json, hints_used, revealed, solved, solved_independently
               FROM chat_practice_items WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    let question: Question =
        serde_json::from_str(&json).map_err(|_| StoreError::Invalid("practice question".into()))?;
    Ok(PracticeRow {
        id: id.to_string(),
        message_id,
        question,
        hints_used: hints_used.max(0) as usize,
        revealed,
        solved,
        solved_independently: independent,
    })
}

fn view(conn: &Connection, row: &PracticeRow) -> Result<PracticeView> {
    let mut stmt = conn.prepare(
        "SELECT answer, correct, assisted, created_at FROM chat_practice_attempts
          WHERE item_id = ?1 ORDER BY created_at, rowid",
    )?;
    let attempts = stmt
        .query_map([&row.id], |r| {
            Ok(PracticeAttempt {
                answer: r.get(0)?,
                correct: r.get(1)?,
                assisted: r.get(2)?,
                created_at: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let open = row.solved || row.revealed;
    let q = &row.question;
    Ok(PracticeView {
        id: row.id.clone(),
        message_id: row.message_id.clone(),
        skill: q.skill.clone(),
        kind: q.kind.as_str().to_string(),
        prompt: q.prompt.clone(),
        options: q.options.clone(),
        hints: q.hints.iter().take(row.hints_used).cloned().collect(),
        hints_total: q.hints.len(),
        revealed: row.revealed,
        solved: row.solved,
        solved_independently: row.solved_independently,
        answer: open.then(|| practice::display(q)),
        explanation: (open && !q.explanation.is_empty()).then(|| q.explanation.clone()),
        attempts,
    })
}

pub fn insert_practice(
    conn: &Connection,
    id: &str,
    thread_id: &str,
    message_id: &str,
    question: &Question,
    now: &str,
) -> Result<()> {
    let json = serde_json::to_string(question)
        .map_err(|_| StoreError::Invalid("practice question".into()))?;
    conn.execute(
        "INSERT INTO chat_practice_items (id, thread_id, message_id, question_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, thread_id, message_id, json, now],
    )?;
    Ok(())
}

pub fn practice_for_thread(conn: &Connection, thread_id: &str) -> Result<Vec<PracticeView>> {
    let ids: Vec<String> = conn
        .prepare(
            "SELECT id FROM chat_practice_items WHERE thread_id = ?1 ORDER BY created_at, rowid",
        )?
        .query_map([thread_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter()
        .map(|id| practice_row(conn, id).and_then(|row| view(conn, &row)))
        .collect()
}

pub fn practice_view(conn: &Connection, id: &str) -> Result<PracticeView> {
    view(conn, &practice_row(conn, id)?)
}

/// Checks an answer and records the attempt. Answers after a solve are not
/// recorded; an unreadable answer is refused without counting.
pub fn practice_attempt(
    conn: &Connection,
    id: &str,
    attempt_id: &str,
    answer: &str,
    now: &str,
) -> Result<PracticeView> {
    let tx = conn.unchecked_transaction()?;
    let row = practice_row(&tx, id)?;
    if !row.solved {
        let correct = practice::check(&row.question.answer, answer).ok_or_else(|| {
            StoreError::Invalid("that answer could not be read; check the format".into())
        })?;
        let assisted = row.hints_used > 0 || row.revealed;
        tx.execute(
            "INSERT INTO chat_practice_attempts (id, item_id, answer, correct, assisted, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![attempt_id, id, answer.trim(), correct, assisted, now],
        )?;
        if correct {
            tx.execute(
                "UPDATE chat_practice_items SET solved = 1, solved_independently = ?2 WHERE id = ?1",
                params![id, !assisted],
            )?;
        }
    }
    tx.commit()?;
    practice_view(conn, id)
}

/// Shows the next hint. Every later attempt on this question is assisted.
pub fn practice_hint(conn: &Connection, id: &str) -> Result<PracticeView> {
    let row = practice_row(conn, id)?;
    if row.hints_used < row.question.hints.len() && !row.solved {
        conn.execute(
            "UPDATE chat_practice_items SET hints_used = hints_used + 1 WHERE id = ?1",
            [id],
        )?;
    }
    practice_view(conn, id)
}

/// Shows the answer. The question can no longer be solved independently.
pub fn practice_reveal(conn: &Connection, id: &str) -> Result<PracticeView> {
    practice_row(conn, id)?;
    conn.execute(
        "UPDATE chat_practice_items SET revealed = 1 WHERE id = ?1",
        [id],
    )?;
    practice_view(conn, id)
}

// ─── learning memory ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryItem {
    pub id: String,
    pub kind: String,
    pub content: String,
    pub status: String,
    pub active: bool,
    pub thread_id: Option<String>,
    pub source_message_id: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

const MEMORY_COLUMNS: &str =
    "id, kind, content, status, active, thread_id, source_message_id, revision, created_at, updated_at";

fn memory_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryItem> {
    Ok(MemoryItem {
        id: r.get(0)?,
        kind: r.get(1)?,
        content: r.get(2)?,
        status: r.get(3)?,
        active: r.get(4)?,
        thread_id: r.get(5)?,
        source_message_id: r.get(6)?,
        revision: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

fn memory_query(conn: &Connection, filter: &str, arg: &str) -> Result<Vec<MemoryItem>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MEMORY_COLUMNS} FROM learning_memory WHERE {filter} ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map([arg], memory_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Approved items and open proposals.
pub fn list_memory(conn: &Connection) -> Result<Vec<MemoryItem>> {
    memory_query(conn, "?1 = ?1", "")
}

/// What may be sent: approved and switched on.
pub fn sendable_memory(conn: &Connection) -> Result<Vec<MemoryItem>> {
    memory_query(conn, "status = 'approved' AND active = 1 AND ?1 = ?1", "")
}

pub fn memory_proposals_for_thread(conn: &Connection, thread_id: &str) -> Result<Vec<MemoryItem>> {
    memory_query(conn, "status = 'proposed' AND thread_id = ?1", thread_id)
}

pub fn memory_item(conn: &Connection, id: &str) -> Result<MemoryItem> {
    conn.query_row(
        &format!("SELECT {MEMORY_COLUMNS} FROM learning_memory WHERE id = ?1"),
        [id],
        memory_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

fn approved_count(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM learning_memory WHERE status = 'approved'",
        [],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

fn duplicate(conn: &Connection, content: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM learning_memory WHERE lower(content) = lower(?1))",
        [content],
        |r| r.get(0),
    )?)
}

/// Stores a model's proposal for the learner to decide on. Repeats of an
/// existing item or proposal are ignored.
#[allow(clippy::too_many_arguments)]
pub fn propose_memory(
    conn: &Connection,
    id: &str,
    kind: super::memory::Kind,
    content: &str,
    thread_id: &str,
    message_id: &str,
    now: &str,
) -> Result<bool> {
    if duplicate(conn, content)? {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO learning_memory (id, kind, content, status, thread_id, source_message_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'proposed', ?4, ?5, ?6, ?6)",
        params![id, kind.as_str(), content, thread_id, message_id, now],
    )?;
    Ok(true)
}

/// The learner adds an item directly; it is approved.
pub fn add_memory(
    conn: &Connection,
    id: &str,
    kind: super::memory::Kind,
    content: &str,
    now: &str,
) -> Result<MemoryItem> {
    let content = super::memory::clean(content)
        .ok_or_else(|| StoreError::Invalid("memory must be one short sentence".into()))?;
    if approved_count(conn)? >= super::memory::MAX_ITEMS {
        return Err(StoreError::Invalid(
            "learning memory is full; forget something first".into(),
        ));
    }
    conn.execute(
        "INSERT INTO learning_memory (id, kind, content, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'approved', ?4, ?4)",
        params![id, kind.as_str(), content, now],
    )?;
    memory_item(conn, id)
}

/// The learner saves a proposal, optionally editing it first.
pub fn approve_memory(
    conn: &Connection,
    id: &str,
    content: Option<&str>,
    now: &str,
) -> Result<MemoryItem> {
    let item = memory_item(conn, id)?;
    let content = match content {
        Some(c) => super::memory::clean(c)
            .ok_or_else(|| StoreError::Invalid("memory must be one short sentence".into()))?,
        None => item.content.clone(),
    };
    if item.status != "approved" && approved_count(conn)? >= super::memory::MAX_ITEMS {
        return Err(StoreError::Invalid(
            "learning memory is full; forget something first".into(),
        ));
    }
    conn.execute(
        "UPDATE learning_memory SET status = 'approved', content = ?2, revision = revision + 1, updated_at = ?3
          WHERE id = ?1",
        params![id, content, now],
    )?;
    memory_item(conn, id)
}

/// Edits an approved item or switches it on or off for future messages.
pub fn update_memory(
    conn: &Connection,
    id: &str,
    content: Option<&str>,
    active: Option<bool>,
    now: &str,
) -> Result<MemoryItem> {
    let item = memory_item(conn, id)?;
    if item.status != "approved" {
        return Err(StoreError::Invalid("save the suggestion first".into()));
    }
    let content = match content {
        Some(c) => super::memory::clean(c)
            .ok_or_else(|| StoreError::Invalid("memory must be one short sentence".into()))?,
        None => item.content,
    };
    conn.execute(
        "UPDATE learning_memory SET content = ?2, active = ?3, revision = revision + 1, updated_at = ?4
          WHERE id = ?1",
        params![id, content, active.unwrap_or(item.active), now],
    )?;
    memory_item(conn, id)
}

/// Removes an item or dismisses a proposal. Earlier messages and what a
/// provider already received are not affected.
pub fn forget_memory(conn: &Connection, id: &str) -> Result<()> {
    if conn.execute("DELETE FROM learning_memory WHERE id = ?1", [id])? == 0 {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

// ─── study artifacts ──────────────────────────────────────────────────────

/// Most saved artifacts kept at once.
pub const MAX_SAVED_ARTIFACTS: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Artifact {
    pub id: String,
    pub status: String,
    pub title: String,
    pub body: super::artifact::Body,
    pub sources: Vec<SourceRef>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const ARTIFACT_COLUMNS: &str =
    "id, status, title, body_json, sources_json, thread_id, message_id, created_at, updated_at";

fn artifact_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Artifact> {
    let body: String = r.get(3)?;
    let sources: String = r.get(4)?;
    let body = serde_json::from_str(&body).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Artifact {
        id: r.get(0)?,
        status: r.get(1)?,
        title: r.get(2)?,
        body,
        sources: serde_json::from_str(&sources).unwrap_or_default(),
        thread_id: r.get(5)?,
        message_id: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

pub fn artifact(conn: &Connection, id: &str) -> Result<Artifact> {
    conn.query_row(
        &format!("SELECT {ARTIFACT_COLUMNS} FROM chat_artifacts WHERE id = ?1"),
        [id],
        artifact_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

fn insert_artifact(
    conn: &Connection,
    id: &str,
    status: &str,
    proposal: &super::artifact::Proposal,
    message_id: &str,
    now: &str,
) -> Result<()> {
    let origin = message(conn, message_id)?;
    let body =
        serde_json::to_string(&proposal.body).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let sources =
        serde_json::to_string(&origin.sources).map_err(|e| StoreError::Invalid(e.to_string()))?;
    conn.execute(
        "INSERT INTO chat_artifacts (id, kind, status, title, body_json, sources_json, thread_id, message_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            id,
            proposal.body.kind(),
            status,
            proposal.title,
            body,
            sources,
            origin.thread_id,
            message_id,
            now
        ],
    )?;
    Ok(())
}

/// Stores a model's proposal for the learner to save or dismiss.
pub fn propose_artifact(
    conn: &Connection,
    id: &str,
    proposal: &super::artifact::Proposal,
    message_id: &str,
    now: &str,
) -> Result<()> {
    insert_artifact(conn, id, "proposed", proposal, message_id, now)
}

fn saved_count(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM chat_artifacts WHERE status = 'saved'",
        [],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

fn ensure_room(conn: &Connection) -> Result<()> {
    if saved_count(conn)? >= MAX_SAVED_ARTIFACTS {
        return Err(StoreError::Invalid(
            "saved notes are full; delete some first".into(),
        ));
    }
    Ok(())
}

/// The learner saves a complete answer as a note.
pub fn save_message_as_note(
    conn: &Connection,
    id: &str,
    message_id: &str,
    title: &str,
    now: &str,
) -> Result<Artifact> {
    let origin = message(conn, message_id)?;
    if origin.role != "assistant" || origin.status != "complete" {
        return Err(StoreError::Invalid(
            "only a finished answer can be saved".into(),
        ));
    }
    let proposal = super::artifact::Proposal {
        title: super::artifact::clean_title(title)
            .ok_or_else(|| StoreError::Invalid("a note needs a short title".into()))?,
        body: super::artifact::Body::Note {
            text: super::artifact::clean_note(&origin.content).ok_or_else(|| {
                StoreError::Invalid("this answer is too long to save as a note".into())
            })?,
        },
    };
    ensure_room(conn)?;
    insert_artifact(conn, id, "saved", &proposal, message_id, now)?;
    artifact(conn, id)
}

/// Saves a proposal. Saving twice is harmless. A goal proposal is not saved
/// here; it is added through the Goals service and then marked.
pub fn save_artifact(conn: &Connection, id: &str, now: &str) -> Result<Artifact> {
    let current = artifact(conn, id)?;
    if matches!(
        current.body,
        super::artifact::Body::Goal { .. } | super::artifact::Body::GoalChange { .. }
    ) {
        return Err(StoreError::Invalid(
            "a goal is added from its preview".into(),
        ));
    }
    if current.status == "proposed" {
        ensure_room(conn)?;
        conn.execute(
            "UPDATE chat_artifacts SET status = 'saved', updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
    }
    artifact(conn, id)
}

/// Records that the learner added a proposed goal, as `goal_id`. The caller
/// checks the goal exists. Marking twice is harmless.
pub fn mark_goal_added(conn: &Connection, id: &str, goal_id: &str, now: &str) -> Result<Artifact> {
    let current = artifact(conn, id)?;
    let super::artifact::Body::Goal { text, .. } = current.body else {
        return Err(StoreError::Invalid("not a goal proposal".into()));
    };
    if current.status == "proposed" {
        let body = super::artifact::Body::Goal {
            text,
            goal_id: Some(goal_id.to_string()),
        };
        let body = serde_json::to_string(&body).map_err(|e| StoreError::Invalid(e.to_string()))?;
        conn.execute(
            "UPDATE chat_artifacts SET status = 'added', body_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, body, now],
        )?;
    }
    artifact(conn, id)
}

/// Records that the learner applied a proposed goal change. The caller
/// checks the goal reflects it. Marking twice is harmless.
pub fn mark_goal_changed(conn: &Connection, id: &str, now: &str) -> Result<Artifact> {
    let current = artifact(conn, id)?;
    if !matches!(current.body, super::artifact::Body::GoalChange { .. }) {
        return Err(StoreError::Invalid("not a goal change".into()));
    }
    if current.status == "proposed" {
        conn.execute(
            "UPDATE chat_artifacts SET status = 'added', updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
    }
    artifact(conn, id)
}

/// Dismisses a proposal or deletes a saved artifact.
pub fn delete_artifact(conn: &Connection, id: &str) -> Result<()> {
    if conn.execute("DELETE FROM chat_artifacts WHERE id = ?1", [id])? == 0 {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

/// Proposals and saved artifacts that came from one thread, oldest first.
pub fn artifacts_for_thread(conn: &Connection, thread_id: &str) -> Result<Vec<Artifact>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ARTIFACT_COLUMNS} FROM chat_artifacts WHERE thread_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map([thread_id], artifact_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every saved artifact, newest first.
pub fn saved_artifacts(conn: &Connection) -> Result<Vec<Artifact>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ARTIFACT_COLUMNS} FROM chat_artifacts WHERE status = 'saved'
          ORDER BY updated_at DESC, id LIMIT {MAX_SAVED_ARTIFACTS}"
    ))?;
    let rows = stmt
        .query_map([], artifact_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-10-23T12:00:00Z";

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for (_, _, sql) in crate::db::schema::MIGRATIONS {
            conn.execute_batch(sql).unwrap();
        }
        conn
    }

    fn snapshot_row(conn: &Connection, id: &str, thread: &str, excerpts: &[String]) {
        insert_snapshot(
            conn,
            &StoredSnapshot {
                id: id.into(),
                thread_id: thread.into(),
                digest: "d".into(),
                manifest_json: "{}".into(),
                expires_at: "2026-10-23T12:15:00Z".into(),
                consumed_at: None,
            },
            excerpts,
            NOW,
        )
        .unwrap();
    }

    fn attribution() -> Attribution<'static> {
        Attribution {
            connection_id: "c1",
            connection_name: "Ollama",
            model_id: "llama3.1:8b",
            location: "device",
        }
    }

    fn start<'a>(
        run_id: &'a str,
        request: &'a str,
        snap: &'a str,
        regenerate_of: Option<&'a str>,
        user_id: &'a str,
        assistant_id: &'a str,
        revision: i64,
    ) -> NewRun<'a> {
        NewRun {
            run_id,
            client_request_id: request,
            snapshot_id: snap,
            thread_id: "t",
            regenerate_of,
            user_message_id: user_id,
            user_text: Some("Why is a positive test usually a false alarm?"),
            assistant_message_id: assistant_id,
            attribution: attribution(),
            sources: &[],
            reviewed_scope: "scope",
            expected_revision: revision,
            now: NOW,
        }
    }

    #[test]
    fn search_finds_complete_messages_and_titles_and_ignores_query_syntax() {
        let conn = db();
        create_thread(&conn, "t", Some("c1"), None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        // The question is complete and indexed; the streaming answer is not.
        let hits = search_threads(&conn, "positive", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id.as_deref(), Some("u1"));
        assert!(hits[0]
            .snippet
            .as_deref()
            .unwrap()
            .contains("\u{2}positive\u{3}"));
        checkpoint(&conn, "a1", "Base rates matter").unwrap();
        assert!(search_threads(&conn, "base rates", 10).unwrap().is_empty());
        finish_run(
            &conn,
            &Finish {
                run_id: "r1",
                assistant_message_id: "a1",
                state: "complete",
                content: "Base rates matter more than the test's accuracy.",
                error: None,
                input_tokens: None,
                output_tokens: None,
                now: NOW,
            },
        )
        .unwrap();
        // Prefixes and diacritics match.
        let hits = search_threads(&conn, "BASE accur", 10).unwrap();
        assert_eq!(hits[0].message_id.as_deref(), Some("a1"));
        assert_eq!(search_threads(&conn, "accúracy", 10).unwrap().len(), 1);
        // Query syntax is literal text, never an error.
        for hostile in [
            "\"",
            "NEAR(",
            "a OR",
            "content:x",
            "*",
            "-x",
            "'; DROP TABLE x; --",
        ] {
            assert!(search_threads(&conn, hostile, 10).is_ok(), "{hostile}");
        }
        assert!(search_threads(&conn, "   ", 10).unwrap().is_empty());

        create_thread(&conn, "t2", None, None, NOW).unwrap();
        rename_thread(&conn, "t2", "Bayes 100% notes", NOW).unwrap();
        let hits = search_threads(&conn, "100%", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].thread.id, "t2");
        assert_eq!(hits[0].snippet, None);
        assert!(
            search_threads(&conn, "_", 10).unwrap().is_empty(),
            "LIKE wildcards are literal"
        );

        delete_thread(&conn, "t").unwrap();
        assert!(search_threads(&conn, "base", 10).unwrap().is_empty());
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM chat_message_search", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "deleting a thread empties its index rows");
    }

    #[test]
    fn proposals_go_with_their_chat_and_only_finished_answers_become_notes() {
        let conn = db();
        create_thread(&conn, "t", Some("c1"), None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        let proposal = super::super::artifact::Proposal {
            title: "Terms".into(),
            body: super::super::artifact::Body::Note {
                text: "Prior: belief before evidence".into(),
            },
        };
        propose_artifact(&conn, "p1", &proposal, "a1", NOW).unwrap();
        // The answer is still streaming, and a question is not an answer.
        assert!(save_message_as_note(&conn, "n1", "a1", "T", NOW).is_err());
        assert!(save_message_as_note(&conn, "n1", "u1", "T", NOW).is_err());
        assert!(matches!(
            artifact(&conn, "missing"),
            Err(StoreError::NotFound)
        ));
        assert_eq!(save_artifact(&conn, "p1", NOW).unwrap().status, "saved");
        assert_eq!(
            save_artifact(&conn, "p1", NOW).unwrap().status,
            "saved",
            "saving twice is harmless"
        );
        propose_artifact(&conn, "p2", &proposal, "a1", NOW).unwrap();
        delete_thread(&conn, "t").unwrap();
        assert!(matches!(artifact(&conn, "p2"), Err(StoreError::NotFound)));
        assert_eq!(saved_artifacts(&conn).unwrap().len(), 1);
        delete_artifact(&conn, "p1").unwrap();
        assert!(delete_artifact(&conn, "p1").is_err());
    }

    #[test]
    fn a_goal_proposal_is_added_not_saved_and_goes_with_its_chat() {
        let conn = db();
        create_thread(&conn, "t", Some("c1"), None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        let goal = super::super::artifact::Proposal {
            title: "Data analyst".into(),
            body: super::super::artifact::Body::Goal {
                text: "Clean data and build dashboards".into(),
                goal_id: None,
            },
        };
        propose_artifact(&conn, "g1", &goal, "a1", NOW).unwrap();
        assert!(save_artifact(&conn, "g1", NOW).is_err());
        let added = mark_goal_added(&conn, "g1", "goal-7", NOW).unwrap();
        assert_eq!(added.status, "added");
        assert_eq!(
            added.body,
            super::super::artifact::Body::Goal {
                text: "Clean data and build dashboards".into(),
                goal_id: Some("goal-7".into()),
            }
        );
        let again = mark_goal_added(&conn, "g1", "goal-8", NOW).unwrap();
        assert_eq!(again.body, added.body, "marking twice keeps the first goal");
        assert!(saved_artifacts(&conn).unwrap().is_empty());

        let note = super::super::artifact::Proposal {
            title: "T".into(),
            body: super::super::artifact::Body::Note { text: "x".into() },
        };
        propose_artifact(&conn, "n1", &note, "a1", NOW).unwrap();
        assert!(mark_goal_added(&conn, "n1", "goal-7", NOW).is_err());

        delete_thread(&conn, "t").unwrap();
        assert!(matches!(artifact(&conn, "g1"), Err(StoreError::NotFound)));
    }

    #[test]
    fn a_run_records_the_turn_and_moves_the_thread_forward() {
        let conn = db();
        create_thread(&conn, "t", Some("c1"), None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        let run = begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        assert_eq!(run.state, "streaming");
        let thread = thread(&conn, "t").unwrap();
        assert_eq!(thread.revision, 2);
        assert_eq!(thread.reviewed_scope.as_deref(), Some("scope"));
        assert_eq!(
            thread.title,
            "Why is a positive test usually a false alarm?"
        );
        assert_eq!(active_run(&conn, "t").unwrap().as_deref(), Some("r1"));

        checkpoint(&conn, "a1", "Because the").unwrap();
        assert_eq!(message(&conn, "a1").unwrap().content, "Because the");
        assert!(finish_run(
            &conn,
            &Finish {
                run_id: "r1",
                assistant_message_id: "a1",
                state: "complete",
                content: "Because the condition is rare.",
                error: None,
                input_tokens: Some(40),
                output_tokens: None,
                now: NOW,
            }
        )
        .unwrap());
        let answer = message(&conn, "a1").unwrap();
        assert_eq!(answer.status, "complete");
        assert_eq!(answer.model_id.as_deref(), Some("llama3.1:8b"));
        assert_eq!(answer.input_tokens, Some(40));
        assert_eq!(answer.output_tokens, None);
        // A completed message is not rewritten by a late checkpoint.
        checkpoint(&conn, "a1", "late").unwrap();
        assert_eq!(
            message(&conn, "a1").unwrap().content,
            "Because the condition is rare."
        );
        assert_eq!(history(&conn, "t").unwrap().len(), 1);
    }

    #[test]
    fn a_stale_revision_a_reused_snapshot_or_a_second_stream_is_refused() {
        let conn = db();
        create_thread(&conn, "t", None, None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        snapshot_row(&conn, "s2", "t", &[]);
        assert!(matches!(
            begin_run(&conn, &start("r0", "q0", "s1", None, "u0", "a0", 7)),
            Err(StoreError::Conflict)
        ));
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        // The same snapshot cannot send twice.
        assert!(begin_run(&conn, &start("r2", "q2", "s1", None, "u2", "a2", 2)).is_err());
        // Another answer is still streaming.
        assert!(matches!(
            begin_run(&conn, &start("r3", "q3", "s2", None, "u3", "a3", 2)),
            Err(StoreError::Conflict)
        ));
        assert!(run_by_client_request(&conn, "q1").unwrap().is_some());
        assert!(run_by_client_request(&conn, "q3").unwrap().is_none());
    }

    #[test]
    fn a_retry_adds_a_version_and_keeps_the_original_answer() {
        let conn = db();
        create_thread(&conn, "t", None, None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        finish_run(
            &conn,
            &Finish {
                run_id: "r1",
                assistant_message_id: "a1",
                state: "complete",
                content: "First answer",
                error: None,
                input_tokens: None,
                output_tokens: None,
                now: NOW,
            },
        )
        .unwrap();
        snapshot_row(&conn, "s2", "t", &[]);
        let retry = begin_run(
            &conn,
            &start("r2", "q2", "s2", Some("u1"), "ignored", "a2", 2),
        )
        .unwrap();
        assert_eq!(retry.user_message_id, "u1");
        let answer = message(&conn, "a2").unwrap();
        assert_eq!((answer.position, answer.version), (0, 2));
        assert_eq!(message(&conn, "a1").unwrap().content, "First answer");
        assert_eq!(messages(&conn, "t").unwrap().len(), 3);
        // Until the retry completes, history keeps the complete first answer.
        assert_eq!(history(&conn, "t").unwrap()[0].1.id, "a1");
    }

    #[test]
    fn runs_without_a_live_task_recover_as_interrupted_with_their_text() {
        let conn = db();
        create_thread(&conn, "t", None, None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        checkpoint(&conn, "a1", "Half an answ").unwrap();
        assert_eq!(
            recover_interrupted(&conn, &|id| id == "r1", NOW).unwrap(),
            0
        );
        assert_eq!(recover_interrupted(&conn, &|_| false, NOW).unwrap(), 1);
        let answer = message(&conn, "a1").unwrap();
        assert_eq!(
            (answer.status.as_str(), answer.content.as_str()),
            ("interrupted", "Half an answ")
        );
        assert!(active_run(&conn, "t").unwrap().is_none());
        // A late finish from a task that was presumed dead does not overwrite.
        assert!(!finish_run(
            &conn,
            &Finish {
                run_id: "r1",
                assistant_message_id: "a1",
                state: "complete",
                content: "Overwritten",
                error: None,
                input_tokens: None,
                output_tokens: None,
                now: NOW
            }
        )
        .unwrap());
        assert!(history(&conn, "t").unwrap().is_empty());
    }

    #[test]
    fn excerpts_are_stored_once_and_collected_with_their_last_thread() {
        let conn = db();
        create_thread(&conn, "t1", None, None, NOW).unwrap();
        create_thread(&conn, "t2", None, None, NOW).unwrap();
        let hash = put_excerpt(&conn, "A long lesson").unwrap();
        assert_eq!(put_excerpt(&conn, "A long lesson").unwrap(), hash);
        snapshot_row(&conn, "s1", "t1", std::slice::from_ref(&hash));
        snapshot_row(&conn, "s2", "t2", std::slice::from_ref(&hash));
        let count = |conn: &Connection| -> i64 {
            conn.query_row("SELECT COUNT(*) FROM chat_excerpts", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(&conn), 1);
        delete_thread(&conn, "t1").unwrap();
        assert_eq!(excerpt(&conn, &hash).unwrap(), "A long lesson");
        delete_thread(&conn, "t2").unwrap();
        assert_eq!(count(&conn), 0);
        assert!(matches!(
            delete_thread(&conn, "t2"),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn attachments_are_idempotent_bounded_and_bump_the_revision() {
        let conn = db();
        create_thread(&conn, "t", None, None, NOW).unwrap();
        let first = add_binding(&conn, "b1", "t", "course", "lesson", "Lesson", NOW).unwrap();
        let again = add_binding(&conn, "b2", "t", "course", "lesson", "Lesson", NOW).unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(thread(&conn, "t").unwrap().revision, 2);
        for i in 0..7 {
            add_binding(
                &conn,
                &format!("x{i}"),
                "t",
                "course",
                &format!("e{i}"),
                "L",
                NOW,
            )
            .unwrap();
        }
        assert!(add_binding(&conn, "x9", "t", "course", "e9", "L", NOW).is_err());
        remove_binding(&conn, "t", "b1", NOW).unwrap();
        assert_eq!(bindings(&conn, "t").unwrap().len(), 7);
        assert!(remove_binding(&conn, "t", "b1", NOW).is_err());
    }

    #[test]
    fn drafts_save_clear_and_are_bounded() {
        let conn = db();
        assert_eq!(draft(&conn, "new").unwrap(), "");
        save_draft(&conn, "new", "Half a thought", NOW).unwrap();
        assert_eq!(draft(&conn, "new").unwrap(), "Half a thought");
        save_draft(&conn, "new", "", NOW).unwrap();
        assert_eq!(draft(&conn, "new").unwrap(), "");
        assert!(save_draft(&conn, "new", &"x".repeat(MAX_DRAFT_BYTES + 1), NOW).is_err());
    }

    #[test]
    fn storage_errors_never_echo_sql() {
        let error = StoreError::Sql(rusqlite::Error::InvalidQuery);
        assert_eq!(
            error.to_string(),
            "storage_error: chat data could not be accessed"
        );
    }

    fn practice_item(conn: &Connection) -> String {
        create_thread(conn, "t", None, None, NOW).unwrap();
        snapshot_row(conn, "s1", "t", &[]);
        begin_run(conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        let q = super::practice::parse(
            r#"{"skill":"Conditional probability","kind":"numeric","prompt":"P?","answer":"2/3","hints":["one","two"],"explanation":"Because."}"#,
        )
        .unwrap();
        insert_practice(conn, "p1", "t", "a1", &q, NOW).unwrap();
        "p1".into()
    }

    #[test]
    fn the_answer_stays_hidden_until_solved_and_an_independent_solve_is_recorded() {
        let conn = db();
        let id = practice_item(&conn);
        let v = practice_view(&conn, &id).unwrap();
        assert_eq!(
            (v.answer.as_ref(), v.explanation.as_ref(), v.hints_total),
            (None, None, 2)
        );
        let v = practice_attempt(&conn, &id, "x1", "1/2", NOW).unwrap();
        assert!(!v.solved && !v.attempts[0].correct && !v.attempts[0].assisted);
        assert!(practice_attempt(&conn, &id, "x2", "two thirds", NOW).is_err());
        let v = practice_attempt(&conn, &id, "x3", "0.667", NOW).unwrap();
        assert!(v.solved && v.solved_independently);
        assert_eq!(v.answer.as_deref(), Some("2/3"));
        assert_eq!(v.explanation.as_deref(), Some("Because."));
        // Further answers after a solve are not recorded.
        let v = practice_attempt(&conn, &id, "x4", "5", NOW).unwrap();
        assert_eq!(v.attempts.len(), 2);
    }

    #[test]
    fn hints_and_a_shown_answer_make_later_attempts_assisted() {
        let conn = db();
        let id = practice_item(&conn);
        let v = practice_hint(&conn, &id).unwrap();
        assert_eq!(v.hints, vec!["one"]);
        practice_hint(&conn, &id).unwrap();
        let v = practice_hint(&conn, &id).unwrap();
        assert_eq!(v.hints.len(), 2, "no more hints than written");
        let v = practice_attempt(&conn, &id, "x1", "2/3", NOW).unwrap();
        assert!(v.solved && !v.solved_independently && v.attempts[0].assisted);

        let conn = db();
        let id = practice_item(&conn);
        let v = practice_reveal(&conn, &id).unwrap();
        assert_eq!(v.answer.as_deref(), Some("2/3"));
        let v = practice_attempt(&conn, &id, "x1", "2/3", NOW).unwrap();
        assert!(v.solved && !v.solved_independently);
        assert_eq!(practice_for_thread(&conn, "t").unwrap().len(), 1);
    }

    #[test]
    fn memory_is_proposed_then_approved_and_every_change_bumps_its_revision() {
        use super::super::memory::Kind;
        let conn = db();
        create_thread(&conn, "t", None, None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        assert!(propose_memory(
            &conn,
            "m1",
            Kind::Preference,
            "Prefers worked numbers",
            "t",
            "a1",
            NOW
        )
        .unwrap());
        assert!(!propose_memory(
            &conn,
            "m2",
            Kind::Preference,
            "prefers WORKED numbers",
            "t",
            "a1",
            NOW
        )
        .unwrap());
        assert!(
            sendable_memory(&conn).unwrap().is_empty(),
            "a proposal is never sent"
        );
        assert_eq!(memory_proposals_for_thread(&conn, "t").unwrap().len(), 1);

        let saved = approve_memory(
            &conn,
            "m1",
            Some("Prefers worked numbers before formulas"),
            NOW,
        )
        .unwrap();
        assert_eq!((saved.status.as_str(), saved.revision), ("approved", 2));
        assert_eq!(sendable_memory(&conn).unwrap().len(), 1);
        let off = update_memory(&conn, "m1", None, Some(false), NOW).unwrap();
        assert_eq!(off.revision, 3);
        assert!(sendable_memory(&conn).unwrap().is_empty());
        forget_memory(&conn, "m1").unwrap();
        assert!(list_memory(&conn).unwrap().is_empty());
        assert!(forget_memory(&conn, "m1").is_err());
    }

    #[test]
    fn deleting_a_chat_drops_its_proposals_but_keeps_saved_memory() {
        use super::super::memory::Kind;
        let conn = db();
        create_thread(&conn, "t", None, None, NOW).unwrap();
        snapshot_row(&conn, "s1", "t", &[]);
        begin_run(&conn, &start("r1", "q1", "s1", None, "u1", "a1", 1)).unwrap();
        propose_memory(
            &conn,
            "p",
            Kind::Observation,
            "Mixes up P(A|B) and P(B|A)",
            "t",
            "a1",
            NOW,
        )
        .unwrap();
        propose_memory(
            &conn,
            "k",
            Kind::Preference,
            "Explain in Hindi when stuck",
            "t",
            "a1",
            NOW,
        )
        .unwrap();
        approve_memory(&conn, "k", None, NOW).unwrap();
        delete_thread(&conn, "t").unwrap();
        let left = list_memory(&conn).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(
            (left[0].id.as_str(), left[0].thread_id.as_ref()),
            ("k", None)
        );
        assert!(add_memory(&conn, "x", Kind::Preference, "   ", NOW).is_err());
        assert!(add_memory(&conn, "y", Kind::Preference, "Short answers please", NOW).is_ok());
    }
}
