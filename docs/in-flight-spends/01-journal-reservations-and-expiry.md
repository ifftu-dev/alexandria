# Journal: reserved outpoints, expiry and replace-expired

Triage: done
Repos: alexandria
Part of: alexandria/docs/in-flight-spends-spec.md

## Task

Foundation for every other ticket. In the submission journal:

- Refuse to checkpoint signed bytes that have no validity upper bound. Update existing journal tests that sign transactions without one.
- A query returning the outpoints (inputs and collateral inputs, decoded from stored signed bytes) reserved at a given tip, per the spec's "Reservations ignore status" and "Expired transactions reserve nothing" decisions.
- Move the expiry margin here as the single constant.
- New terminal status `expired` (schema check constraint edited in place). Reconciliation takes the tip and expires only per the spec's rule; unknown tip or provider error never expires.
- A replace-expired operation that keeps the expired entry as history and checkpoints new bytes atomically before sending.
- One shared coin-selection helper that filters a UTxO list by the reserved set, and a distinct "wallet busy" error value for builders to return.
- Fake ledger extensions from the spec's Testing Decisions: mempool conflict rejection, lagging index with an explicit catch-up, and never-land.

Acceptance: journal-level tests for TTL refusal, reservation window across all statuses, restart survival, expiry (including unknown tip and a receipt inside the margin), and replace-expired history. Fake ledger behaviour covered. `cargo fmt --check`, `cargo clippy -- -D warnings` and the cardano test modules pass.

## Implementation workflow

Read the parent spec and the applicable AGENTS.md files. Use the TDD skill at the seam the user confirmed: feature ticks driven through the `Ledger` port against the in-memory fake ledger. Work on an isolated branch started from the integration branch; confirm ancestry before editing. Merge the integration tip before reporting. Return commits, red/green evidence, tests run and open blockers; the parent arranges the merge.

## Notes

### 2026-11-12: completed

Merged to the integration branch with the other tickets of this spec. Code review follow-ups and accepted deviations are recorded in the parent spec's Notes.
