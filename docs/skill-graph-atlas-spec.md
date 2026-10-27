# Personal Skill Graph atlas

Triage: ready-for-agent
Repos: alexandria

## Source and scope

Implement the user-confirmed design in [skill-graph-visual-research.md](skill-graph-visual-research.md#confirmed-design-brief). The interview decisions override the exploratory HTML examples. This work covers the personal skills page, expanded modal, and sidebar only. Public instructor graphs, guided-learning-path restrictions, taxonomy storage, prerequisite relationships, and credential authority are unchanged.

Integration branch: `feat/skill-graph-atlas-integration`.
Starting commit: `d894a10badbf74975e9cc615e13e48e8c7f9c98b`.
Tracker: local Markdown; close tickets with `Triage: done` and a dated note. No PR is requested.

## Acceptance criteria

1. Named, stable subject-field regions preserve all existing skills and prerequisite relationships, including cross-field links. Unknown membership appears in an explicit Unassigned region. Field counts use distinct loaded skill IDs and communicate earned skills and learning opportunities.
2. Subject color, current earned Bloom number **and name**, and a separate earned checkmark are simultaneously visible. Bloom means the highest level evidenced by the current subject's active skill credentials, consistent with the existing credential helpers; it is never the derived 1–5 aggregation band or the skill's intrinsic category. Null subject identity must not show another holder's achievements. Invalid/missing earned-level data must not fabricate Remember. Uniform node sizes do not imply value or competence.
3. Every unearned skill is labeled Ready and remains selectable regardless of prerequisites. Skills whose prerequisites are already earned can receive stronger emphasis; other skills are not described as locked or inaccessible. The inspector labels the learning-outcome Bloom category separately. Opportunities include further learning for earned skills as well as new skills, without manufacturing a six-step advancement requirement.
4. The graph has field focus, search, visible emphasis/filter state, prerequisite/dependent navigation, zoom controls, and overview/reset. Search and selection retain surrounding/cross-field context; filtering must not make a missing prerequisite appear satisfied. Node selection stays in the graph. A distinct Open skill action navigates to existing skill details. Returning restores camera, selection, and relevant graph controls.
5. The inspector is a desktop side panel and a mobile bottom sheet with a collapsed state. Both offer the same information and actions. Core tasks have keyboard and tap-accessible controls, visible focus, adequate hit targets, and full readable labels outside the dense graph. Do not rely on color or hover alone.
6. The layout remains still when settled. Brief camera/context transitions and localized earned/level-improvement feedback respect reduced motion. Do not replay achievement feedback on initial load or every remount. Stop idle animation/redraw work. Theme tokens support light/dark appearance; layout supports long labels and narrow screens.
7. All three personal surfaces use consistent data and visual meanings. The compact sidebar makes fields/earned levels legible and expanding the graph explicit. Refresh after credential changes and navigation, including level-only changes. Reset profile-specific data/view state on lock or profile change and reject stale async responses.
8. Preserve existing browse, credentials, graph visibility, goals, and skill-detail functionality. No new backend commands or schema changes are necessary. Follow existing i18n conventions and strict Vue/TypeScript checks.

## Tickets and dependencies

- [01 — Shared graph model and profile-safe state](skill-graph-atlas/01-model-state.md): ready first.
- [02 — Interactive subject atlas and inspector](skill-graph-atlas/02-explorer.md): blocked by 01.
- [03 — Personal surfaces and end-to-end verification](skill-graph-atlas/03-personal-surfaces.md): blocked by 01 and 02.

## Approved public test boundaries

The user approved all three public test boundaries and review base `d894a10b` on 2026-10-27. This approval applies to implementer and fix agents; do not ask again for these seams. The user also authorized temporarily rebinding this T3 project to the app repository for isolated ticket worktrees, restoring the original parent workspace after cleanup.

- **Graph adapter API:** active credential-to-earned-Bloom mapping, Ready state, counts, field metadata, stable presentation, and preservation of prerequisite references. Does not test canvas/SVG pixels or backend credential verification.
- **Mounted graph component API:** user-visible search/select/inspect/open/reset interactions and reduced-motion behavior. Tests public props/events/rendered controls, not private renderer internals.
- **Shared graph-state API:** credential refresh, level-only updates, camera/selection restoration, profile reset, and stale-load rejection. Does not re-test backend acceptance policy.

## Verification and completion

Use the required TDD workflow at approved boundaries. Run focused tests per ticket, `vue-tsc -b --noEmit`, frontend tests, production build, applicable i18n checks, and desktop/mobile visual verification. Rust checks are required if Rust changes become necessary; the planned implementation is frontend only. Review the complete integration diff for both documented standards and the confirmed spec, then fix findings before closing work.

The implementation workflow authorizes maintaining this spec/ticket lifecycle. Assess other documentation after code changes and ask before editing it, following the standing repository instructions.

## Notes

- 2026-10-27: Created the ticket graph from the confirmed research brief. Existing unrelated untracked research is excluded from this work.
