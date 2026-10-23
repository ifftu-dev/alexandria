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

/// Deletes a thread with its messages, attachments, snapshots, runs and
/// draft, then drops excerpts no remaining snapshot references.
pub fn delete_thread(conn: &Connection, id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
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
    let tx = conn.unchecked_transaction()?;
    thread(&tx, thread_id)?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM chat_bindings
              WHERE thread_id = ?1 AND kind = 'lesson' AND course_id = ?2 AND element_id = ?3",
            params![thread_id, course_id, element_id],
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
             VALUES (?1, ?2, 'lesson', ?3, ?4, ?5, ?6)",
            params![id, thread_id, course_id, element_id, label, now],
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
    let changed = tx.execute(
        "UPDATE chat_runs SET state = ?2, finished_at = ?3 WHERE id = ?1 AND state = 'streaming'",
        params![finish.run_id, finish.state, finish.now],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    tx.execute(
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
    tx.commit()?;
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
}
