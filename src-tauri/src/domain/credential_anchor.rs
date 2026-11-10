//! Where a credential's integrity anchor stands (spec §11.3.1).
//!
//! Mirrors `CredentialAnchorState` in `src/types/index.ts`.

use serde::{Deserialize, Serialize};

/// Where one credential's anchor stands. Queue states exist for credentials
/// this node issued; a credential received from elsewhere is only ever
/// `Confirmed`, once its imported receipt verifies against the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStatus {
    /// Queued for the next batch; not yet signed and sent.
    Pending,
    /// Sent, but whether the ledger took it is not yet known.
    OutcomeUnknown,
    /// Accepted by the submission endpoint, awaiting confirmation.
    Submitted,
    /// In a block. Only this state makes a credential `integrityAnchored`.
    Confirmed,
    /// Gave up after repeated failures, or rejected on chain.
    Failed,
}

impl AnchorStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Submitted => "submitted",
            Self::Confirmed => "confirmed",
            Self::Failed => "failed",
        }
    }

    /// The stored vocabulary; `None` for a value this build does not know.
    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Pending,
            Self::OutcomeUnknown,
            Self::Submitted,
            Self::Confirmed,
            Self::Failed,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
    }
}

/// One credential's anchor, as the credentials screens show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialAnchorState {
    pub credential_id: String,
    pub status: AnchorStatus,
    /// The anchoring transaction, once one has been signed.
    pub tx_hash: Option<String>,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// The last error is one a wallet balance fixes (fee or collateral).
    pub needs_funds: bool,
    pub enqueued_at: String,
    pub confirmed_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stored_vocabulary_round_trips_through_serde() {
        for status in [
            AnchorStatus::Pending,
            AnchorStatus::OutcomeUnknown,
            AnchorStatus::Submitted,
            AnchorStatus::Confirmed,
            AnchorStatus::Failed,
        ] {
            assert_eq!(AnchorStatus::parse(status.as_str()), Some(status));
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::Value::String(status.as_str().into())
            );
        }
        assert_eq!(AnchorStatus::parse("minted"), None);
    }
}
