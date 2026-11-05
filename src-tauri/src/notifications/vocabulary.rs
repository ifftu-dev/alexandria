// SPDX-License-Identifier: AGPL-3.0-or-later
//! The notification vocabularies the webview must know, as one snapshot.
//!
//! The vocabularies themselves are domain types in `domain::notification`.
//! `scripts/notification-vocab.json` is a snapshot of them, checked by the
//! `notification_vocab_snapshot_is_current` test below and rewritten by
//! `npm run generate:notification-vocab`, which then has
//! `scripts/check-tauri-commands.mjs --write` render it into
//! `src/generated/notification-vocab.ts`. Both guards run in CI, so adding a
//! variant without regenerating fails the build rather than drifting.

use serde::Serialize;

use super::{NotificationKind, NotificationPermissionOutcome, NotificationSource, PushErrorCode};

/// The whole notification vocabulary as the webview generator reads it.
#[derive(Serialize)]
struct Snapshot {
    _comment: &'static str,
    notification_kinds: Vec<&'static str>,
    notification_sources: Vec<&'static str>,
    notification_permission_outcomes: Vec<&'static str>,
    push_error_codes: Vec<&'static str>,
}

fn names<T: Copy>(all: &[T], as_str: fn(T) -> &'static str) -> Vec<&'static str> {
    all.iter().map(|&item| as_str(item)).collect()
}

/// The snapshot `scripts/check-tauri-commands.mjs` renders into TypeScript.
pub fn snapshot_json() -> String {
    let snapshot = Snapshot {
        _comment: "Written by `npm run generate:notification-vocab` from src-tauri/src/domain/notification.rs, and read by scripts/check-tauri-commands.mjs --write. Do not edit by hand.",
        notification_kinds: names(NotificationKind::ALL, NotificationKind::as_str),
        notification_sources: names(NotificationSource::ALL, NotificationSource::as_str),
        notification_permission_outcomes: names(
            NotificationPermissionOutcome::ALL,
            NotificationPermissionOutcome::as_str,
        ),
        push_error_codes: names(PushErrorCode::ALL, PushErrorCode::as_str),
    };
    let mut json = serde_json::to_string_pretty(&snapshot).expect("vocabulary serialises");
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocabulary::assert_round_trips as round_trips;

    #[test]
    fn every_notification_vocabulary_round_trips_through_its_string_form() {
        round_trips(
            NotificationKind::ALL,
            NotificationKind::as_str,
            NotificationKind::parse,
        );
        round_trips(
            NotificationSource::ALL,
            NotificationSource::as_str,
            NotificationSource::parse,
        );
        round_trips(
            NotificationPermissionOutcome::ALL,
            NotificationPermissionOutcome::as_str,
            NotificationPermissionOutcome::parse,
        );
        round_trips(
            PushErrorCode::ALL,
            PushErrorCode::as_str,
            PushErrorCode::parse,
        );
    }

    /// The checked-in snapshot equals the notification vocabularies. With
    /// `UPDATE_NOTIFICATION_VOCAB=1` (set by `npm run
    /// generate:notification-vocab`) the snapshot is rewritten instead.
    #[test]
    fn notification_vocab_snapshot_is_current() {
        crate::vocabulary::assert_snapshot_current(
            "../scripts/notification-vocab.json",
            &snapshot_json(),
            "UPDATE_NOTIFICATION_VOCAB",
            "npm run generate:notification-vocab",
        );
    }
}
