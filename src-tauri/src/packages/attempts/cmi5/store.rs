//! The cmi5 tables: sessions, the statements of each attempt and the xAPI
//! documents, and what the launching system derives from them.

use alexandria_studio::package::progress;
use alexandria_studio::xapi::cmi5::{self as rules, Checked, SessionFacts, SessionRecord};
use alexandria_studio::xapi::Satisfiable;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::{roll_up, rows, stored_package, AttemptError};
use super::{millis, move_on_of, LAUNCH_MODE};
use crate::db::Database;
use crate::domain::package::Package;
use crate::domain::package_attempt::{PackageAttempt, PackageAttemptProgress};
use crate::learning_records::{self, Cmi5Record};

/// The activity id a unit is given, and the object of `satisfied` for a
/// block or the course: the DID URL its records carry, or, when the course
/// cannot be named that way, an identifier derived from the package and the
/// course structure identifier. Never the publisher's identifier (8.1.5, 9.4).
fn activity_iri(conn: &Connection, attempt: &PackageAttempt, publisher_id: &str) -> String {
    learning_records::package_activity_ref(conn, attempt, publisher_id)
        .ok()
        .and_then(|activity| activity.iri().ok())
        .unwrap_or_else(|| {
            let digest = blake3::hash(publisher_id.as_bytes());
            format!(
                "urn:alexandria:package:{}:{}",
                attempt.package_hash,
                &digest.to_hex()[..32]
            )
        })
}

/// What a session's launch fixes.
pub(super) fn facts(
    conn: &Connection,
    learner_key: &[u8; 32],
    attempt: &PackageAttempt,
    unit_id: &str,
    learner_id: &str,
    session_id: &str,
    mastery_score: Option<f64>,
) -> SessionFacts {
    SessionFacts {
        actor: rules::actor(learner_id),
        registration: rules::registration(learner_key, &attempt.id),
        session_id: session_id.to_string(),
        activity_id: activity_iri(conn, attempt, unit_id),
        publisher_id: unit_id.to_string(),
        mastery_score,
        launch_mode: LAUNCH_MODE,
    }
}

/// A session's facts, read back from its `launched` statement: what ends a
/// session the app did not launch in this run.
fn facts_from_launched(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<SessionFacts>, AttemptError> {
    let launched: Option<String> = conn
        .query_row(
            "SELECT statement_json FROM cmi5_statements \
             WHERE session_id = ?1 AND verb = 'launched' ORDER BY seq LIMIT 1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(launched) = launched else {
        return Ok(None);
    };
    let statement: Value = serde_json::from_str(&launched)
        .map_err(|e| AttemptError::Database(format!("session {session_id}: {e}")))?;
    let context = &statement["context"];
    Ok(Some(SessionFacts {
        actor: statement["actor"].clone(),
        registration: context["registration"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        session_id: session_id.to_string(),
        activity_id: statement["object"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        publisher_id: context["contextActivities"]["grouping"][0]["id"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        mastery_score: context["extensions"][rules::EXTENSION_MASTERY_SCORE].as_f64(),
        launch_mode: LAUNCH_MODE,
    }))
}

/// One statement row.
pub(super) struct StatementRow<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub activity_id: &'a str,
    pub verb: Option<&'a str>,
    pub score: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub statement: &'a Value,
}

fn insert_statement(
    conn: &Connection,
    attempt_id: &str,
    row: &StatementRow<'_>,
) -> Result<(), AttemptError> {
    conn.execute(
        "INSERT INTO cmi5_statements \
         (id, attempt_id, session_id, activity_id, verb, score, duration_seconds, statement_json, stored_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.id,
            attempt_id,
            row.session_id,
            row.activity_id,
            row.verb,
            row.score,
            row.duration_seconds,
            row.statement.to_string(),
            millis(Utc::now()),
        ],
    )?;
    Ok(())
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Whether the unit used `verb` in the attempt (its registration).
fn used(
    conn: &Connection,
    attempt_id: &str,
    unit_id: &str,
    verb: &str,
) -> Result<bool, AttemptError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM cmi5_statements \
         WHERE attempt_id = ?1 AND activity_id = ?2 AND verb = ?3)",
        params![attempt_id, unit_id, verb],
        |row| row.get(0),
    )?)
}

