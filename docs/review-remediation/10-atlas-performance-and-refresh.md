# Atlas: performance and refresh policy

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/09-atlas-accessibility-and-stable-fields.md

## Task

- **Refresh policy.** Refresh while an atlas surface is mounted and visible, and on profile change or credential/skill events. Route changes elsewhere no longer refresh it.
- **Indexing.** Index nodes and fields by ID once per model load.
- **Rendering.**
  - Panning updates a view transform without re-rendering the match list.
  - Window the match list.
  - Cull edges to the viewport, as nodes already are.
  - Compute overview field summaries once per model.
- **Scale fixture.** Add one to the desktop UI mocked backend: about 1,500 skills across every field, with several thousand edges.
- **Accepted additions.** Keep the catalog's "learning for a skill" filter and the Browse goal button's single-pending-goal protection. They are recorded as accepted in the parent spec; add tests if they have none.

Covers spec stories 54–56 and 58–59.

## Acceptance

- The atlas composable Vitest shows no refresh on unrelated route changes, and that stale loads are rejected.
- On Chromium, the scale fixture pans with no main-thread task over 50 ms.
- The skill-filter and goal-button behaviours are covered by tests.
