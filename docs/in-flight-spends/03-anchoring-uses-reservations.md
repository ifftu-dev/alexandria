# Credential anchoring: reservations, wallet busy, journal expiry

Triage: done
Repos: alexandria
Part of: alexandria/docs/in-flight-spends-spec.md
Blocked by: alexandria/docs/in-flight-spends/01-journal-reservations-and-expiry.md

## Task

Credential anchoring adopts ticket 01:

- The issuer-paid input, sponsor-paid collateral and sponsor pool pick all filter reserved outpoints (sponsor pool filtering happens before the issuer-and-tip hash pick).
- "Wallet busy" is a wait: unbuilt-retry delay, no attempt counted, reason shown as the anchor status message.
- Replace the anchor queue's own tip comparison with the journal's `expired` status, keeping today's release behaviour.
- Keep the identical-rebuild guard.

Acceptance: the spec's anchoring scenarios (collateral avoids a pending outpoint, sponsor pool skips own in-flight spend, all reserved means wait, expired batch released through the journal counting one attempt) as anchor queue tick tests. fmt, clippy, tests pass.

## Implementation workflow

Read the parent spec and the applicable AGENTS.md files. Use the TDD skill at the seam the user confirmed: feature ticks driven through the `Ledger` port against the in-memory fake ledger. Work on an isolated branch started from the integration branch; confirm ancestry before editing. Merge the integration tip before reporting. Return commits, red/green evidence, tests run and open blockers; the parent arranges the merge.

## Notes

### 2026-11-12: completed

Merged to the integration branch with the other tickets of this spec. Code review follow-ups and accepted deviations are recorded in the parent spec's Notes.
