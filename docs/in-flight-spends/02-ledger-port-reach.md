# Username anchoring and completion minting through the Ledger port

Triage: done
Repos: alexandria
Part of: alexandria/docs/in-flight-spends-spec.md

## Task

Behaviour-preserving refactor per the spec's "`Ledger` port reach" decision. Make the username anchoring tick and builder, and the completion mint builder, queue and recovery, generic over the `Ledger` port instead of taking the Blockfrost client. Update the background chain pass wiring. The port's surface does not change. The completion observer, relay registry and stake-pubkey registry stay on the Blockfrost client.

Acceptance: existing username, completion queue and completion recovery tests pass unchanged in intent. At least one tick-level test per feature now runs against the fake ledger. Transport-level tests that need the loopback HTTP fake chain keep using it. fmt, clippy and tests pass, including the iOS check with `--features tutoring-video-ios` if any gated code is touched.

## Implementation workflow

Read the parent spec and the applicable AGENTS.md files. Use the TDD skill at the seam the user confirmed: feature ticks driven through the `Ledger` port against the in-memory fake ledger. Work on an isolated branch started from the integration branch; confirm ancestry before editing. Merge the integration tip before reporting. Return commits, red/green evidence, tests run and open blockers; the parent arranges the merge.

## Notes

### 2026-11-12: completed

Merged to the integration branch with the other tickets of this spec. Code review follow-ups and accepted deviations are recorded in the parent spec's Notes.
