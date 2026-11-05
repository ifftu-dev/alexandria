# Press feedback and motion rollout step 2

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/07-motion-progress-slow-start-and-tokens.md

## Task

- **Shared press treatment.** Add one, built from the motion tokens, that controls opt into through a shared class or wrapper. Apply it to navigation items, tab bar items, list rows, search results, chips and icon buttons.
- **Rollout step 2.** Complete it through the shared policy, tokens and existing transition components (no new animation library):
  - persistent navigation selection indicators (sidebar, mobile tab bar, mode switcher)
  - search open/close and result entry
  - list enter/leave/move
  - home, goal and catalog card entry and press
  - goal add/remove confirmation
  - lesson step transitions in daily learning

Every transition honours Reduced and Off, keeps all information visible, and never delays input.

Covers spec stories 32–38 and 44.

## Acceptance

- The desktop UI suite covers press feedback on non-button controls and Reduced/Off behaviour on each step 2 surface, on Chromium and WebKit.
- Component Vitests cover the shared press treatment.
- There are no input delays: interactions stay immediate under the existing interaction tests.

## Completion evidence — 2026-11-04

What changed:

- **Shared press feedback.** `.pressable` in `main.css` scales a control inward over `--duration-fast`; in Reduced and Off it dims in place (no scale). The class owns the control's transition, so controls that opt in drop their own. Applied to sidebar items, section toggles and previews, mobile tab bar items, mode switcher segments, search results, catalog and goal-picker chips, Home hero, stat, goal and course cards and graph chips, catalog cards, lesson rows and skill tags, and the dialog close button. Cards set `--press-scale` closer to 1.
- **Selection indicators.** `.selectable` draws an indicator from `aria-current`, `aria-checked` or `aria-selected`, settling in over `--duration-base`. Used by the sidebar, the mobile tab bar (a pill behind the icon) and the mode switcher. Sidebar and tab bar items now expose `aria-current`; the sidebar section toggles have a name and `aria-expanded`.
- **Shared transitions.** `main.css` names `motion-fade`, `motion-pop`, `motion-rise` (entry only) and `motion-list` (enter, leave, move) for Vue `<Transition>`/`<TransitionGroup>`. Their from-states apply only in full motion, so Reduced and Off show and remove content at once with nothing hidden. A leaving element has `pointer-events: none`. `holdLeavingPlace` (useMotion) holds a leaving list item in place while the rest move.
- **Search.** The backdrop fades, the panel pops from its anchor, and results rise in as they arrive. Results that stop matching go at once, so typing never queues motion. Closing never blocks the page beneath.
- **Lists and cards.** Home's goal rail, continue and tutorial rows and course grid, the catalog grid and the goal cards enter, leave and move as `motion-list`. Catalog filter chips expose `aria-pressed`.
- **Goal confirmation.** Removing a goal shows "Removed … from your goals." in a status line. Adding one shows "… is now one of your goals." at the top of the dialog's next step. Both are translated by hand into all nine locales. The Goals spinner covers only the first load, so refreshes keep the cards on screen.
- **Lesson steps.** The next lesson arrives from the end of the reading direction and the previous one from its start, mirrored in right-to-left locales. It runs through `animateMotion` over `--duration-base`, replacing the lesson body's fixed vertical keyframes.

Red → green:

- `pressFeedback.test.ts` (Vitest): tab bar items and mode switcher segments carry the shared press and selection classes, and selection follows `aria-current` / `aria-checked`. Red against the previous components, then green.
- `motion-surfaces.spec.ts` (desktop UI, Chromium and WebKit, new `motion-surfaces` fixture over one unlocked profile). Each check runs in System, Reduced and Off:
  - press feedback on non-button controls: sidebar item, mode switcher segment, search result, Home goal card, catalog chip and card, lesson row, dialog close icon button. Red (no scale), then green.
  - navigation selection indicators in the sidebar, mode switcher and mobile tab bar. Red (no `aria-current`), then green.
  - search open, result entry and close without blocking the next pointer. Red (no result entry; close blocked the page), then green.
  - Home and catalog cards enter, leave and move. Red (no entry motion), then green.
  - goal cards enter, and removal and addition confirm themselves. Red (no confirmation), then green.
  - lesson steps follow the reading direction, including a right-to-left locale. Red (vertical, direction-blind), then green.
  - the OS reduce preference swaps press scaling and list motion for immediate changes.
- In Reduced and Off every check asserts no motion longer than the 0.01 ms floor and the arriving content at full opacity.

Checks: full Vitest suite (89 files, 566 tests passed); full desktop UI suite (108 passed, 2 skipped by existing browser-specific guards); `npx vue-tsc -b --noEmit`; `npm run check:csp`; `npm run i18n:keys`; `npm run i18n:parity`; `npm run i18n:no-raw-text`; `scripts/check-docs.sh`.

Deviations:

- Reduced uses immediate changes, not short cross-fades: the global rule from ticket 07 already holds every transition at 0.01 ms in Reduced, and the shared transitions apply their from-states only in full motion.
- The notifications inbox list was not converted; Home, catalog and goals are the primary list surfaces here.
- A goal's addition is confirmed in the dialog, where the learner is, rather than also on the page, so the same feedback is not shown twice.

