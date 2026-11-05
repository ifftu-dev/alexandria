// SPDX-License-Identifier: AGPL-3.0-or-later
//! Notification types as the webview sees them. Mirrors the notification
//! section of `src/types/index.ts`; the vocabularies here are generated into
//! `src/generated/notification-vocab.ts` (see `notifications::vocabulary`).
//!
//! These cross the Tauri bridge only. The Cloud wire contract (the FCM data
//! keys and the Cloud push API bodies) is defined where it is spoken, in
//! `notifications::push`, and keeps its own field names.

use serde::{Deserialize, Serialize};

use crate::vocabulary::vocabulary;

vocabulary! {
    /// What a notification is about. The app and Alexandria Cloud share the
    /// strings; they are stored in the `notifications` table as written here.
    NotificationKind {
        InterviewInvited => "interview_invited",
        InterviewRoomOpened => "interview_room_opened",
        OfferSent => "offer_sent",
        AccessRequested => "access_requested",
        AssessmentRequested => "assessment_requested",
        ClassroomCallStarted => "classroom_call_started",
        ClassroomJoinRequested => "classroom_join_requested",
        CredentialIssued => "credential_issued",
        GuardianRequest => "guardian_request",
        /// The fallback for a stored kind this build does not know.
        Generic => "generic",
    }
}

impl NotificationKind {
    /// A kind read from storage. A kind this build does not know (written by
    /// a newer build, say) reads as [`Self::Generic`] rather than failing the
    /// whole list.
    pub fn stored(text: &str) -> Self {
        Self::parse(text).unwrap_or(Self::Generic)
    }
}

vocabulary! {
    /// Where a notification came from.
    NotificationSource {
        /// Produced by this app from something it observed itself.
        Local => "local",
        /// Produced from a remote push the platform delivered.
        Push => "push",
    }
}

vocabulary! {
    /// Why a push registration or removal failed. The webview translates each
    /// code, and the codes are stored as written here, so renaming one is a
    /// storage change.
    PushErrorCode {
        /// The learner did not allow notifications on this device.
        PermissionDenied => "permission_denied",
        /// The phone's push service did not hand over a token.
        TokenUnavailable => "token_unavailable",
        /// The Cloud origin is neither https nor loopback, so no proof is sent.
        InsecureOrigin => "insecure_origin",
        /// The request never got an answer from Cloud.
        CloudUnreachable => "cloud_unreachable",
        /// Cloud answered with an error status.
        CloudRejected => "cloud_rejected",
        /// Cloud answered without acknowledging this mutation's version, or
        /// answered a removal with something other than 204.
        CloudUnacknowledged => "cloud_unacknowledged",
        /// Cloud's answer to a registration had no valid recipient binding.
        InvalidResponse => "invalid_response",
    }
}

vocabulary! {
    /// The answer to asking the OS whether the app may show banners. Named
    /// apart from the browser's own `NotificationPermission` type, which the
    /// webview's TypeScript also sees.
    NotificationPermissionOutcome {
        /// Banners are allowed. The desktop always answers this.
        Granted => "granted",
        /// The learner refused, now or earlier.
        Denied => "denied",
        /// The system prompt could not run at all.
        PromptUnavailable => "prompt_unavailable",
    }
}

impl NotificationPermissionOutcome {
    /// The outcome of the system prompt: its answer, or why it could not run.
    pub fn of_prompt(answer: Result<bool, String>) -> Self {
        match answer {
            Ok(true) => Self::Granted,
            Ok(false) => Self::Denied,
            Err(_) => Self::PromptUnavailable,
        }
    }
}

/// A notification in the profile's inbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationRow {
    pub id: String,
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    pub route: Option<String>,
    pub source: NotificationSource,
    pub created_at: String,
    pub read_at: Option<String>,
    pub payload: Option<serde_json::Value>,
}

/// Whether this device is registered for push, and with which service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushStatus {
    pub supported: bool,
    pub registered: bool,
    pub platform: Option<String>,
    pub cloud_origin: Option<String>,
    /// Why the last attempt failed, as a code the webview translates.
    pub last_error_code: Option<PushErrorCode>,
    /// The untranslated failure text, for diagnostics. Never shown as the
    /// learner-facing message.
    pub last_error_detail: Option<String>,
    pub enabled: bool,
    pub pending_cleanup: bool,
    pub pending_registration: bool,
    /// Push is on but organisations are off, so nothing is registered with
    /// Cloud. The push preference is kept for when organisations come back.
    pub paused_by_organisations: bool,
}

/// A banner or push tap, resolved locally: the profile and network come from
/// this device's own bindings, never from provider data.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationOpen {
    pub id: String,
    pub profile_id: String,
    pub recipient: Option<String>,
    pub route: String,
    pub network_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_permission_prompt_has_one_outcome_per_real_case() {
        assert_eq!(
            NotificationPermissionOutcome::of_prompt(Ok(true)),
            NotificationPermissionOutcome::Granted
        );
        assert_eq!(
            NotificationPermissionOutcome::of_prompt(Ok(false)),
            NotificationPermissionOutcome::Denied
        );
        assert_eq!(
            NotificationPermissionOutcome::of_prompt(Err("no activity".into())),
            NotificationPermissionOutcome::PromptUnavailable
        );
        assert_eq!(
            serde_json::to_value(NotificationPermissionOutcome::PromptUnavailable).unwrap(),
            "prompt_unavailable"
        );
    }

    #[test]
    fn a_stored_kind_this_build_does_not_know_reads_as_generic() {
        assert_eq!(
            NotificationKind::stored("offer_sent"),
            NotificationKind::OfferSent
        );
        assert_eq!(
            NotificationKind::stored("future_kind"),
            NotificationKind::Generic
        );
        assert!(serde_json::from_value::<NotificationKind>("future_kind".into()).is_err());
    }
}
