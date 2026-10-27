# Validate the integrated spec and close only evidenced work

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/05-localization-and-native-readiness.md

## Task

Run required Rust formatting/Clippy, strict frontend type checking, focused and relevant broader tests, real Cloud PostgreSQL integration tests, and available native builds. Invoke the code-review skill on the integration branches, using separate standards and spec review subagents; route fixes through one isolated TDD implementer and merge via merger subagent. Recheck affected behavior after fixes.

Perform agreed real-device iOS/Android and supported-desktop presentation/tap tests when actual devices/configuration are available. If not available, leave this ticket and parent spec open with precise evidence and prerequisites; do not claim native readiness or turn missing validation into a pass. Do not provision credentials or deploy. Close other fully completed tickets with Triage: done and dated notes. Clean up only task-created implementer worktrees after their commits are safely merged, preserving integration checkouts and original work. Report branches, SHAs, validation and remaining external requirements. No PR is required by this Markdown tracker workflow.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.
