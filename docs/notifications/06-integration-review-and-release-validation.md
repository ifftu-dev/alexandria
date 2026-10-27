# Validate the integrated spec and close only evidenced work

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/05-localization-and-native-readiness.md

## Task

Run required Rust formatting/Clippy, strict frontend type checking, focused and relevant broader tests, real Cloud PostgreSQL integration tests, and available native builds. Invoke the code-review skill on the integration branches, using separate standards and spec review subagents; route fixes through one isolated TDD implementer and merge via merger subagent. Recheck affected behavior after fixes.

Perform agreed real-device iOS/Android and supported-desktop presentation/tap tests when actual devices/configuration are available. If not available, leave this ticket and parent spec open with precise evidence and prerequisites; do not claim native readiness or turn missing validation into a pass. Do not provision credentials or deploy. Close other fully completed tickets with Triage: done and dated notes. Clean up only task-created implementer worktrees after their commits are safely merged, preserving integration checkouts and original work. Report branches, SHAs, validation and remaining external requirements. No PR is required by this Markdown tracker workflow.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.

## App final review corrections — 2026-10-27

Implemented the seven app findings: encrypted profile-bound held destinations with opaque replay metadata, vendored modification notices, shared desktop/mobile presence admission, versioned Cloud mutation acknowledgements with durable retry identity, retained Android cold actions, race-safe inbox initialization, and removal of iOS token-prefix diagnostics. The coordinated Cloud protocol prevents an old PUT from undoing an acknowledged DELETE; the app preserves cleanup debt when acknowledgement is absent or mismatched. Incompatible prelaunch plaintext pending-open state is discarded; the encrypted format preserves matching-profile taps and consumption across restart.

Validation passed: 38 notification-related Rust tests, 69 database tests, 19 tutoring tests, 78 profile-related tests, nine push-plugin tests, all 467 frontend tests, strict TypeScript, command registry, formatting and Rust 1.91 all-target app/plugin Clippy. Actual Android native command tests passed. Integrated Android arm64 debug APK/AAB and iOS arm64 simulator builds passed without Firebase deployment files. Android packaging initially ran out of disk space and passed after relocating generated output; no source workaround was needed.

This records the first final-fix candidate; the coordinator completed independent review, separate merge and the full-workspace check as recorded below. This ticket and the parent spec remain open for configured real-device delivery, foreground/background/terminated taps, permission handling, token rotation, offline opt-out and production signing validation. No credentials were provisioned and no service deployed.

## Final validation disposition

### 2026-10-27 — implementation and local validation complete; release acceptance open

Tickets 01–05 are complete. Final app and Cloud standards/spec reviews have no remaining actionable findings after isolated TDD fixes and independent merger review. The final fixes protect held notification activity with profile-bound encryption, order registration mutations across cancellation/retry, retain native cold taps, close listener/snapshot races, share room admission behavior, and remove token-derived logs.

Local evidence: the app workspace passed 2,158 tests with zero failures and nine existing ignored tests on the first final-fix candidate; the subsequent Rust-only tap-handoff change passed all ten plugin tests, 38 notification tests and 60 frontend routing/inbox/profile tests. The full frontend suite passed 467 tests. Strict TypeScript, command registry, locale/copy parity, formatting and CI-pinned Rust 1.91 Clippy passed. Android APK/AAB and iOS simulator builds passed on the preceding candidate; the final handoff change has no native-interface change. Cloud passed all 427 tests, including 23 real-PostgreSQL push tests under the ordinary parallel runner, with zero ignored tests; Rust 1.91 and 1.99 all-target Clippy, formatting and vendor checks passed. Independent mergers rechecked focused suites. An earlier unrelated peer-fetch timeout passed isolated and suite reruns; interrupted and disk-full native build attempts are excluded from successful evidence.

Rollout requirement: every Cloud registration writer must enforce the durable `versioned-v1` mutation barrier. PUT and DELETE send a positive canonical `x-alexandria-push-version`; successful responses echo that version plus `x-alexandria-push-protocol: versioned-v1`. Permanent DID/device tombstones reject stale mutations, including legacy delayed PUT after DELETE acknowledgement. App cleanup stays pending without the exact acknowledgement, preserves the original body/version for ambiguous retries, and allocates a fresh device UUID after acknowledged deletion. Old direct-writing server binaries can bypass the barrier and must not coexist as writers. Already provider-queued notifications cannot be recalled.

Privacy compatibility: incompatible prelaunch plaintext held-open sidecars are discarded, including their consumed-tap history. Prototype-held taps are not migrated. Profile databases, inbox rows, read state and durable inbox deduplication are retained; new encrypted held taps survive restart and matching unlock.

Remaining acceptance requires deployment-specific Firebase app files, configured Firebase/APNs and Cloud sender credentials, a push-enabled Apple App ID with matching signing/provisioning, and connected real iOS/Android devices. Record OS/build versions and results for permission grant/denial, token acquisition/rotation, foreground/background/terminated delivery, taps/cold start, multi-profile isolation and offline opt-out. Supported desktop focused/unfocused presentation and taps also remain unverified. Translations are machine-drafted, not native-speaker-certified. No credentials were provisioned, services deployed, or production delivery claimed. This ticket and the parent spec remain open solely for those external release checks.

Reviewed app source was integrated at `37c22800c8d3d7a044956531f73ca301750689d5`; Cloud integration is `cd9fa4367b67107ba5842e21591d629070cafcd3`. Final app merger independently reran ten plugin, 38 notification and 60 frontend tests plus formatting, command registry and commit-subject checks. The final merge delta has no whitespace errors; ten pre-existing vendored full-base whitespace warnings remain. App Clippy uses CI-pinned Rust 1.91; latest-toolchain app Clippy still encounters the existing vendored media deprecation. Cloud passes both pinned and latest toolchains.
