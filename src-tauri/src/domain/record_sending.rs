//! Sending learning records, as the interface sees it: record destinations,
//! their outbox status, the consent preview and the local access log.
//! Mirrored in `src/types/index.ts`. The logic is
//! [`crate::record_sending`].

use serde::{Deserialize, Serialize};

/// What a recipient receives: records from these courses, completion-level
/// always and interaction-level (individual responses) only when chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordScope {
    pub course_ids: Vec<String>,
    pub include_interactions: bool,
}

/// Who a recipient is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientKind {
    /// A store the learner named, contacted directly.
    Destination,
    /// A record share through an organisation's hosted service.
    Share,
}

/// The identifier a recipient will see, as the learner chooses it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierChoice {
    /// One made only for this recipient (the default).
    #[default]
    Pairwise,
    /// The learner's own DID.
    RealDid,
}

/// A course the learner may include in a scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseChoice {
    pub id: String,
    pub title: String,
}

/// What the consent screen shows before the learner approves a store.
/// Producing it sends nothing: the address is only looked up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationPreview {
    /// The store's xAPI base address, normalised.
    pub endpoint: String,
    /// `device`, `private_network` or `remote`.
    pub location: String,
    /// True for an address on this device or a private network: the learner
    /// must confirm it explicitly.
    pub needs_confirmation: bool,
    /// The account name records will carry with the default identifier.
    pub pairwise_actor: String,
    /// The account name records will carry if the learner chooses their DID.
    pub real_did_actor: String,
    /// Courses the learner is enrolled in.
    pub courses: Vec<CourseChoice>,
}

/// The learner's approval of a store. The secret is stored with the
/// profile's other secrets and never returned.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewDestinationRequest {
    pub address: String,
    pub username: String,
    pub secret: String,
    pub course_ids: Vec<String>,
    #[serde(default)]
    pub include_interactions: bool,
    #[serde(default)]
    pub identifier: IdentifierChoice,
    /// The learner confirmed a private or on-device address.
    #[serde(default)]
    pub confirm_private: bool,
}

/// Where a recipient's queue stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxStatus {
    /// Waiting to be sent, including records waiting for a connection.
    pub queued: u32,
    /// Refused on the last try; retried at `next_attempt_at`.
    pub failed: u32,
    pub sent: u32,
    /// The most recent reason a try did not succeed, while any record waits.
    pub last_error: Option<String>,
    /// The earliest retry time among waiting records.
    pub next_attempt_at: Option<String>,
    pub last_sent_at: Option<String>,
}

/// A record destination as the interface lists it. It never carries the
/// store's secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordDestination {
    pub id: String,
    pub endpoint: String,
    pub location: String,
    pub username: String,
    /// `pairwise`, `real_did` or `proposed`.
    pub identifier: String,
    pub actor_name: String,
    pub scope: RecordScope,
    pub xapi_version: String,
    pub created_at: String,
    pub status: OutboxStatus,
}

/// What removing a destination did. Sending has stopped either way; voiding
/// is best effort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalReport {
    /// Records the store was asked to void.
    pub voided: u32,
    /// Why voiding did not complete, if it did not.
    pub void_error: Option<String>,
}

/// What an access log row records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessAction {
    /// Records sent.
    Send,
    /// Voiding statements for records sent earlier, on removal.
    Void,
}

/// One send in the learner-readable access log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordAccessEntry {
    pub seq: i64,
    pub at: String,
    pub recipient_id: String,
    pub recipient_kind: RecipientKind,
    /// What the learner knows the recipient as.
    pub recipient_name: String,
    pub action: AccessAction,
    pub scope: RecordScope,
    pub record_count: u32,
    /// The account name the records carried.
    pub identifier: String,
    /// True when the recipient accepted them.
    pub accepted: bool,
    /// Why the recipient refused them.
    pub detail: Option<String>,
}
