# Typed module errors

Triage: done
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

Done: every module named in the task, credentials included, returns its own thiserror enum.

What changed:

- **Studio access denial.** `alexandria_studio::access::Refused` is now `AccessDenied`, a thiserror enum whose display is the reason. The `Studio(Error)` variant and both `From` impls are gone. Chat maps a denial through `ai_chat::vocab::refused` (code from `denial_code`), the tutor, Studio runs and the decision service through `access::run_error`, each at its call. A lesson in a course the reader may not read is `LessonNotFound`; Studio data the rule could not read is `Database`; a malformed identifier is `Invalid`. `ai_access::request_approval` takes the connection the caller read, so a missing connection is the caller's error.
- **Chat turn.** The dropped fenced-block outcome is `turn::DroppedBlock`, and `Outcome::refused` is `Outcome::dropped`.
- **Enrolment** returns `EnrollmentError`; **aggregation** returns `AggregationError` (database, cached-state JSON, scoring inputs, network profile), over a new `db::with_typed_transaction`; **chat operations** return `ai_chat::ops::OpError` (denial, chat store, Studio, goal gone, model id too long, unknown memory kind, assessment text in memory or a note); the chat `StoreError` is now thiserror; **notifications** (store, recording, channel, recipients, opens, push) return `NotificationError`. **Credentials** (issue, `Issuer`, status lists, lifecycle, store, bundle, auto) return `credentials::CredentialError`: a locked vault, a key that is not the DID's current one, a subject that is not a DID, a policy without a session, an unknown session, a broken policy bound (`PolicyViolation`, one variant per bound), a missing or mismatched superseded credential (`SupersessionRequires`), a full list, a list no host serves or another issuer owns, a payload over its limits, an unrecognised or unsupported bundle, a status change by anyone but the issuer (`NotIssuer(StatusChange)`) or to a credential that is not here, and the database, JSON, key registry, signing, HTTP and completion failures beneath. Commands keep `Result<T, String>` and convert with `.map_err(|e| e.to_string())`.

Red → green (each failed to build until its enum or variant existed):

- `alexandria-studio` `policy::the_tutor_and_chat_refuse_a_non_text_lesson_through_the_shared_rule` (adds the not-your-course and malformed-id cases) and `access::tests`.
- `enrollment::tests` (no verified document, already enrolled, invalid policy, unsupported version 1).
- `aggregation::store::tests::an_unreadable_cached_state_is_a_decoding_error`; `scoring_adversarial_tests::failed_recomputation_never_presents_a_stale_score` asserts `Database`.
- `ai_chat::ops::tests` (attach, the attempt table, listing models, `chat_operations_refuse_what_they_cannot_hold`).
- `notifications::store::tests::a_failed_write_is_a_database_error`; `push_tests` assert `OtherProfile`, `SessionChanged` and `Profile` where they asserted `is_err()`.
- `credentials::lifecycle::tests` (another issuer cannot revoke, suspend or reinstate: `NotIssuer` with the change; new `a_status_change_to_an_unknown_credential_names_it`), `credentials::issuer::tests` (`VaultLocked`, `NotCurrentKey`), `credentials::issue::tests` (each `PolicyViolation`, `PolicyWithoutSession`, `SubjectNotDid`, `Supersession(SameIssuer)`, `SupersededNotFound`; new `an_unknown_integrity_session_is_refused`), `credentials::bundle::tests` (`InvalidPayload`, `CredentialEntry`, `Unrecognised`, `UnsupportedBundleVersion`; new `the_bundle_verifier_refuses_what_is_not_a_bundle`), `credentials::auto::tests` (`Completion`, `ObservationMismatch`). The assessment and adaptive tests assert `NotCurrentKey` for a key that is not the learner's.

Checks for credentials: `cargo test -p alexandria-node --lib` (1738 passed, 4 ignored); `--test e2e_vc --test guardian_e2e` (43 passed); `cargo check -p alexandria --all-targets` (the CLI reads bundles); `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `npm run check:tauri-commands` and its tests; `scripts/check-docs.sh`; `cargo check -p alexandria-node --target aarch64-apple-ios --features tutoring-video-ios`.

Checks for the other modules: `cargo test -p alexandria-studio`; `cargo test -p alexandria-node --lib` for AI access, AI chat, Studio and decision commands, course notes, enrolment, aggregation, scoring, completion, assessment, adaptive, talent index, notifications, auto-issuance, guardian and inbound; `--test e2e_vc --test guardian_e2e` (43 passed); `cargo fmt --check`; `cargo clippy -p alexandria-node -p alexandria-studio --all-targets -- -D warnings`; `npm run check:tauri-commands` and its tests; `scripts/check-docs.sh`; `cargo check -p alexandria-node --target aarch64-apple-ios --features tutoring-video-ios`. `p2p::inbound::tests::classroom_notifications_follow_committed_membership_and_moderator_authority` failed then, and failed the same way before this ticket's first commit; it passes in the full credentials run, after the `call_id` test fix.

Text the webview sees: unchanged, except:

- A held notification open read while the vault is locked now says "vault is locked — unlock first", as push does.
- Suspending or reinstating another issuer's credential says "only the credential issuer may suspend it" (or "reinstate it"), and an unknown credential "credential <id> not found"; both said "credential <id> not found or caller is not its issuer".
- A bundle export that cannot sign a status list names the list. The bundle verifier, which no command calls, no longer prefixes payload errors with "parse bundle: ".

Deviations:

- The Cloud discovery pass keeps its string outcome: it is a shared future, so its error must be `Clone`.
- Profile admission and the database executor have string interfaces, so their failures, and a job's own, sit in `NotificationError::Profile`.
- `AccessDenied::Invalid` also covers Studio errors the lesson lookups and the attempt gate never produce.
- Credentials is one enum, not several: bundles and issuance share the database, signing and status-list failures. The status-list publication report and the auto-issuance report keep their per-item string lines, which the webview shows.
- Revocation still reports a missing credential and one without a status list as one case, `NotRevocable`.

## Follow-up — 2026-11-05

The Cloud discovery deviation is gone. `cloud_pull::discover` returns `Result<usize, DiscoveryError>`, where `DiscoveryError` is `Arc<NotificationError>`, so the shared pass that joined callers await stays `Clone`. `NotificationError` gains `AppStateUnavailable` and `DiscoveryTask`. `notifications_discover` converts to a string at the command boundary. The `discover` doc comment now says one pass runs per profile session, and passes for different sessions can run side by side.

Red → green: `cloud_pull::tests::a_pass_with_the_vault_locked_fails_with_the_typed_refusal` would not build against the string outcome. Now green: it asserts `NotificationError::VaultLocked`, and Cloud is not asked.

Text the webview sees: unchanged, except a poisoned discovery slot now says "poisoned lock: another task failed inside" (it said "discovery state poisoned").
