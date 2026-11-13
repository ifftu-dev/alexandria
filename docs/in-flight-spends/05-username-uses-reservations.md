# Username anchoring: reservations and rebuild after expiry

Triage: done
Repos: alexandria
Part of: alexandria/docs/in-flight-spends-spec.md
Blocked by: alexandria/docs/in-flight-spends/01-journal-reservations-and-expiry.md, alexandria/docs/in-flight-spends/02-ledger-port-reach.md

## Task

Username anchoring adopts ticket 01: input selection filters reserved outpoints; "wallet busy" waits until the next pass. An expired batch returns its claims to pending; a next batch with the same claims, and so the same operation id, goes through replace-expired.

Acceptance: username anchoring tick tests for a reserved input being avoided, wallet busy, and an expired batch rebuilt with the expired bytes still readable as history. fmt, clippy, tests pass.

## Implementation workflow

Read the parent spec and the applicable AGENTS.md files. Use the TDD skill at the seam the user confirmed: feature ticks driven through the `Ledger` port against the in-memory fake ledger. Work on an isolated branch started from the integration branch; confirm ancestry before editing. Merge the integration tip before reporting. Return commits, red/green evidence, tests run and open blockers; the parent arranges the merge.

## Notes

### 2026-11-12: completed

Merged to the integration branch with the other tickets of this spec. Code review follow-ups and accepted deviations are recorded in the parent spec's Notes.
