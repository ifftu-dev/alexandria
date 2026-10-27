# Interactive subject atlas and inspector

Triage: in-progress
Repos: alexandria
Assignee: skill-graph-explorer
Part of: alexandria/docs/skill-graph-atlas-spec.md
Blocked by: alexandria/docs/skill-graph-atlas/01-model-state.md

## Work

Build one reusable personal graph explorer over ticket 01's public API, covering acceptance criteria 1–6 and 8. It must be usable from the personal graph page and expanded modal. Review the confirmed design; the external mocks are inspiration only and contain superseded Locked labels.

- Named stable subject regions, simultaneous subject color/current earned Bloom/name/checkmark, Ready nodes, cross-field prerequisites, explicit selection and emphasis.
- Search, field focus, prerequisite/dependent navigation, zoom/reset and explicit Open skill event. Preserve view state through remount using the shared state boundary.
- Desktop side inspector and collapsible mobile bottom sheet; equivalent accessible controls/list for keyboard and touch. Include learning actions for earned and new skills through existing routes/events.
- Motion only for contextual navigation and new earned/level transitions; respect reduced motion, avoid initial/replayed celebrations, and stop idle work. Support light/dark and locale direction with existing theme/i18n conventions.
- Keep the public instructor SVG graph and guided-path behavior unchanged. Prefer existing dependencies; use a stable non-physics presentation when it better satisfies field layout without altering relationships.

## Acceptance and verification

Use TDD at the approved mounted graph component seam for meaningful user-visible interactions. Verify cross-field inspection, Ready selection despite unmet prerequisites, current earned level versus intrinsic category, explicit navigation, restoration, reduced motion, and responsive inspector controls. Run focused tests/typecheck and supply a reproducible visual fixture or existing fixture route suitable for parent desktop/mobile inspection.

## Notes

- 2026-10-27: Created; blocked by the model/state API.

- 2026-10-27: Claimed in isolated worktree `t3-36ffe649`, branch `t3/skill-graph-explorer`; confirmed HEAD and ancestry at integration `f989e3f0`. Approved mounted props/events seam and ticket 01 final API govern implementation.
