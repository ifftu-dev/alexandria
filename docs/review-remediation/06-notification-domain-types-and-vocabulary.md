# Notification domain types and vocabulary

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/01-push-respects-organisations-opt-out.md, alexandria/docs/review-remediation/02-profile-bound-notification-events.md

## Task

- **Domain types.** Move the notification row, push status and notification-open types into the domain module as snake_case types in both Rust and TypeScript. Event payload keys follow. This is IPC only: the Cloud wire contract is unchanged.
- **Vocabulary.**
  - Generate notification kinds and sources from the existing vocabulary macro, including the TypeScript union.
  - Type the row's kind and source.
  - Handle unknown kinds arriving from outside at the boundary, by rejecting them or mapping them to the generic kind.
- **Interview-room route.** Build it in one function, shared by the inbox and Cloud discovery.
- **Permission request.** Return a typed outcome (granted, denied or prompt-unavailable), and make its documentation match.
- **README.** Fix the mobile push plugin README: iOS returns an FCM token derived from the APNs token.

Covers spec stories 17–22.

## Acceptance

- The vocabulary snapshot test covers notification kinds.
- No camelCase notification fields remain on either side.
- Strict `vue-tsc` passes with `kind` typed to the generated union.
- Rust and Vitest suites pass.

## Completion evidence — 2026-11-04

What changed:

- `domain/notification.rs` holds `NotificationRow`, `PushStatus` and `NotificationOpen`, snake_case in Rust and in `src/types/index.ts`. The `notifications:new` and `notifications:open` event payloads follow, and so do the keys of the row `payload` the app writes (`event_id`, `cloud_origin`, `role_label`, `classroom_id`, ...). The Cloud wire is unchanged: `PushData` keeps the FCM data keys, and the Cloud API bodies and envelopes keep theirs.
- The `vocabulary!` macro moved to `src-tauri/src/vocabulary.rs` and now serialises each vocabulary as its string, refusing unknown strings on input. `NotificationKind`, `NotificationSource`, `PushErrorCode` and `NotificationPermission` use it. `notifications::kind` and `Source` are gone. The snapshot (`scripts/chat-vocab.json`) carries all four lists, and `scripts/check-tauri-commands.mjs` renders them into `src/generated/notification-vocab.ts`. The TypeScript types come from that file, so `AppNotification.kind` is the generated union, not a string.
- Unknown kinds: a push whose kind this build does not know, or which Cloud does not send, fails to parse and is dropped. A stored kind this build does not know lists as `generic`, and an unknown stored source lists as `local`. The debug emitter refuses unknown kinds.
- `notifications::interview_room_route` builds the room route for `cloud_copy` (inbox rows and pushes) and for the Cloud discovery check.
- `notifications_permission_request` returns `granted`, `denied` or `prompt_unavailable`, and its doc lists exactly those. The Settings panel shows a message for each outcome through an exhaustive table. The new `permissionPromptUnavailable` string was translated by hand into all nine locales.
- The held-tap file is now snake_case at format version 2. Earlier files are discarded unread, since they are pre-launch data.
- The mobile push plugin README says iOS `getToken()` returns an FCM registration token derived from the APNs token. Its iOS setup also lists bundling `GoogleService-Info.plist`.

Red → green:

- `each_push_failure_is_reported_as_a_stable_code_with_its_raw_detail`, with its helper reading `last_error_code`/`last_error_detail`: red (every pair was `Null`), now green.
- `the_webview_receives_rows_with_snake_case_fields_and_typed_vocabulary`: red (`createdAt`, `readAt`), now green.
- `a_stored_kind_this_build_does_not_know_lists_as_generic`, `the_permission_prompt_has_one_outcome_per_real_case`, `a_stored_kind_this_build_does_not_know_reads_as_generic`: red at compile time, now green.
- `chat_vocab_snapshot_is_current`: red (stale snapshot) until regenerated.
- `scripts/check-tauri-commands.test.mjs`: the stale notification vocabulary test and the missing `notification_kinds` test were red, now green.
- Vitest `NotificationsPanel permission request`: red for all three outcomes, now green.
- The pending-open IPC test now asserts the snake_case JSON by value. The push boundary test rejects `future_kind` and `generic` pushes. A camelCase held-tap file is discarded.

Checks: `cargo test -p alexandria-node --lib` for `notifications::`, `domain::`, `ai_chat::`, `commands::`, `db::`, `credentials::`, `profile::` and `settings::` (768 passed), plus `--test guardian_e2e`; `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `npx vue-tsc -b --noEmit`; Vitest over settings, pages, deeplink, i18n, utils and useNotifications (232 passed); `node --test scripts/check-tauri-commands.test.mjs`; `npm run check:tauri-commands`; `i18n:keys`, `i18n:parity` and `i18n:no-raw-text`; `npx playwright test tests/desktop-ui/notification-links.spec.ts` (4 passed); `scripts/check-docs.sh`.

Deviations:

- OS banner extras (`notificationId`, `profileId`, `networkId`) keep their names. They pass between the app and the notification plugin, not over IPC.
- The snapshot file and its test keep their `chat-vocab` names, but the notification lists render into their own generated file.
- The Notifications page keeps its fallback to generic copy for a kind the catalog lacks. That is defence in depth: the backend no longer sends unknown kinds.
- The mobile builds were not cross-compiled. The permission command uses `cfg!(mobile)`, so its code path compiles on desktop.

## Notes

- 2026-11-04: This ticket's commit broke `p2p::inbound::tests::classroom_notifications_follow_committed_membership_and_moderator_authority`. A bisect from the last release tip shows the commit is the first bad one, and the test is stable at its parent. Recipient routing was unchanged: each role still got the right number of rows. The test still read the classroom call row's payload as `callId`, which this ticket renamed to `call_id` on purpose, so it found no match. The ticket's checks covered `notifications::` but not `p2p::`. The fix updates the test to read `call_id`.
