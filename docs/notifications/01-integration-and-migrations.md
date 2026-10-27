# Integrate preserved notifications and reconcile migrations

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md

## Task

Integrate app main with the preserved notifications implementation; Cloud main is already merged on its integration branch. Resolve all conflicts while retaining current main features and notification behavior. Allocate the notification migration after existing main migrations and safely handle the provisional version-18 notification database without losing data or skipping call-kind migration. Use TDD at the existing migration seam. Reconcile generated IPC declarations, capabilities, locale exports, dependency locks, and native project configuration. Do not alter the parallel architecture worktrees.

Acceptance: clean committed integration bases in both repositories; migration tests cover fresh/current-main/provisional notification data; focused notification tests and strict frontend checks pass. Record prerequisite baseline issues rather than masking them.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.
