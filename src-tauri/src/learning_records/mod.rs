//! Learning records: what the app records about the learner's own activity.
//!
//! - **Events, not statements.** A record is stored as an
//!   [`alexandria_studio::xapi::LearningEvent`] with the learner's DID beside
//!   it. Whoever reads it (the activity log, a record destination, a record
//!   share) gets a statement built for them by [`statement`], with their own
//!   actor and identifiers. Each event is validated by building it once
//!   before it is stored.
//! - **Written by the host.** Enrolment, element launch, progress,
//!   completion, pass and fail are recorded by [`crate::enrollment`]; package
//!   launches and package results by [`crate::packages::attempts`]. A record
//!   that cannot be written never fails the learner's action: the writers log
//!   and carry on.
//! - **Two kinds.** Interaction-level records (responses a package reported)
//!   are kept on the device and leave it only under a scope that names them;
//!   everything else is completion-level. Alexandria's own assessments are
//!   recorded as a result and a score only, never per answer.
//! - **A visible size limit** ([`LIMIT_BYTES`], reported by [`usage`]). Past
//!   it the oldest interaction-level records go first, then completion-level
//!   records are kept as one summary per attempt at each activity.
//! - **Per profile.** The tables live in the profile's own database.

pub mod activity_log;
mod limit;
mod unit_records;
mod write;

#[cfg(test)]
mod activity_log_tests;
#[cfg(test)]
mod tests;

use alexandria_studio::xapi::{
    self, Account, BuildOptions, IdKey, LearningEvent, Recipient, Statement, StatementError,
};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

pub use alexandria_studio::xapi::RecordKind;
pub use limit::{enforce_limit, usage, LIMIT_BYTES};
pub use write::{
    package_activity_ref, record_cmi5, record_enrolment, record_package_launch,
    record_package_session, record_progress, Cmi5Record, PriorProgress,
};

/// A record that could not be written never fails the learner's action: the
/// host calls this with the writer's result and carries on.
pub fn log_unwritten(result: Result<(), RecordError>) {
    if let Err(error) = result {
        log::warn!("learning record not written: {error}");
    }
}

/// The recipient label of the learner's own activity log. Statements built
/// for the log carry the learner's DID; their identifiers come from this
/// label. Never change it: exported identifiers would change with it.
pub const LOCAL_LOG_LABEL: &str = "activity-log";

/// Why a record was not written. Displays as `code: detail`.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// The profile has no DID yet, or the course's creator is not known, so
    /// the record cannot name its learner or its activity.
    #[error("unnamed: {0}")]
    Unnamed(String),
    /// The event does not build into a valid statement.
    #[error("invalid_record: {0}")]
    Invalid(#[from] StatementError),
    #[error("database: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for RecordError {
    fn from(error: rusqlite::Error) -> Self {
        RecordError::Database(error.to_string())
    }
}

impl From<serde_json::Error> for RecordError {
    fn from(error: serde_json::Error) -> Self {
        RecordError::Database(error.to_string())
    }
}

/// One learning record as the activity log holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LearningRecord {
    pub id: String,
    pub learner_did: String,
    pub kind: RecordKind,
    pub verb: String,
    pub course_id: String,
    pub element_id: Option<String>,
    /// The local enrolment or package attempt id.
    pub registration: Option<String>,
    /// The activity's identifier, a DID URL under the course's creator.
    pub activity_id: String,
    /// Kept as the one summary of its attempt at this activity when the log
    /// was over its limit.
    pub summary: bool,
    pub bytes: u64,
    pub recorded_at: String,
    pub event: LearningEvent,
}

const RECORD_COLUMNS: &str = "id, learner_did, kind, verb, course_id, element_id, registration, \
                              activity_id, summary, bytes, recorded_at, event_json";

/// Every record in the log, newest first.
pub fn list(conn: &Connection) -> Result<Vec<LearningRecord>, RecordError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {RECORD_COLUMNS} FROM learning_records ORDER BY seq DESC"
    ))?;
    let rows = statement.query_map([], from_row)?;
    rows.map(|row| row?).collect()
}

/// One record by its local id.
pub fn get(conn: &Connection, id: &str) -> Result<Option<LearningRecord>, RecordError> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        &format!("SELECT {RECORD_COLUMNS} FROM learning_records WHERE id = ?1"),
        params![id],
        from_row,
    )
    .optional()?
    .transpose()
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<LearningRecord, RecordError>> {
    let id: String = row.get(0)?;
    let kind: String = row.get(2)?;
    let bytes: i64 = row.get(9)?;
    let event_json: String = row.get(11)?;
    let kind = match kind.as_str() {
        "completion" => RecordKind::Completion,
        "interaction" => RecordKind::Interaction,
        other => {
            return Ok(Err(RecordError::Database(format!(
                "record {id}: unknown kind {other}"
            ))))
        }
    };
    let event = match serde_json::from_str(&event_json) {
        Ok(event) => event,
        Err(error) => return Ok(Err(RecordError::Database(format!("record {id}: {error}")))),
    };
    Ok(Ok(LearningRecord {
        id,
        learner_did: row.get(1)?,
        kind,
        verb: row.get(3)?,
        course_id: row.get(4)?,
        element_id: row.get(5)?,
        registration: row.get(6)?,
        activity_id: row.get(7)?,
        summary: row.get(8)?,
        bytes: u64::try_from(bytes).unwrap_or(0),
        recorded_at: row.get(10)?,
        event,
    }))
}

/// The recipient the learner's own activity log is built for: the learner's
/// DID as the actor, identifiers keyed by [`LOCAL_LOG_LABEL`].
/// `learner_secret` is the learner's Ed25519 signing key seed.
pub fn local_recipient(
    learner_secret: &[u8; 32],
    learner_did: &str,
) -> Result<Recipient, StatementError> {
    Recipient::real_did(learner_secret, learner_did, LOCAL_LOG_LABEL)
}

/// Builds the record's statement for `recipient`. The same record and
/// recipient always give the same statement.
pub fn statement(
    record: &LearningRecord,
    recipient: &Recipient,
    options: &BuildOptions,
) -> Result<Statement, StatementError> {
    xapi::build_statement(&record.event, recipient, options)
}

/// Builds `event` once for a stand-in recipient named by `learner_did`, so
/// nothing that would not build is stored. The stand-in's identifier key
/// is fixed: the statement is checked and dropped.
fn validate(event: &LearningEvent, learner_did: &str) -> Result<(), StatementError> {
    let recipient = Recipient::new(
        Account::for_did(learner_did)?,
        IdKey::derive(&[0u8; 32], LOCAL_LOG_LABEL)?,
    );
    xapi::build_statement(event, &recipient, &BuildOptions::default()).map(|_| ())
}
