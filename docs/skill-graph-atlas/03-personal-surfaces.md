# Integrate personal graph page, modal, and sidebar

Triage: done
Repos: alexandria
Part of: alexandria/docs/skill-graph-atlas-spec.md
Blocked by: alexandria/docs/skill-graph-atlas/01-model-state.md, alexandria/docs/skill-graph-atlas/02-explorer.md

## Work

Wire the shared data state and explorer into all personal surfaces, covering acceptance criteria 4–8.

- Replace the personal page and modal's divergent graph implementations with the shared explorer. Keep browse filters separate from graph controls so hidden filters cannot silently constrain the graph.
- Give the sidebar a matching compact presentation with identifiable fields, earned Bloom meanings, and explicit expand. Remove Locked wording only from the redesigned personal graph surfaces; do not alter the guided path or public instructor graph.
- Ensure graph selection/camera and the graph tab restore after Open skill and return; profile lock clears personal state. Preserve browse, credentials, goals, graph visibility, and existing detail routes.
- Integrate credential refresh without duplicate event listeners/requests or stale level displays. Reuse shared state and retain existing tests' meaningful refresh behavior.
- Maintain locale key parity and generated key declarations; avoid new raw interface strings. Remove obsolete personal-graph code only when no remaining consumer needs it.

## Acceptance and verification

Use TDD at the approved mounted-component and shared-state boundaries. Run focused/full frontend tests, strict typecheck, production build, and applicable i18n checks. Verify all three surfaces with the full bootstrap fixture, long labels/unassigned membership, both themes, reduced motion, and desktop/mobile sizing. Assess documentation needs, but do not modify unrelated documentation without permission. Report head SHA, validation, visual fixture access, and limitations for integration review.

## Notes

- 2026-10-27: Created; blocked by shared state and explorer.

- 2026-10-27: Completed in isolated worktree `/Users/hack/.t3/worktrees/skill-graph-atlas-integration/t3-28c682d8`, branch `t3/personal-surfaces-integration`. Starting HEAD exactly matched integration `609b982025948daecd4adecc9c921664065e3f14`; approved review base `d894a10b` is an ancestor. Used the preapproved mounted component, shared state, and public camera seams with real profile lifecycle, memory router, settings/goal workflow, and strict external IPC stubs. No subagents, PR, push, integration merge, or project binding change.

## Implementation and API handoff

- `SkillAtlasHost.vue` is installed once by `App.vue`. It owns the reference-counted event/navigation bridge and the expanded modal independently of sidebar visibility. Startup waits for profile initialization; entry loads coalesce through shared state, profile changes clear both views, and unmount releases subscriptions and modal scroll/focus ownership.
- The page and modal use `SkillAtlasExplorer`; the sidebar uses the same model and shared field palette for legible field counts and earned number/name/checkmarks, plus explicit expansion. All unearned personal skills are Ready. Browse field/subject/search controls remain separate; credentials and graph visibility/teaching controls remain usable. The visibility editor follows the explorer. The obsolete, now-unreferenced personal force-graph adapter and hover renderer were removed. Public instructor graphs and guided paths are unchanged; the package/CSP setup for the former renderer was not changed.
- `useSkillAtlas().openSkill(router, surface, id)` records one profile-owned return and navigates to the existing skill detail. `returnToAtlas(router)` handles the detail page's existing Back action. Browser history Back or that explicit action restores the originating modal and its selection/camera/controls once; unrelated navigation, another Open action, lock/reset, and final bridge disposal discard the return. Page tab/view restoration also works through ordinary navigation back to `/skills`.
- `useSkillAtlasActions(surface)` handles Open and Find learning opportunities. Learning uses the existing settings-backed goal action for earned and Ready skills, initializes settings when entering through the sidebar, rejects stale session continuations, avoids duplicate submissions, and exposes failures without closing the modal. Successful learning opens `/goals`.
- Mouse/pen dragging moves the camera with a movement threshold and suppresses click selection after a drag. Touch horizontal swipes pan; vertical swipes retain normal page scrolling and native browser pinch zoom remains available. Buttons and arrow keys still provide all camera navigation. This deliberately does not implement custom graph pinch zoom.
- The integration-blocking `LocaleDropdown` selector now scopes its RTL transform to the dropdown menu. The browser regression first observed the document shifted by 195 px at 390 px width, then verified zero document transform and in-viewport content after the fix.
- Updated map navigation guidance in all nine catalogs and regenerated keys (no key-set change). Assessed README and other docs: no broader documentation edits are needed for this bounded UI change; only this ticket and the spec lifecycle index were updated.

## Verification and reproduction

