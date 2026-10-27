# Bind receipt, taps, and registration lifecycle to profiles

Triage: done
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

## Completion evidence — 2026-10-27

Implemented registration-200 recipient bindings, migration 29 for durable staged registration and opt-out cleanup, minimal locked recipient lookup, matching-profile held taps, and exactly-once consumption. Existing profile leases guard backend operations; frontend continuations and router commits reject stale sessions. Native callbacks and local banner taps use the same recipient/profile routing boundaries, with no extra iOS push banner.

Red/green boundary tests cover two profiles sharing a token, interrupted PUT and offline DELETE across restart and origin changes, token rotation, unknown legacy recipients, locked pending-open IPC, real picker unlock precedence, and late native/frontend operations. After merging ticket04, an authenticated discovery/push integration test verifies both delivery orders, durable dedupe after clearing, distinct room-opening events, and retention of exact event identity and locally trusted origin in payloads and held taps.

Validation passed: 30 scoped Rust tests, 69 database tests, 9 mobile-push plugin tests, 84 frontend tests, strict TypeScript, command registry and locale parity, Rust 1.91 formatting and relevant all-target Clippy, and Swift syntax parsing. Integrated native builds/device validation remain with tickets05/06. Two new notification status strings use English placeholders pending ticket05 translation.