/// What a launch writes besides its facts.
pub(super) struct Begin {
    pub session_id: String,
    pub unit_id: String,
    pub move_on: alexandria_studio::package::MoveOn,
    pub launch_parameters: Option<String>,
    pub entitlement_key: Option<String>,
    pub return_url: String,
    pub launch_url: String,
    pub languages: String,
}

/// Opens a session: the session row, `LMS.LaunchData`, the learner
/// preferences when none are stored, the unit's stored state, whatever the
/// moveOn criteria satisfy already (9.6.1), then `launched`. Returns what the
/// registration has seen of the unit.
pub(super) fn begin(
    db: &Database,
    attempt: &PackageAttempt,
    package: &Package,
    facts: &SessionFacts,
    begin: &Begin,
) -> Result<SessionRecord, AttemptError> {
    let conn = db.conn();
    let now = Utc::now();
    conn.execute(
        "INSERT INTO cmi5_sessions (id, attempt_id, activity_id, status, launched_at) \
         VALUES (?1, ?2, ?3, 'open', ?4)",
        params![begin.session_id, attempt.id, begin.unit_id, millis(now)],
    )?;
    let values = rules::LaunchValues {
        move_on: begin.move_on,
        launch_parameters: begin.launch_parameters.as_deref(),
        entitlement_key: begin.entitlement_key.as_deref(),
        return_url: Some(&begin.return_url),
        launch_url: &begin.launch_url,
    };
    let launch_data = rules::launch_data(facts, &values);
    put_document(
        conn,
        &DocumentKey {
            package_hash: &package.hash,
            kind: DocumentKind::State,
            activity_id: &facts.activity_id,
            registration: &facts.registration,
            document_id: rules::LAUNCH_DATA_STATE_ID,
        },
        "application/json",
        launch_data.to_string().as_bytes(),
    )?;
    let preferences = DocumentKey {
        package_hash: &package.hash,
        kind: DocumentKind::AgentProfile,
        activity_id: "",
        registration: "",
        document_id: rules::LEARNER_PREFERENCES_PROFILE_ID,
    };
    if get_document(conn, &preferences)?.is_none() {
        put_document(
            conn,
            &preferences,
            "application/json",
            rules::learner_preferences(&begin.languages)
                .to_string()
                .as_bytes(),
        )?;
    }
    // 9.6.1.1-3: the moveOn criteria are evaluated as the registration
    // starts, so what they satisfy already (NotApplicable units) is recorded
    // before the unit is launched.
    refresh_unit(conn, attempt, package, &begin.unit_id)?;
    satisfy(conn, attempt, package, facts)?;
    let launched = rules::launched(&new_id(), facts, &values, now);
    insert_statement(
        conn,
        &attempt.id,
        &StatementRow {
            id: launched["id"].as_str().unwrap_or_default(),
            session_id: &begin.session_id,
            activity_id: &begin.unit_id,
            verb: Some("launched"),
            score: None,
            duration_seconds: None,
            statement: &launched,
        },
    )?;
    learning_records::log_unwritten(learning_records::record_package_launch(
        conn,
        attempt,
        package,
        &begin.unit_id,
    ));
    roll_up(db, attempt, package)?;
    Ok(SessionRecord {
        completed_before: used(conn, &attempt.id, &begin.unit_id, "completed")?,
        passed_before: used(conn, &attempt.id, &begin.unit_id, "passed")?,
        ..SessionRecord::default()
    })
}

