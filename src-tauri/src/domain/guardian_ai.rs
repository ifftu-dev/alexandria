// SPDX-License-Identifier: AGPL-3.0-or-later
//! Guardian approvals of remote AI connections, on both sides of a guardian
//! link: the learner's own requests, what travels to the guardian, the
//! guardian's view and the decision that comes back. Mirrors
//! `ChatGuardianApproval`, `GuardianAiRequest` and `GuardianAiRequestStatus`
//! in `src/types/index.ts`; `ai_access` owns every read and write of them.

use serde::{Deserialize, Serialize};

use crate::vocabulary::vocabulary;

vocabulary! {
    /// Where a request stands with the guardian. Stored as written here in
    /// `ai_connection_approvals.status` (the learner's side) and
    /// `guardian_ai_requests.status` (the guardian's).
    GuardianAiRequestStatus {
        /// The guardian has not decided yet.
        Pending => "pending",
        Approved => "approved",
        /// Declined, or an approval withdrawn.
        Declined => "declined",
    }
}

impl GuardianAiRequestStatus {
    /// The status a guardian's decision leaves the request in.
    pub const fn of_decision(approved: bool) -> Self {
        if approved {
            Self::Approved
        } else {
            Self::Declined
        }
    }
}

/// A request for a guardian's approval of a remote connection, as the
/// learner sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianApproval {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub endpoint: String,
    pub model: String,
    pub status: GuardianAiRequestStatus,
    pub requested_at: String,
    pub decided_at: Option<String>,
}

/// An open request as it travels to the guardian: what the guardian sees.
/// The guardian sync seals it as it is into the link's approval-request
/// message, so these field names are the peer-to-peer wire format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub connection_name: String,
    pub endpoint: String,
    pub model: String,
}

/// A ward's request, as the guardian sees it in the overview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianAiRequest {
    pub id: String,
    pub link_id: String,
    pub ward_name: Option<String>,
    pub connection_name: String,
    pub endpoint: String,
    pub model: String,
    pub status: GuardianAiRequestStatus,
    pub received_at: String,
    pub decided_at: Option<String>,
    /// The current decision reached the ward.
    pub delivered: bool,
}

/// A guardian's decision on a ward's request. The guardian sync seals it as
/// it is into the link's approval-decision message, so these field names are
/// the peer-to-peer wire format. The latest decision applies, so an approval
/// can be withdrawn later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianDecision {
    pub request_id: String,
    pub endpoint: String,
    pub approved: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_status_crosses_the_bridge_as_its_stored_string() {
        for (status, text) in [
            (GuardianAiRequestStatus::Pending, "pending"),
            (GuardianAiRequestStatus::Approved, "approved"),
            (GuardianAiRequestStatus::Declined, "declined"),
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), text);
            assert_eq!(GuardianAiRequestStatus::parse(text), Some(status));
        }
        assert!(serde_json::from_value::<GuardianAiRequestStatus>("withdrawn".into()).is_err());
        assert_eq!(
            GuardianAiRequestStatus::of_decision(false),
            GuardianAiRequestStatus::Declined
        );
    }

    #[test]
    fn a_request_keeps_the_peer_to_peer_field_names() {
        let request = ApprovalRequest {
            request_id: "req".into(),
            connection_name: "Work OpenAI".into(),
            endpoint: "https://api.openai.com/v1".into(),
            model: "gpt-5.1-mini".into(),
        };
        let wire = serde_json::json!({
            "request_id": "req",
            "connection_name": "Work OpenAI",
            "endpoint": "https://api.openai.com/v1",
            "model": "gpt-5.1-mini",
        });
        assert_eq!(serde_json::to_value(&request).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ApprovalRequest>(wire).unwrap(),
            request
        );
    }

    #[test]
    fn a_decision_keeps_the_peer_to_peer_field_names() {
        let decision = GuardianDecision {
            request_id: "req".into(),
            endpoint: "https://api.openai.com/v1".into(),
            approved: true,
        };
        let wire = serde_json::json!({
            "request_id": "req",
            "endpoint": "https://api.openai.com/v1",
            "approved": true,
        });
        assert_eq!(serde_json::to_value(&decision).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<GuardianDecision>(wire).unwrap(),
            decision
        );
    }
}
