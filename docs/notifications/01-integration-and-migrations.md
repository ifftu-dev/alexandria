# Integrate preserved notifications and reconcile migrations

Triage: done
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md

## Task

Integrate app main with the preserved notifications implementation; Cloud main is already merged on its integration branch. Resolve all conflicts while retaining current main features and notification behavior. Allocate the notification migration after existing main migrations and safely handle the provisional version-18 notification database without losing data or skipping call-kind migration. Use TDD at the existing migration seam. Reconcile generated IPC declarations, capabilities, locale exports, dependency locks, and native project configuration. Do not alter the parallel architecture worktrees.

Acceptance: clean committed integration bases in both repositories; migration tests cover fresh/current-main/provisional notification data; focused notification tests and strict frontend checks pass. Record prerequisite baseline issues rather than masking them.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.


## Notes

### 2026-10-27 — integration audit

- Isolated implementer `t3/notifications-integration-migrations` began clean at `36245c59`, an exact descendant of the committed app integration base. Incorporated integration tip `a529e5cf`, including its eight locale-export fixes; both chat and notification namespaces remain exported in all nine locales.
- Cloud was inspected read-only: clean `feat/notifications-integration` at `da421dd`; `0043_push_notifications.sql` follows main's `0042_interview_integrity.sql` without a collision. No Cloud or architecture-worktree files changed.
- App migration 28 retains the provisional notification SQL unchanged. Main's call-kind migration 18 and migrations 19–27 remain intact. The upgrade bridge recognizes only the exact provisional history and moves its notification marker while applying main's missing migrations in one transaction.
- The IPC declaration/scope guard accounts for 470 registered commands (421 invoked, 49 intentionally allowlisted), and its seven regressions pass. Notification/mobile-push capabilities, registrations, dependency locks, top-bar chat/bell integration and locale key parity were audited.
- Restored main's App Attest entitlement, omitted by the preserved notification branch. Untracked the generated notification plugin `.tauri` API copy (identical to the locked Tauri 2.11.5 sources) and ignored native generated copies/build outputs. The iOS build regenerates that API successfully.
- Native-readiness follow-up for ticket 05/06: iOS `aps-environment` is absent, and CI's provisioning-capability list currently requests only associated domains. Deployment Firebase files, APNs configuration and actual-device delivery/tap acceptance remain unverified. No provisioning or deployment was performed.
- Baseline/toolchain distinction: workspace Clippy with warnings denied fails on local Rust 1.99's deprecation of `Atomic::fetch_update` in `crates/moq-media/src/publish.rs:545`. The same workspace/all-targets gate passes on CI's pinned Rust 1.91.0. No media code or lint suppression was added.

### 2026-10-27 — completed

- Red/green evidence at the approved migration seam: the historical version-18 fixture first failed with `migration history is not a supported prefix at version 18 (notifications)`; after the atomic bridge it passes with all notification fields/read state, deduplication (including cleared events), push registration and call-kind backfill retained. A second regression first failed with `accepted missing notifications`; storage validation now refuses incomplete provisional schemas without rewriting their history.
- All five added migration regressions pass, covering fresh and current-main databases, preserved provisional data, incomplete storage, encrypted close/reopen after an injected mid-upgrade failure and successful retry, and unknown/renamed/gapped histories. The schema inventory now includes the three notification tables and unread index.
- Final focused Rust results: `cargo test -p alexandria-node --lib db::` **69 passed**; `cargo test -p alexandria-node --lib notifications::` **14 passed**. Formatting and whitespace checks pass. Final `cargo +1.91.0 clippy --workspace --all-targets -- -D warnings` passes (separate build directory); the Rust 1.99 baseline limitation above remains recorded.
- Frontend: strict `npx vue-tsc -b --noEmit`, production `npm run build`, and **39** notification/deep-link tests pass. The coordinator separately reports the merged baseline's full frontend suite **423 passed / 63 files** and Cloud's actual-PostgreSQL push baseline **5 passed**, with evidence in `/tmp/alexandria-notifications-20261027-validation.md`.
- Native checks: Xcode 27 built an unsigned arm64 iOS simulator app with `tauri ios build --debug --target aarch64-sim --no-sign --ci`, including Firebase/plugin linkage and the absent-configuration fallback. Android NDK 26.3/JDK 21 built a debug APK with `JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home bash scripts/android-build.sh --debug --apk --ci`. A direct Tauri Android invocation initially failed to find the NDK/CMake toolchain; the repository wrapper supplied it and the retry passed. Existing Xcode group/linker and Gradle/JDK deprecation warnings remain. These are build checks, not actual-device push acceptance.
- Closed only this integration/migration ticket. Native APNs capability/provisioning, Firebase deployment configuration and device delivery/taps remain with tickets 05/06; the parent notification spec remains open. Documentation was assessed; only this authorized tracker entry was updated.