/// Brings one unit's stored state up to date from its statements in the
/// attempt: satisfied by its moveOn criteria, its latest judged score and
/// the time of its finished sessions.
fn refresh_unit(
    conn: &Connection,
    attempt: &PackageAttempt,
    package: &Package,
    unit_id: &str,
) -> Result<(), AttemptError> {
    let unit = super::super::launchable(package, unit_id)?;
    let completed = used(conn, &attempt.id, unit_id, "completed")?;
    let passed = used(conn, &attempt.id, unit_id, "passed")?;
    let score: Option<f64> = conn
        .query_row(
            "SELECT score FROM cmi5_statements \
             WHERE attempt_id = ?1 AND activity_id = ?2 AND verb IN ('passed', 'failed') \
               AND score IS NOT NULL \
             ORDER BY verb = 'passed' DESC, seq DESC LIMIT 1",
            params![attempt.id, unit_id],
            |row| row.get(0),
        )
        .optional()?;
    let seconds: f64 = conn.query_row(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM cmi5_statements \
         WHERE attempt_id = ?1 AND activity_id = ?2 AND verb IN ('terminated', 'abandoned')",
        params![attempt.id, unit_id],
        |row| row.get(0),
    )?;
    rows::put_activity(
        conn,
        &attempt.id,
        unit_id,
        &rows::ActivityWrite {
            runtime_json: "{}",
            complete: progress::unit_satisfied(move_on_of(unit), completed, passed),
            score,
            total_seconds: seconds,
            session_seconds: 0.0,
        },
    )
}

/// Records `satisfied` for every block, and the course, whose units have
/// all met their moveOn criteria and that has none yet in the attempt
/// (9.3.9), in the session whose statement satisfied it.
fn satisfy(
    conn: &Connection,
    attempt: &PackageAttempt,
    package: &Package,
    facts: &SessionFacts,
) -> Result<(), AttemptError> {
    let outcomes = rows::outcomes(conn, &attempt.id)?;
    let description = &package.description;
    for node in progress::satisfied_nodes(description, &outcomes) {
        if used(conn, &attempt.id, &node.id, "satisfied")? {
            continue;
        }
        let scope = if node.id == description.tree.id {
            Satisfiable::Course
        } else {
            Satisfiable::Block
        };
        let object_id = activity_iri(conn, attempt, &node.id);
        let statement = rules::satisfied(&new_id(), facts, scope, &object_id, &node.id, Utc::now());
        insert_statement(
            conn,
            &attempt.id,
            &StatementRow {
                id: statement["id"].as_str().unwrap_or_default(),
                session_id: &facts.session_id,
                activity_id: &node.id,
                verb: Some("satisfied"),
                score: None,
                duration_seconds: None,
                statement: &statement,
            },
        )?;
        learning_records::log_unwritten(learning_records::record_cmi5(
            conn,
            attempt,
            package,
            &node.id,
            format!("cmi5|{}|satisfied|{}", attempt.id, node.id),
            &Cmi5Record::Satisfied(scope),
        ));
    }
    Ok(())
}

/// A statement whose id is already stored with other content.
#[derive(Debug)]
pub(super) struct Conflict(pub String);

