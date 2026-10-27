# Complete missed-event recovery and inbox behavior

Triage: done
Repos: alexandria, alexandria-cloud
Assignee: ticket04 implementation agent
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/02-cloud-delivery-contract.md

## Task

Consume the authenticated room discovery contract so running desktop and resumed mobile recover currently joinable room openings without push. Deduplicate push and pull consistently while permitting a genuinely new room opening. Preserve expiration, answered-item, recipient and authority filtering and independent list failure behavior. Verify every local producer and core inbox read/clear behavior through the agreed boundaries. Work primarily in discovery/store/producer modules; coordinate shared notification types with ticket 03 through the contract note.

Acceptance: missed/no-push recovery works for all Cloud-owned event kinds; stale/foreign/non-actionable events do not notify; clearing does not resurrect events; producer and inbox behavioral tests pass.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.

## Closing notes — 2026-10-27

Implemented authenticated discovery for all five Cloud event kinds, exact opening-scoped room event IDs, independent list failures, current recipient/expiry/answered filtering, and persistent seen-key deduplication across normalized push and pull input. Candidate notification routes resolve the saved trusted origin and re-fetch room authority before joining. The configured directory authorization remains enforced.

Both native tutoring managers own panel presence through the existing room tasks and profile admission lease. Publication uses opening/presence IDs and the previous-opening compare-and-swap guard; joining, 30-second heartbeats and own-presence withdrawal use their separate endpoints. Cancellation cleans up ambiguous publication with the original authority and IDs. A heartbeat conflict only permits fresh admission to the same opening kept live by another panelist. Session conflict reconciliation disposes a losing local room and joins the authoritative winner without replacing remaining panelists. Page unmount does not own the lease.

Auto-issued credentials and verified guardian links record notifications in their owning transactions and announce after commit. Behavioral tests cover gossip call/join producers, disabled recording, unread/read/clear behavior and non-resurrection. No database migration was required.

Validation: red-to-green slices covered room discovery, stale candidate joins, competing panel publication, presence cancellation/renewal, and transactional local producers. Final checks passed: 90 targeted Rust unit tests, three guardian lifecycle integration tests with real cryptography, 52 frontend tests, strict Vue/TypeScript checking, Rust formatting, the Tauri command registry check, and Rust 1.91 Clippy for the library and tests with warnings denied. Ticket 03 owns the actual push parser and recipient lifecycle integration; this ticket tests their shared normalized event contract without editing that implementation. Full native builds and device delivery validation remain with tickets 05/06.
