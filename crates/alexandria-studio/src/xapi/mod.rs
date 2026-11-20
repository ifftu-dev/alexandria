//! xAPI statements for learning records.
//!
//! A pure builder: learning events plus a recipient go in, validated xAPI
//! statements come out. It has no Tauri, database or network dependency, so
//! the app, the CLI and tests share one implementation (ADR-0004). Storage is
//! the learning records module's job and sending is the outbox's.
//!
//! - Statements are written as xAPI 2.0 within the subset that is also valid
//!   1.0.3, so one statement serves both versions ([`XAPI_VERSIONS`]).
//! - Activity identifiers are DID URLs under the DID that created the course
//!   ([`ActivityRef`]), in one canonical percent-encoding, capped at
//!   [`MAX_IRI_LEN`] with a digest fallback.
//! - The actor is an account under the fixed [`ACCOUNT_HOME_PAGE`]. Statement
//!   and registration identifiers are UUIDv8 from a keyed hash per recipient
//!   ([`Recipient`]), so the same event always gives a recipient the same
//!   identifiers and no two recipients share one.
//! - Verbs come from the ADL and cmi5 vocabularies ([`Verb`]). SCORM run-time
//!   data maps to events through [`scorm`], following the xAPI SCORM profile.
//! - The standard's per-statement signature is never attached: it allows only
//!   RSA algorithms the learner's key cannot produce. An export is signed
//!   whole, detached, by the learner's Ed25519 key.

mod build;
mod identifiers;
pub mod scorm;
mod statement;

pub use build::{
    build_batch, build_statement, Action, BuildOptions, Interaction, InteractionComponent,
    InteractionType, LearningEvent, Names, Outcome, Profile, RecordKind, Target, Verb,
};
pub use identifiers::{pairwise_did, ActivityRef, IdKey, Recipient};
pub use statement::{
    Account, Activity, Agent, Context, ContextActivities, ContextActivity, Definition, LanguageMap,
    Score, Statement, StatementObject, StatementResult, VerbObject,
};

/// The account home page every actor carries. It names the identifier
/// scheme, not a place. Part of every recipient's actor identity: never
/// change it.
pub const ACCOUNT_HOME_PAGE: &str = "urn:uuid:044eac38-2871-47c7-a8d8-5c25a8a9479c";

/// `context.platform` on every statement the app writes.
pub const PLATFORM: &str = "Alexandria";

/// The xAPI versions a built statement is valid in, newest first, as the
/// `X-Experience-API-Version` header spells them.
pub const XAPI_VERSIONS: [&str; 2] = ["2.0.0", "1.0.3"];

/// Longest identifier any tested store keeps (ADL: varchar(2083)).
pub const MAX_IRI_LEN: usize = 2083;

/// Longest account name any tested store keeps (ADL: varchar(50)).
pub const MAX_ACCOUNT_NAME_LEN: usize = 50;

/// Why an event or statement was refused. The text before the colon is a
/// stable code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StatementError {
    #[error("invalid_did: {0}")]
    InvalidDid(String),
    #[error("invalid_identifier: {0}")]
    InvalidIdentifier(String),
    #[error("invalid_activity: {0}")]
    InvalidActivity(String),
    #[error("invalid_account: {0}")]
    InvalidAccount(String),
    #[error("invalid_result: {0}")]
    InvalidResult(String),
    #[error("invalid_language: {0}")]
    InvalidLanguage(String),
    #[error("invalid_statement: {0}")]
    InvalidStatement(String),
}

impl StatementError {
    /// The stable refusal code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidDid(_) => "invalid_did",
            Self::InvalidIdentifier(_) => "invalid_identifier",
            Self::InvalidActivity(_) => "invalid_activity",
            Self::InvalidAccount(_) => "invalid_account",
            Self::InvalidResult(_) => "invalid_result",
            Self::InvalidLanguage(_) => "invalid_language",
            Self::InvalidStatement(_) => "invalid_statement",
        }
    }
}
