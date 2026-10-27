# Personal Skill Graph atlas

Triage: done
Repos: alexandria

## Scope

Implement the [confirmed design](skill-graph-visual-research.md#confirmed-design-brief) across the personal skills page, expanded modal, and sidebar. Public instructor graphs, guided paths, taxonomy storage, prerequisite relationships, and credential authority are unchanged.

## Acceptance criteria

1. Named, stable subject-field regions preserve all existing skills and prerequisite relationships, including cross-field links. Unknown membership appears in an explicit Unassigned region. Field counts use distinct loaded skill IDs and communicate earned skills and learning opportunities.
2. Subject color, current earned Bloom number **and name**, and a separate earned checkmark are simultaneously visible. Bloom means the highest level evidenced by the current subject's active skill credentials, consistent with the existing credential helpers; it is never the derived 1–5 aggregation band or the skill's intrinsic category. Null subject identity must not show another holder's achievements. Invalid/missing earned-level data must not fabricate Remember. Uniform node sizes do not imply value or competence.
3. Every unearned skill is labeled Ready and remains selectable regardless of prerequisites. Skills whose prerequisites are already earned can receive stronger emphasis; other skills are not described as locked or inaccessible. The inspector labels the learning-outcome Bloom category separately. Opportunities include further learning for earned skills as well as new skills, without manufacturing a six-step advancement requirement.
4. The graph has field focus, search, visible emphasis/filter state, prerequisite/dependent navigation, zoom controls, and overview/reset. Search and selection retain surrounding/cross-field context; filtering must not make a missing prerequisite appear satisfied. Node selection stays in the graph. A distinct Open skill action navigates to existing skill details. Returning restores camera, selection, and relevant graph controls.
5. The inspector is a desktop side panel and a mobile bottom sheet with a collapsed state. Both offer the same information and actions. Core tasks have keyboard and tap-accessible controls, visible focus, adequate hit targets, and full readable labels outside the dense graph. Do not rely on color or hover alone.
6. The layout remains still when settled. Brief camera/context transitions and localized earned/level-improvement feedback respect reduced motion. Do not replay achievement feedback on initial load or every remount. Stop idle animation/redraw work. Theme tokens support light/dark appearance; layout supports long labels and narrow screens.
7. All three personal surfaces use consistent data and visual meanings. The compact sidebar makes fields/earned levels legible and expanding the graph explicit. Refresh after credential changes and navigation, including level-only changes. Reset profile-specific data/view state on lock or profile change and reject stale async responses.
8. Preserve existing browse, credentials, graph visibility, goals, and skill-detail functionality. No new backend commands or schema changes are necessary. Follow existing i18n conventions and strict Vue/TypeScript checks.

## Completed tickets

- [Shared model and profile-safe state](skill-graph-atlas/01-model-state.md)
- [Interactive explorer and inspector](skill-graph-atlas/02-explorer.md)
- [Personal surfaces](skill-graph-atlas/03-personal-surfaces.md)

## Verification

Completed 2026-10-27. Approved test boundaries cover the data adapter, mounted graph interactions, and shared refresh/profile/view state. Independent standards and spec reviews found five issues; all were resolved, including actual learning destinations for earned skills and visible overview Bloom emphasis.

441 frontend tests and 45 Chromium/WebKit checks pass, with one WebKit native-touch-synthesis skip. Strict typechecking, production build, i18n checks, and the command guard pass. Native WebView validation and native-speaker review of new copy remain outstanding. No Rust or backend behavior changed.

Reproduce browser checks with `npm run test:desktop-ui`. The production-surface fixture is at `tests/desktop-ui/?screen=personal-atlas`; add `theme=dark`, `scenario=stress`, or `locale=ur` to exercise presentation variants.
