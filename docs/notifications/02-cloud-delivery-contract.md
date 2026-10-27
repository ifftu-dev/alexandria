# Harden Cloud delivery and publish the shared payload contract

Triage: done
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/01-integration-and-migrations.md

## Task

Implement recipient-specific opaque binding, stable event identity (including distinct room openings), per-device delivery progress across mixed success/failure retries, and authenticated discoverability of currently joinable interview rooms. Use the existing signed registration HTTP surface, real PostgreSQL transactions/roles, and fake Sender tests. Preserve transactional enqueue for all five event kinds and bounded provider retry/expiry policy. Validate payload size/collapse identifiers and prohibit identifying content.

Write a concise shared wire-contract note outside the repo for the app implementers, identifying exact fields and room discovery response semantics. Record its location in the completion report. Keep localization additions compatible with the later localization ticket.

Acceptance: HTTP/database integration tests actually execute (no silently skipped suite), including foreign-DID access, transactional rollback, mixed multi-device outcomes, concurrent workers, and room lifecycle. Relevant Rust formatting/Clippy and unit tests pass.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.


## Notes

### 2026-10-27 — completed

Cloud implementation and separate merger completed at `e9c6594eb0892f994bba303dbec6dd908e53029e` on Cloud `feat/notifications-integration`. Registration returns a server-generated opaque recipient UUID; payloads carry stable event identity; durable per-registration outcomes prevent resending terminal devices on ordinary retries. Interview-owned opening/presence state supplies authenticated live-room discovery, guarded replacement and own-presence leave with a bounded 90-second liveness window. Existing business authority and transactional enqueue remain intact.

Validation: 10 real-PostgreSQL push integration tests (independently rerun by merger), 31 related database tests and 247 library tests pass. Formatting and CI-pinned Rust 1.91 all-target Clippy pass. Rust 1.99 full Clippy has a pre-existing redundant borrow in `tests/idempotency.rs:444`; changed targets pass. No skipped database suite is counted as a pass.

The settled cross-repository implementation contract is `/tmp/alexandria-notifications-wire-contract.md`; implementation evidence is `/tmp/alexandria-notifications-cloud02-validation.md`, independent merger evidence `/tmp/alexandria-notifications-cloud02-merger.md`. App duties remain in tickets03/04: persist registration bindings and cleanup intent, enforce recipient/session isolation, consume discovery identities, and maintain presence from connected panel room runtimes. Cloud generic translations remain in ticket05. Provider collapse/crash redelivery and offline opt-out limitations are explicit; no device delivery or release readiness is claimed.