/// Stores a checked batch from the unit, then updates what it decides: the
/// session's last statement and status, the unit's state, its records,
/// `satisfied` statements and the element's progress. A batch whose ids
/// collide with different stored statements is refused whole (nothing is
/// written); a statement already stored unchanged is skipped.
pub(super) fn store_batch(
    db: &Database,
    attempt_id: &str,
    unit_id: &str,
    facts: &SessionFacts,
    batch: &[(Value, Checked)],
) -> Result<Result<(), Conflict>, AttemptError> {
    let conn = db.conn();
    let attempt = super::super::open_attempt(conn, attempt_id)?;
    let package = stored_package(conn, &attempt.package_hash)?;
    let mut fresh = Vec::new();
    for (statement, checked) in batch {
        let id = statement["id"].as_str().unwrap_or_default();
        let stored: Option<String> = conn
            .query_row(
                "SELECT statement_json FROM cmi5_statements WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        match stored {
            None => fresh.push((statement, checked)),
            Some(stored) => {
                let mut stored: Value = serde_json::from_str(&stored)
                    .map_err(|e| AttemptError::Database(e.to_string()))?;
                if let Some(object) = stored.as_object_mut() {
                    object.remove("stored");
                }
                let mut incoming = statement.clone();
                if let Some(object) = incoming.as_object_mut() {
                    object.remove("stored");
                }
                if stored != incoming {
                    return Ok(Err(Conflict(format!(
                        "statement {id} is stored with other content"
                    ))));
                }
            }
        }
    }
    let now = millis(Utc::now());
    let mut decides = false;
    for (statement, checked) in &fresh {
        let mut statement = (*statement).clone();
        if let Some(object) = statement.as_object_mut() {
            object.insert("stored".into(), now.clone().into());
        }
        let id = statement["id"].as_str().unwrap_or_default().to_string();
        insert_statement(
            conn,
            &attempt.id,
            &StatementRow {
                id: &id,
                session_id: &facts.session_id,
                activity_id: unit_id,
                verb: checked.verb.map(|verb| verb.as_str()),
                score: checked.score,
                duration_seconds: checked.duration_seconds,
                statement: &statement,
            },
        )?;
        let Some(verb) = checked.verb else {
            continue;
        };
        decides = true;
        if verb == rules::UnitVerb::Terminated {
            conn.execute(
                "UPDATE cmi5_sessions SET status = 'terminated', ended_at = ?2 WHERE id = ?1",
                params![facts.session_id, now],
            )?;
        }
        learning_records::log_unwritten(learning_records::record_cmi5(
            conn,
            &attempt,
            &package,
            unit_id,
            format!("cmi5|{}|{id}", attempt.id),
            &Cmi5Record::Result {
                verb,
                score: checked.score,
                duration_seconds: checked.duration_seconds,
            },
        ));
    }
    if !fresh.is_empty() {
        conn.execute(
            "UPDATE cmi5_sessions SET last_statement_at = ?2 WHERE id = ?1",
            params![facts.session_id, now],
        )?;
    }
    if decides {
        refresh_unit(conn, &attempt, &package, unit_id)?;
        satisfy(conn, &attempt, &package, facts)?;
        roll_up(db, &attempt, &package)?;
    }
    Ok(Ok(()))
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// Records `abandoned` for an open session: its length runs from the launch
/// to the unit's last statement (9.5.4.2).
fn abandon(
    conn: &Connection,
    attempt: &PackageAttempt,
    package: &Package,
    session_id: &str,
    facts: &SessionFacts,
) -> Result<(), AttemptError> {
    let (unit_id, launched_at, last): (String, String, Option<String>) = conn.query_row(
        "SELECT activity_id, launched_at, last_statement_at FROM cmi5_sessions WHERE id = ?1",
        params![session_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let seconds = match (
        parse_time(&launched_at),
        last.as_deref().and_then(parse_time),
    ) {
        (Some(from), Some(to)) => ((to - from).num_milliseconds().max(0) as f64) / 1000.0,
        _ => 0.0,
    };
    let now = Utc::now();
    let statement = rules::abandoned(&new_id(), facts, seconds, now);
    insert_statement(
        conn,
        &attempt.id,
        &StatementRow {
            id: statement["id"].as_str().unwrap_or_default(),
            session_id,
            activity_id: &unit_id,
            verb: Some("abandoned"),
            score: None,
            duration_seconds: Some(seconds),
            statement: &statement,
        },
    )?;
    conn.execute(
        "UPDATE cmi5_sessions SET status = 'abandoned', ended_at = ?2 WHERE id = ?1",
        params![session_id, millis(now)],
    )?;
    learning_records::log_unwritten(learning_records::record_cmi5(
        conn,
        attempt,
        package,
        &unit_id,
        format!("cmi5|{}|abandoned|{session_id}", attempt.id),
        &Cmi5Record::Abandoned { seconds },
    ));
    refresh_unit(conn, attempt, package, &unit_id)
}

/// Abandons every session of the attempt still open in the database: an
/// earlier run of the app stopped while they ran.
pub(super) fn abandon_stale(
    db: &Database,
    attempt: &PackageAttempt,
    package: &Package,
) -> Result<(), AttemptError> {
    let conn = db.conn();
    let open: Vec<String> = conn
        .prepare("SELECT id FROM cmi5_sessions WHERE attempt_id = ?1 AND status = 'open'")?
        .query_map(params![attempt.id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for session_id in open {
        match facts_from_launched(conn, &session_id)? {
            Some(facts) => abandon(conn, attempt, package, &session_id, &facts)?,
            None => {
                conn.execute(
                    "UPDATE cmi5_sessions SET status = 'abandoned', ended_at = ?2 WHERE id = ?1",
                    params![session_id, millis(Utc::now())],
                )?;
            }
        }
    }
    Ok(())
}

/// Closes a session in the database: `abandoned` if the unit never
/// terminated it, then the attempt's progress.
pub(super) fn finish(
    db: &Database,
    session_id: &str,
    facts: Option<&SessionFacts>,
) -> Result<PackageAttemptProgress, AttemptError> {
    let conn = db.conn();
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT attempt_id, status FROM cmi5_sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((attempt_id, status)) = row else {
        return Err(AttemptError::NotFound(format!("cmi5 session {session_id}")));
    };
    let attempt = rows::get(conn, &attempt_id)?;
    let package = stored_package(conn, &attempt.package_hash)?;
    if status == "open" {
        let facts = match facts {
            Some(facts) => Some(facts.clone()),
            None => facts_from_launched(conn, session_id)?,
        };
        match facts {
            Some(facts) => abandon(conn, &attempt, &package, session_id, &facts)?,
            None => {
                conn.execute(
                    "UPDATE cmi5_sessions SET status = 'abandoned', ended_at = ?2 WHERE id = ?1",
                    params![session_id, millis(Utc::now())],
                )?;
            }
        }
    }
    roll_up(db, &attempt, &package)
}

// ── Statements, read back ────────────────────────────────────────────────

/// One stored statement of the attempt by id.
pub(super) fn statement(
    conn: &Connection,
    attempt_id: &str,
    id: &str,
) -> Result<Option<Value>, AttemptError> {
    let found: Option<String> = conn
        .query_row(
            "SELECT statement_json FROM cmi5_statements WHERE attempt_id = ?1 AND id = ?2",
            params![attempt_id, id],
            |row| row.get(0),
        )
        .optional()?;
    found
        .map(|json| serde_json::from_str(&json).map_err(|e| AttemptError::Database(e.to_string())))
        .transpose()
}

/// The filters a statements query may use.
#[derive(Debug, Default)]
pub(super) struct StatementQuery {
    pub verb: Option<String>,
    pub activity: Option<String>,
    pub registration: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<usize>,
    pub ascending: bool,
}

/// The attempt's statements matching `query`, newest first unless asked
/// otherwise. Filters compare the statement's own fields.
pub(super) fn statements(
    conn: &Connection,
    attempt_id: &str,
    query: &StatementQuery,
) -> Result<Vec<Value>, AttemptError> {
    let order = if query.ascending { "ASC" } else { "DESC" };
    let mut select = conn.prepare(&format!(
        "SELECT statement_json FROM cmi5_statements WHERE attempt_id = ?1 ORDER BY seq {order}"
    ))?;
    let rows = select.query_map(params![attempt_id], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let statement: Value =
            serde_json::from_str(&row?).map_err(|e| AttemptError::Database(e.to_string()))?;
        if query
            .verb
            .as_deref()
            .is_some_and(|verb| statement["verb"]["id"].as_str() != Some(verb))
        {
            continue;
        }
        if query
            .activity
            .as_deref()
            .is_some_and(|activity| statement["object"]["id"].as_str() != Some(activity))
        {
            continue;
        }
        if query.registration.as_deref().is_some_and(|registration| {
            statement["context"]["registration"].as_str() != Some(registration)
        }) {
            continue;
        }
        let stored = statement["stored"]
            .as_str()
            .or_else(|| statement["timestamp"].as_str())
            .and_then(parse_time);
        let since = query.since.as_deref().and_then(parse_time);
        let until = query.until.as_deref().and_then(parse_time);
        if since.is_some_and(|since| stored.is_none_or(|at| at <= since))
            || until.is_some_and(|until| stored.is_none_or(|at| at > until))
        {
            continue;
        }
        out.push(statement);
        if query
            .limit
            .is_some_and(|limit| limit > 0 && out.len() >= limit)
        {
            break;
        }
    }
    Ok(out)
}

// ── Documents ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DocumentKind {
    State,
    AgentProfile,
    ActivityProfile,
}

impl DocumentKind {
    fn as_str(self) -> &'static str {
        match self {
            DocumentKind::State => "state",
            DocumentKind::AgentProfile => "agent_profile",
            DocumentKind::ActivityProfile => "activity_profile",
        }
    }
}

pub(super) struct DocumentKey<'a> {
    pub package_hash: &'a str,
    pub kind: DocumentKind,
    pub activity_id: &'a str,
    pub registration: &'a str,
    pub document_id: &'a str,
}

/// A stored document.
#[derive(Debug, Clone)]
pub(super) struct Document {
    pub content_type: String,
    pub body: Vec<u8>,
    pub etag: String,
    pub updated_at: String,
}

pub(super) fn etag(body: &[u8]) -> String {
    format!("\"{}\"", hex::encode(Sha256::digest(body)))
}

pub(super) fn get_document(
    conn: &Connection,
    key: &DocumentKey<'_>,
) -> Result<Option<Document>, AttemptError> {
    Ok(conn
        .query_row(
            "SELECT content_type, body, etag, updated_at FROM xapi_documents \
             WHERE package_hash = ?1 AND kind = ?2 AND activity_id = ?3 \
               AND registration = ?4 AND document_id = ?5",
            params![
                key.package_hash,
                key.kind.as_str(),
                key.activity_id,
                key.registration,
                key.document_id
            ],
            |row| {
                Ok(Document {
                    content_type: row.get(0)?,
                    body: row.get(1)?,
                    etag: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )
        .optional()?)
}

pub(super) fn put_document(
    conn: &Connection,
    key: &DocumentKey<'_>,
    content_type: &str,
    body: &[u8],
) -> Result<(), AttemptError> {
    conn.execute(
        "INSERT INTO xapi_documents \
         (package_hash, kind, activity_id, registration, document_id, content_type, body, etag, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(package_hash, kind, activity_id, registration, document_id) DO UPDATE SET \
           content_type = ?6, body = ?7, etag = ?8, updated_at = ?9",
        params![
            key.package_hash,
            key.kind.as_str(),
            key.activity_id,
            key.registration,
            key.document_id,
            content_type,
            body,
            etag(body),
            millis(Utc::now()),
        ],
    )?;
    Ok(())
}

/// Deletes one document, or with no `document_id` every state document of
/// the activity (and registration).
pub(super) fn delete_documents(
    conn: &Connection,
    key: &DocumentKey<'_>,
) -> Result<(), AttemptError> {
    if key.document_id.is_empty() {
        conn.execute(
            "DELETE FROM xapi_documents WHERE package_hash = ?1 AND kind = ?2 \
             AND activity_id = ?3 AND registration = ?4 AND document_id <> ?5",
            params![
                key.package_hash,
                key.kind.as_str(),
                key.activity_id,
                key.registration,
                rules::LAUNCH_DATA_STATE_ID
            ],
        )?;
    } else {
        conn.execute(
            "DELETE FROM xapi_documents WHERE package_hash = ?1 AND kind = ?2 \
             AND activity_id = ?3 AND registration = ?4 AND document_id = ?5",
            params![
                key.package_hash,
                key.kind.as_str(),
                key.activity_id,
                key.registration,
                key.document_id
            ],
        )?;
    }
    Ok(())
}

/// The ids of the documents under a key with no `document_id`.
pub(super) fn document_ids(
    conn: &Connection,
    key: &DocumentKey<'_>,
) -> Result<Vec<String>, AttemptError> {
    let mut select = conn.prepare(
        "SELECT document_id FROM xapi_documents WHERE package_hash = ?1 AND kind = ?2 \
         AND activity_id = ?3 AND registration = ?4 ORDER BY document_id",
    )?;
    let ids = select
        .query_map(
            params![
                key.package_hash,
                key.kind.as_str(),
                key.activity_id,
                key.registration
            ],
            |row| row.get(0),
        )?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}
