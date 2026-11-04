# Typed module errors

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/06-notification-domain-types-and-vocabulary.md, alexandria/docs/review-remediation/11-guardian-ai-requests-through-ai-access.md, alexandria/docs/review-remediation/12-credentials-signed-by-a-whole-issuer.md

## Task

- **Error enums.** The notifications, credentials, AI access, enrollment, aggregation and AI chat operation modules each expose a thiserror enum at their public interface. They may use anyhow internally. Commands keep returning `Result<T, String>` and convert at the boundary.
- **Studio access refusal.** Rewrite it on thiserror and rename it so the name says it is an access denial.
- **Chat turn outcome.** Rename the dropped fenced-block outcome so the name says what happened.
- **Conversion.** Remove the two-way conversion between access refusals and Studio errors. Callers map explicitly.

Covers spec stories 68–70.

## Acceptance

- Each module's tests assert refusal variants rather than error strings.
- Command error text seen by the webview is unchanged, or any change is listed.
- Clippy with `-D warnings` and the guard script pass.

## Completion evidence — 2026-11-04

Partial: every module except credentials is converted. The header stays `ready-for-agent` until credentials lands.

What changed:

- **Studio access denial.** `alexandria_studio::access::Refused` is now `AccessDenied`, a thiserror enum whose display is the reason. The `Studio(Error)` variant and both `From` impls are gone. Chat maps a denial through `ai_chat::vocab::refused` (code from `denial_code`), the tutor, Studio runs and the decision service through `access::run_error`, each at its call. A lesson in a course the reader may not read is `LessonNotFound`; Studio data the rule could not read is `Database`; a malformed identifier is `Invalid`. `ai_access::request_approval` takes the connection the caller read, so a missing connection is the caller's error.
- **Chat turn.** The dropped fenced-block outcome is `turn::DroppedBlock`, and `Outcome::refused` is `Outcome::dropped`.
- **Enrolment** returns `EnrollmentError`; **aggregation** returns `AggregationError` (database, cached-state JSON, scoring inputs, network profile), over a new `db::with_typed_transaction`; **chat operations** return `ai_chat::ops::OpError` (denial, chat store, Studio, goal gone, model id too long, unknown memory kind, assessment text in memory or a note); the chat `StoreError` is now thiserror; **notifications** (store, recording, channel, recipients, opens, push) return `NotificationError`. Commands keep `Result<T, String>` and convert with `.map_err(|e| e.to_string())`.

Red → green (each failed to build until its enum or variant existed):

- `alexandria-studio` `policy::the_tutor_and_chat_refuse_a_non_text_lesson_through_the_shared_rule` (adds the not-your-course and malformed-id cases) and `access::tests`.
- `enrollment::tests` (no verified document, already enrolled, invalid policy, unsupported version 1).
- `aggregation::store::tests::an_unreadable_cached_state_is_a_decoding_error`; `scoring_adversarial_tests::failed_recomputation_never_presents_a_stale_score` asserts `Database`.
- `ai_chat::ops::tests` (attach, the attempt table, listing models, `chat_operations_refuse_what_they_cannot_hold`).
- `notifications::store::tests::a_failed_write_is_a_database_error`; `push_tests` assert `OtherProfile`, `SessionChanged` and `Profile` where they asserted `is_err()`.

Checks: `cargo test -p alexandria-studio`; `cargo test -p alexandria-node --lib` for AI access, AI chat, Studio and decision commands, course notes, enrolment, aggregation, scoring, completion, assessment, adaptive, talent index, notifications, auto-issuance, guardian and inbound; `--test e2e_vc --test guardian_e2e` (43 passed); `cargo fmt --check`; `cargo clippy -p alexandria-node -p alexandria-studio --all-targets -- -D warnings`; `npm run check:tauri-commands` and its tests; `scripts/check-docs.sh`; `cargo check -p alexandria-node --target aarch64-apple-ios --features tutoring-video-ios`. `p2p::inbound::tests::classroom_notifications_follow_committed_membership_and_moderator_authority` fails, and failed the same way before this ticket's first commit.

Text the webview sees: unchanged, except that a held notification open read while the vault is locked now says "vault is locked — unlock first", as push does.

Deviations:

- The Cloud discovery pass keeps its string outcome: it is a shared future, so its error must be `Clone`.
- Profile admission and the database executor have string interfaces, so their failures, and a job's own, sit in `NotificationError::Profile`.
- `AccessDenied::Invalid` also covers Studio errors the lesson lookups and the attempt gate never produce.

Remaining: `credentials/*` (issue, issuer, lifecycle, status lists, store, bundle, auto) still returns `Result<_, String>`, and its about 40 callers and string-asserting tests are unconverted.