- Observed red → green for named sidebar fields/current earned Bloom and level-only refresh, independent page browse/graph state and real detail return, accessible expanded modal/one-shot return/scroll cleanup, pointer camera movement versus node clicks, goal-save failure handling, and the RTL document-transform regression.
- `npx vitest run`: **438 tests in 65 files passed**. Coverage includes shared refresh refcount/teardown, profile lock and stale responses, page/modal restoration, earned and Ready learning goals, goal-save retry, and visibility/teaching settings.
- `npx vue-tsc -b --noEmit`: passed. `npm run build`: passed; only existing large-chunk warnings remain.
- `npm run i18n:keys`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, and `git diff --check`: passed. All nine catalogs have 3539 keys; parity reports the existing unrelated 437 pending translations.
- `DESKTOP_UI_PORT=5198 npx playwright test --workers=2`: **41 passed, 1 skipped** across Chromium/WebKit. The skip is Chromium-CDP native touch synthesis under WebKit; responsive layout, taps/controls, focus/scroll handling, navigation, and RTL still run in both engines. Chromium native touch confirms sideways panning and normal vertical page scrolling.
- Browser checks cover full-taxonomy page/modal/sidebar, real detail routes and history return, level-only refresh, profile lock/unlock, light/dark, 390 px mobile, long bilingual labels, Unassigned/malformed evidence/unloaded prerequisites, reduced motion, mouse drag, wheel scrolling, and Urdu RTL with the actual locale dropdown CSS loaded. Inspected light overview, dark modal, mobile long-label modal, and Urdu screenshots.

Start the reusable surface fixture from this checkout:

```sh
npm run dev -- --host 127.0.0.1 --port 5198 --strictPort
# http://127.0.0.1:5198/tests/desktop-ui/?screen=personal-atlas&theme=dark
# Add &scenario=stress, &locale=ur, or use theme=light.
DESKTOP_UI_PORT=5198 npx playwright test tests/desktop-ui/personal-atlas.spec.ts --workers=2
```

`PersonalAtlasFixture.vue` mounts production page, sidebar, modal host, detail, goals, and locale dropdown. It uses all 103 bootstrap skills, eight fields, and 89 prerequisite records, a real profile lifecycle and memory router, and shared strict IPC fixtures. Fixture controls simulate a level-only credential update, lock/unlock a different profile, and history Back. Stress adds one explicitly synthetic Unassigned long-label/malformed-level case. Screenshots and validation logs are preserved in `/Users/hack/Documents/Personal/Code/alexandria-mark3/work/skill-graph-implementation/ticket03-personal-surfaces/`.

## Limitations

- Browser regression coverage is not packaged native WebView validation; no real user profiles, native builds, or backend changes were involved.
- Custom graph pinch zoom is intentionally absent to preserve native touch scrolling/zoom; horizontal touch panning, full mouse/pen drag, buttons, and keyboard navigation are available.
- Non-English navigation guidance is machine-drafted and still benefits from native-speaker review.

## Accepted review fixes — 2026-10-27

- Spec P2(1), superseding the original learning-action handoff above: `useSkillAtlasActions(surface).learnSkill(id)` now opens `/courses?skillId=<id>` for both earned and Ready skills. It no longer creates a goal. Course browse uses existing catalog hydration/listing and `Course.skill_ids` to display matching courses/tutorials independently of earned status. The selected skill is named, counts reflect that skill, entry starts with all content kinds, and empty results offer access to general browsing. Existing course cards continue to course detail or tutorial playback. Guided paths, assessment eligibility, and the existing Goals feature are unchanged.
- Standards finding 1: `actionError` is returned through `readonly()`; session ownership, navigation failure reporting/retry, and modal close/scroll cleanup are retained.
- Mounted red → green first reproduced the old completed-goal destination, then followed an earned Big-O skill into a matching course and its real production detail page, showing a reading lesson and enabled Enroll. Ready learning, route reuse, prior browse preferences, unrelated-course exclusion, empty results, and navigation failure/retry are covered. Shared strict IPC fixtures now include illustrative learning content and mount production course browse/detail routes.
- Final checks: 36 focused tests and 441 frontend tests pass; strict `vue-tsc -b --noEmit`, production build, generated keys, nine-locale parity (3542 keys), no-raw-text, and diff checks pass. `DESKTOP_UI_PORT=5199 npx playwright test --workers=2`: 45 pass, one existing WebKit native-touch-synthesis skip. Screenshots inspected for overview/list emphasis, earned learning content, mobile long labels, and both themes. The first browser run was interrupted by fixture live reload; later investigation also observed an intermittent Chromium reduced-motion animation-count assertion. The final uninstrumented full run passes that original assertion; diagnostic logs are retained.
- Fixture: `http://127.0.0.1:5199/tests/desktop-ui/?screen=personal-atlas&theme=dark` (start with `npm run dev -- --host 127.0.0.1 --port 5199 --strictPort`). Logs/screenshots: `/Users/hack/Documents/Personal/Code/alexandria-mark3/work/skill-graph-implementation/review-fixes-v1/`. This remains a browser/IPC fixture, not native WebView, catalog availability, enrollment completion, or credential-policy validation. New locale copy is machine-drafted. README/other documentation assessed; no unrelated documentation changes needed.
