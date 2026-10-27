# Finish localization and native configuration behavior

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/03-profile-isolation-and-opt-out.md, alexandria/docs/notifications/04-discovery-and-inbox.md

## Task

Translate all notification locale resources and any new recipient/cleanup states into the eight non-English supported languages. Localize app-generated and Cloud provider generic copy using selected/registered locale with English fallback; retain a consistent app/Cloud vocabulary without personal data. Finish native configuration handling so builds without Firebase project files remain usable and report push unavailability. Inspect existing native patches for build compatibility, duplicate foreground banners and cold-start taps. Never invent or commit credentials, and do not provision/deploy services.

Acceptance: actual translations and key parity, app/Cloud payload contract checks, focused tests and available native builds pass; unavailable deployment/device prerequisites are explicitly recorded for final validation.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.
