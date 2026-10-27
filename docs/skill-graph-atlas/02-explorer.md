# Interactive subject atlas and inspector

Triage: done
Repos: alexandria
Part of: ../skill-graph-atlas-spec.md
Blocked by: 01-model-state.md

## Work

Build one reusable personal graph explorer over ticket 01's public API, covering acceptance criteria 1–6 and 8. It must be usable from the personal graph page and expanded modal. Review the confirmed design; the external mocks are inspiration only and contain superseded Locked labels.

- Named stable subject regions, simultaneous subject color/current earned Bloom/name/checkmark, Ready nodes, cross-field prerequisites, explicit selection and emphasis.
- Search, field focus, prerequisite/dependent navigation, zoom/reset and explicit Open skill event. Preserve view state through remount using the shared state boundary.
- Desktop side inspector and collapsible mobile bottom sheet; equivalent accessible controls/list for keyboard and touch. Include learning actions for earned and new skills through existing routes/events.
- Motion only for contextual navigation and new earned/level transitions; respect reduced motion, avoid initial/replayed celebrations, and stop idle work. Support light/dark and locale direction with existing theme/i18n conventions.
- Keep the public instructor SVG graph and guided-path behavior unchanged. Prefer existing dependencies; use a stable non-physics presentation when it better satisfies field layout without altering relationships.

## Acceptance and verification

Use TDD at the approved mounted graph component seam for meaningful user-visible interactions. Verify cross-field inspection, Ready selection despite unmet prerequisites, current earned level versus intrinsic category, explicit navigation, restoration, reduced motion, and responsive inspector controls. Run focused tests/typecheck and supply a reproducible visual fixture or existing fixture route suitable for parent desktop/mobile inspection.

## Completion

2026-10-27: `SkillAtlasExplorer.vue` accepts model, view, optional feedback, and reduced-motion props. It emits view updates, explicit open/learn actions, and feedback consumption. It keeps stable field coordinates and readable earned labels, supports keyboard/pointer navigation and a responsive inspector, and emphasizes Bloom matches in overview previews, list, and map. Camera clamping is shared across pointer and keyboard input. Mounted and browser regressions cover selection, context, motion, and restoration.
