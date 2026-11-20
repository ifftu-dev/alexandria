//! The hosted service's candidate API as JSON: the shapes the service sends
//! and the requests the app makes. Copied from the service on purpose (the
//! two sides share no crate); the service's API document is the reference.
//!
//! Item text arrives in [`Item`]. Its `Debug` leaves the text out, so an item
//! formatted into a log line or an error says which item it was and nothing
//! of what it asked.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The format token of the terms this client reads.
pub const TERMS_FORMAT: &str = "alexandria-hosted-attempt-terms/1";

/// What the candidate sees and accepts before the first item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Terms {
    pub format: String,
    pub organisation: String,
    pub role: String,
    pub items: u32,
    pub max_probes: u32,
    /// `any` or `device_attested`.
    pub device_requirement: String,
    pub retakes: Retakes,
    pub disconnect_grace_secs: u64,
    pub retention_days: u32,
    /// The consent text, in order.
    pub statements: Vec<String>,
}

impl Terms {
    /// The role requires an attested phone: each answer carries a platform
    /// assertion.
    pub fn requires_device_attested(&self) -> bool {
        self.device_requirement == "device_attested"
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Retakes {
    pub limit: u32,
    pub wait_days: u32,
    pub abandoned_counts: bool,
}

/// `GET …/invites/{invite}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TermsReply {
    pub terms: Terms,
    /// Hex SHA-256 of the terms' JCS serialisation.
    pub terms_digest: String,
    /// An attempt to resume.
    pub open_attempt: Option<String>,
    pub attempts_used: i64,
    pub attempts_allowed: i64,
    /// Set while a retake wait runs.
    pub next_attempt_at: Option<String>,
    pub can_open: bool,
}

/// The candidate's acceptance, sent in the open request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Consent {
    pub accepted: bool,
    pub terms_digest: String,
}

/// The App Attest key an attested attempt registers at open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceKey {
    /// Standard base64.
    pub key_id: String,
    /// Standard base64 of the attestation object.
    pub attestation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenBody {
    pub consent: Consent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_key: Option<DeviceKey>,
}

/// Open, resume and answer all reply with this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptState {
    pub attempt: AttemptInfo,
    /// `None` once every item is answered.
    pub item: Option<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptInfo {
    pub id: String,
    pub status: String,
    pub opened_at: String,
    pub items: i64,
    pub answered: i64,
    pub max_probes: i64,
    pub requires_attested: bool,
    pub ready_to_close: bool,
}

/// One item as the service sent it, options in this attempt's order.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub seq: i32,
    pub probe: bool,
    pub question: String,
    pub options: Vec<String>,
    /// `one` or `many`.
    pub select: String,
    pub time_limit_ms: i64,
    pub sent_at: String,
    /// On the service's clock: it keeps running while disconnected.
    pub remaining_ms: i64,
}

impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item")
            .field("seq", &self.seq)
            .field("probe", &self.probe)
            .field("options", &self.options.len())
            .field("time_limit_ms", &self.time_limit_ms)
            .field("remaining_ms", &self.remaining_ms)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AnswerBody {
    pub seq: i32,
    /// Displayed positions; empty for no answer.
    pub selected: Vec<i64>,
    /// Stored beside the service's timings and never read by it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_timings: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloseBody {
    /// The session's integrity assertion, camelCase, with `commitmentRoot`.
    pub integrity: Value,
    /// Platform evidence over the close digest, on an attested phone or with
    /// a paired phone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Value>,
}

/// The service's grade over the drawn items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub correct: i64,
    pub total: i64,
    pub fraction: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probes {
    pub max: i64,
    pub asked: i64,
    pub passed: i64,
    pub failed: i64,
    pub late: i64,
    pub unavailable: i64,
    pub probed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptResult {
    pub score: Score,
    pub probes: Probes,
    pub flagged: bool,
}

/// What became of a closed attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Issuance {
    /// Signed as the organisation; collected through the signed credential
    /// fetch.
    Issued { credential_id: String },
    /// The first unmet requirement: `code` plus the rule's own fields.
    Refused { refusal: Value },
    /// Everything but the `live` rung is met.
    Held { waiting_for: String },
    /// The run carried no issuance policy.
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedAttempt {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub closed_at: Option<String>,
}

/// `POST …/close` and `GET …/result`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseReply {
    pub attempt: ClosedAttempt,
    pub result: AttemptResult,
    pub issuance: Issuance,
    #[serde(default)]
    pub close_digest: Option<String>,
}

/// Every refusal: `{"error": "...", "code": "...", ...}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServiceRefusal {
    #[serde(default)]
    pub error: String,
    pub code: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}
