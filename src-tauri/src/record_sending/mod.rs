//! Sending learning records to someone the learner approved.
//!
//! The mechanics every way of sending shares, and the first of them, record
//! destinations (stores the learner names and the app contacts directly).
//!
//! - **Recipients.** A recipient ([`RecipientEntry`]) is a row the learner
//!   approved: its kind (a destination, or a record share through the hosted
//!   service), its label, the identifier it sees ([`Identity`]) and its
//!   [`Scope`]. Its id names its rows in the per-recipient outbox.
//! - **Identifiers at send.** Records are stored naming the learner by DID.
//!   Each send builds statements for the recipient from its label: by default
//!   an actor made only for it, never the learner's DID unless the learner
//!   chose that ([`recipients::recipient`]).
//! - **Scope.** Course ids and record kinds; completion-level only unless the
//!   learner also chose interaction-level ([`Scope::admits`]). Checked when
//!   records are queued and again when they are sent.
//! - **Outbox.** Records in scope are queued per recipient ([`outbox`]),
//!   sent in batches oldest first, kept while the recipient cannot be reached
//!   and retried with backoff; a refusal is kept as a visible failure.
//! - **Access log.** Every send that reached a recipient, and every voiding
//!   on removal, is a row in the learner-readable log ([`access_log`]).
//! - **Destinations** ([`destinations`]) work with the organisations switch
//!   off and need no hosted service.
//!
//! Outside the commands layer so commands, the background sender and record
//! shares share one implementation. Database work runs through [`RecordDb`];
//! nothing holds the database across a request.

pub mod access_log;
pub mod destinations;
pub mod outbox;
pub mod recipients;
mod send;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod xapi_stub;

use std::future::Future;
use std::pin::Pin;

use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::db::Database;
use crate::learning_records::{LearningRecord, RecordKind};
use crate::p2p::inbound::InboundDatabase;
use crate::profile::scope::ProfileLease;
use alexandria_studio::xapi::lrs::LrsError;
use alexandria_studio::xapi::{Account, Statement, StatementError};

pub use crate::domain::record_sending::{RecipientKind, RecordScope as Scope};
pub use send::{send_due, void_sent, BatchOutcome};

/// Why sending could not go ahead. Displays as `code: detail`.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("not_found: {0}")]
    NotFound(String),
    #[error("confirmation_required: {0}")]
    ConfirmationRequired(String),
    #[error("already_added: {0}")]
    AlreadyAdded(String),
    #[error("invalid_scope: {0}")]
    InvalidScope(String),
    #[error("invalid_credentials: {0}")]
    InvalidCredentials(String),
    #[error("{0}")]
    Store(#[from] LrsError),
    #[error("invalid_record: {0}")]
    Statement(#[from] StatementError),
    #[error("database: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for SendError {
    fn from(error: rusqlite::Error) -> Self {
        SendError::Database(error.to_string())
    }
}

impl From<serde_json::Error> for SendError {
    fn from(error: serde_json::Error) -> Self {
        SendError::Database(error.to_string())
    }
}

impl From<crate::learning_records::RecordError> for SendError {
    fn from(error: crate::learning_records::RecordError) -> Self {
        SendError::Database(error.to_string())
    }
}

impl RecipientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Destination => "destination",
            Self::Share => "share",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, SendError> {
        match value {
            "destination" => Ok(Self::Destination),
            "share" => Ok(Self::Share),
            other => Err(SendError::Database(format!(
                "unknown recipient kind {other}"
            ))),
        }
    }
}

/// The actor a recipient sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// The default: an identifier made only for this recipient.
    Pairwise,
    /// The learner chose to be known by their own DID.
    RealDid,
    /// The learner approved an account the recipient proposed.
    Proposed(Account),
}

impl Identity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pairwise => "pairwise",
            Self::RealDid => "real_did",
            Self::Proposed(_) => "proposed",
        }
    }
}

impl Scope {
    /// Whether `record` may go to a recipient with this scope.
    pub fn admits(&self, record: &LearningRecord) -> bool {
        self.course_ids.contains(&record.course_id)
            && (record.kind == RecordKind::Completion || self.include_interactions)
    }

    /// Refuses an empty scope or one naming a course twice.
    pub fn validate(&self) -> Result<(), SendError> {
        if self.course_ids.is_empty() {
            return Err(SendError::InvalidScope("choose at least one course".into()));
        }
        let mut seen = std::collections::HashSet::new();
        if let Some(twice) = self.course_ids.iter().find(|id| !seen.insert(id.as_str())) {
            return Err(SendError::InvalidScope(format!(
                "course {twice} is named twice"
            )));
        }
        Ok(())
    }
}

/// A recipient the learner approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientEntry {
    /// Names the recipient's rows in the outbox.
    pub id: String,
    pub kind: RecipientKind,
    /// Every identifier the recipient sees is derived from it. Fixed when
    /// the recipient is approved.
    pub label: String,
    pub identity: Identity,
    /// The account name the recipient's records carry, as shown at consent.
    pub actor_name: String,
    pub scope: Scope,
    pub created_at: String,
}

/// Delivers built statements to one recipient. Record destinations post
/// them to a store; record shares send them to the hosted service. Errors
/// use the store's categories: [`LrsError::Unreachable`] means nothing left
/// the device (kept queued), anything else is a failure the learner sees.
pub trait Transport: Send + Sync {
    fn deliver<'a>(
        &'a self,
        statements: &'a [Statement],
    ) -> Pin<Box<dyn Future<Output = Result<(), LrsError>> + Send + 'a>>;
}

/// The database, as sending reaches it: one job per step, never held across
/// a request.
pub trait RecordDb: Send + Sync {
    fn run<T, F>(
        &self,
        label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static;
}

/// Background work (the sender), fenced to its profile session.
impl RecordDb for InboundDatabase {
    fn run<T, F>(
        &self,
        label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        InboundDatabase::run(self, label, operation)
    }
}

/// A command's database access, on the learner lane under its lease.
pub struct LeasedDb {
    executor: DatabaseExecutor,
    lease: ProfileLease,
}

impl LeasedDb {
    pub(crate) fn new(executor: DatabaseExecutor, lease: ProfileLease) -> Self {
        Self { executor, lease }
    }
}

impl RecordDb for LeasedDb {
    fn run<T, F>(
        &self,
        label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.executor.execute(
            DatabaseWorkload::Learner,
            self.lease.clone(),
            label,
            operation,
        )
    }
}

/// Current time as the tables store it.
pub(crate) fn timestamp(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
