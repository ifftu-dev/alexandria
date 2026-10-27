# Harden Cloud delivery and publish the shared payload contract

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/01-integration-and-migrations.md

## Task

Implement recipient-specific opaque binding, stable event identity (including distinct room openings), per-device delivery progress across mixed success/failure retries, and authenticated discoverability of currently joinable interview rooms. Use the existing signed registration HTTP surface, real PostgreSQL transactions/roles, and fake Sender tests. Preserve transactional enqueue for all five event kinds and bounded provider retry/expiry policy. Validate payload size/collapse identifiers and prohibit identifying content.

Write a concise shared wire-contract note outside the repo for the app implementers, identifying exact fields and room discovery response semantics. Record its location in the completion report. Keep localization additions compatible with the later localization ticket.

Acceptance: HTTP/database integration tests actually execute (no silently skipped suite), including foreign-DID access, transactional rollback, mixed multi-device outcomes, concurrent workers, and room lifecycle. Relevant Rust formatting/Clippy and unit tests pass.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.
