# Interactive subject atlas and inspector

Triage: done
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

- 2026-10-27: Completed reusable explorer, deterministic field layout, mounted interaction tests, nine locale catalogs/keys, and the production-component browser fixture. Ancestry confirmed before edits at `f989e3f0`; latest `feat/skill-graph-atlas` remains that commit and is already contained. Personal page/modal/sidebar wiring belongs to ticket 03. No public instructor graph, guided path, backend, taxonomy, or legacy helper changes.

## API and integration handoff

Import `src/components/skills/SkillAtlasExplorer.vue`. Props: `model: AtlasModel`, `view: AtlasViewState`, optional `feedback: AtlasFeedback | null`, and optional `reducedMotion: boolean`. The component reads ticket 01 types without importing its runtime singleton or performing IPC/routing.

- `update:view` emits a complete view snapshot. Bind the appropriate `pageView`/`modalView`, forwarding to `atlas.updateView(surface, next)`. Selection/search/field/Bloom/neighborhood/depth/camera/collapse survive remount through this boundary. Camera coordinates represent world center; explorer controls use zoom 1–1.8 so earned names stay readable. Null camera with no focused field selects the field overview.
- `open-skill(id)` is the only detail-navigation request. Selection never routes. `learn-skill(id)` requests the existing learning/goal action for both earned and Ready skills; the owner implements the route/goal workflow.
- `feedback-consumed(revision)` forwards to `atlas.acknowledgeFeedback`. The owner must register/release `atlas.activateSurface(surface)` while visible, as ticket 01 specifies. Mounted stale feedback is consumed silently; new feedback has an 800 ms one-shot visual highlight and 1500 ms announcement lifetime, honors system/prop reduced motion, and is cleared on hiding/unmount. No animation frame or idle polling loop exists.
- The desktop inspector becomes a sticky, collapsible bottom sheet at 700 px. Native controls, arrow-key map panning, Escape to leave relationship emphasis, full-label list, and explicit relation buttons provide keyboard/tap equivalents. Overview reveals field counts and earned previews; fixed-coordinate detail shows actual directed edges, including cross-field connections. Unloaded endpoints remain named in the inspector. Layout is independent of achievements and filters.

## Reproduction and verification

From this checkout:

```sh
npm run dev -- --host 127.0.0.1 --port 5197 --strictPort
# http://127.0.0.1:5197/tests/desktop-ui/?screen=skill-atlas&theme=dark
DESKTOP_UI_PORT=5197 npx playwright test tests/desktop-ui/skill-atlas.spec.ts --workers=2
```

The existing desktop-ui workflow now accepts optional `DESKTOP_UI_PORT` (default remains 5184), allowing isolated checkouts to avoid server collisions. Fixture query options: `theme=light|dark`, `locale=ur` (or any supported locale), `scenario=stress|empty-earned`, `motion=reduced`. The base fixture uses all 103 bootstrap skills, 8 fields, and 89 real prerequisite records through `buildSkillAtlas`, with illustrative current earned ranks. Stress adds one synthetic unassigned long bilingual label, malformed claim, and unloaded prerequisite without altering bootstrap data. Fixture buttons simulate evidence updates/remounts and display emitted open/learn requests.

- Observed mounted red → green for Ready selection/explicit actions, earned versus intrinsic Bloom and malformed claims, search/cross-field inspection/reversible focus, bounded feedback, keyboard Escape, and duplicate-feedback suppression. Seven mounted tests also cover view restoration and hide/unmount cleanup. Tests use public props/events/controls; no private layout tests or renderer mocks.
- `npx vitest run`: **431 tests / 64 files passed**, including legacy and model/state tests.
- `DESKTOP_UI_PORT=5197 npx playwright test --workers=2`: **30 passed** across Chromium/WebKit (10 new atlas cases). Light/dark, keyboard, remount, mobile collapse, long labels, Urdu viewport bounds, and actual zero-animation behavior under reduced motion passed; screenshots inspected.
- Strict `npx vue-tsc -b --noEmit`, `npm run build`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, and `git diff --check` passed. Build retains existing large-chunk warnings; parity reports existing unrelated pending translations, with all 53 atlas keys present in all nine catalogs.

## Integration limitations

- This is a component/fixture delivery, not completed personal-surface wiring or native WebView verification. Native drag/pinch is not implemented; pan/zoom buttons, arrow keys, field controls, and search/list selection cover navigation.
- Browser inspection exposed an existing unrelated RTL CSS leak in `src/components/settings/LocaleDropdown.vue:179`: `:global([dir='rtl']) .locale-dd__menu` compiles to `[dir="rtl"] { transform: translateX(50%); }`, shifting the whole document after that stylesheet loads. The atlas fixture loads only its own component, so it verifies the explorer independently. Ticket 03 should address or separately track that app-level RTL issue; this ticket leaves the existing dropdown untouched.
- New non-English translations follow the repository's machine-drafted locale convention and still need native-speaker review.

## Accepted review fixes — 2026-10-27

- Spec P2(2): Bloom emphasis now visibly outlines matching earned previews and full-label list entries as well as map nodes. Matching earned skills move into the capped overview preview; loaded/earned/Ready counts remain unchanged. A localized active summary counts matches in the search-result list, including zero. All other skills, Ready actions, prerequisite navigation, and earned number/name/checkmarks remain available.
- Standards finding 3: button/keyboard pan and pointer drag share computed world bounds and one camera movement/clamping function; their input handling remains separate. Standards finding 2 uses the shared pure view-state factory for Reset.
- Mounted red → green reproduced the inert overview/list before implementation. Additional coverage checks capped previews, search counts, zero matches, intrinsic versus earned Bloom, and Ready/prerequisite context. Browser checks verify actual outline styles in both themes and engines. Final validation: 36 focused tests, 441 frontend tests, strict typecheck/build/i18n checks, and 45 browser checks pass; one existing WebKit native-touch-synthesis skip remains.
- Browser evidence: `/Users/hack/Documents/Personal/Code/alexandria-mark3/work/skill-graph-implementation/review-fixes-v1/browser-verified/`. Run the existing fixture/test commands with `DESKTOP_UI_PORT=5199` for this review-fix checkout.
