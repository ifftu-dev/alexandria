# Complete missed-event recovery and inbox behavior

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/02-cloud-delivery-contract.md

## Task

Consume the authenticated room discovery contract so running desktop and resumed mobile recover currently joinable room openings without push. Deduplicate push and pull consistently while permitting a genuinely new room opening. Preserve expiration, answered-item, recipient and authority filtering and independent list failure behavior. Verify every local producer and core inbox read/clear behavior through the agreed boundaries. Work primarily in discovery/store/producer modules; coordinate shared notification types with ticket 03 through the contract note.

Acceptance: missed/no-push recovery works for all Cloud-owned event kinds; stale/foreign/non-actionable events do not notify; clearing does not resurrect events; producer and inbox behavioral tests pass.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.
