//! Sentinel gossip message types.

use serde::{Deserialize, Serialize};

/// Message carried on `/alexandria/integrity-attestation/1.0`.
///
/// A committee attestor node's automated co-signature over a finalized
/// integrity session's terminal attestation payload (§ Integrity→VC
/// bridge P1). The receiving learner records it via
/// `record_attestation_impl`, which re-verifies committee membership,
/// the registered key binding, and the signature before storing — so a
/// forged announcement cannot inflate a session's assurance level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityCoSignAnnouncement {
    /// The `integrity_sessions.id` being attested.
    pub session_id: String,
    /// Attestor's Cardano stake address (committee membership key).
    pub attestor_address: String,
    /// Attestor's Ed25519 public key (hex) — must match the registered
    /// binding for `attestor_address`.
    pub public_key: String,
    /// Ed25519 signature (hex) over the session's terminal payload.
    pub signature: String,
}
