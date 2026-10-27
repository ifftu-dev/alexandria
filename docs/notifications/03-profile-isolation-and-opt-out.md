# Bind receipt, taps, and registration lifecycle to profiles

Triage: in-progress
Assignee: notification profile isolation implementer
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/02-cloud-delivery-contract.md

## Task

Implement the Cloud payload contract in app/native receipt and tap handling. Unknown or unresolved recipients must never be guessed into the active profile. Preserve recipient plus destination through cold start and locking; replay only after the matching profile unlocks. Guard whole async operations and frontend state updates against profile changes. Make unregister durable with pending cleanup, authenticated retries, accurate status, no token-refresh re-enable, and safe reconciliation when Cloud origin changes.

Use the existing IPC/event, profile, deep-link, and notification registration boundaries and the agreed behavioral tests. Update mirrored types, generated commands/policies, native adapters and notification Settings as needed.

Acceptance: two profiles sharing one token remain isolated across receipt/taps/lock/startup/rotation; offline opt-out survives restart and eventually unregisters; in-flight work cannot write or navigate into the next profile; targeted tests, strict frontend checks and Rust checks pass.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.
