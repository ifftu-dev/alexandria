# Completion minting: reservations and replacement after expiry

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/in-flight-spends-spec.md
Blocked by: alexandria/docs/in-flight-spends/01-journal-reservations-and-expiry.md, alexandria/docs/in-flight-spends/02-ledger-port-reach.md

## Task

Completion minting adopts ticket 01: the input and collateral filter reserved outpoints; "wallet busy" uses the existing reserved backoff. An expired mint is rebuilt from its preserved recovery context through replace-expired, with identical evidence and only inputs, collateral, validity and fee changing. The completion request status follows the replacement.

Acceptance: the spec's scenarios as completion queue and recovery tick tests: a completion mint then an issuer-paid anchor in one pass behind a lagging index, both accepted with different inputs (this test needs ticket 03's anchoring changes; if 03 has not merged, cover the completion side and leave a note); a double-spend-rejected mint expired and replaced with identical evidence; a replacement with different evidence refused. fmt, clippy, tests pass.

## Implementation workflow

Read the parent spec and the applicable AGENTS.md files. Use the TDD skill at the seam the user confirmed: feature ticks driven through the `Ledger` port against the in-memory fake ledger. Work on an isolated branch started from the integration branch; confirm ancestry before editing. Merge the integration tip before reporting. Return commits, red/green evidence, tests run and open blockers; the parent arranges the merge.
