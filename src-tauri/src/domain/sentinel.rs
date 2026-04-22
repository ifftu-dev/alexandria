//! Sentinel gossip message types.
//!
//! Announcement payload for the `TOPIC_SENTINEL_PRIORS` topic. Carries
//! the metadata needed to mirror a ratified adversarial prior into a
//! peer's local `sentinel_priors` table. The referenced blob (samples)
//! is content-addressed and fetched separately on demand.
//!
//! The envelope's Ed25519 signature (in `SignedGossipMessage`) proves
//! who broadcasted the announcement. The inbound handler additionally
//! verifies the signer is a member of the Sentinel DAO committee,
//! which is the authority gate for ratification.

use serde::{Deserialize, Serialize};

/// Message carried on `/alexandria/sentinel-priors/1.0`.
///
/// All fields are captured at ratification time. The `cid` points to
/// the content-addressed labeled-samples blob; peers pull it lazily
/// when training models that need it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentinelPriorAnnouncement {
    /// Deterministic prior id: `blake2b(cid || label || model_kind)`.
    pub prior_id: String,
    /// Governance proposal that approved this prior. Used by the
    /// inbound handler to confirm the ratification chain locally
    /// (ordering invariant: skip if the proposal isn't known yet).
    pub proposal_id: String,
    /// BLAKE3 content hash of the labeled-samples blob.
    pub cid: String,
    /// Model this prior trains against: `"keystroke"` or `"mouse"`.
    /// Face kind is forbidden and rejected by the inbound handler.
    pub model_kind: String,
    pub label: String,
    pub schema_version: u32,
    pub sample_count: i64,
    /// Optional curator notes; ignored by training but shown in UIs.
    pub notes: Option<String>,
    /// Placeholder for a future Sentinel-DAO threshold signature over
    /// `(cid || label || model_kind || schema_version)`. Today this is
    /// either the on-chain tx hash (if available at ratification) or
    /// the blake2b digest computed at insert time.
    pub signature: String,
    pub ratified_at: String,
}
