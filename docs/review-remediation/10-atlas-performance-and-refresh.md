# Atlas: performance and refresh policy

Triage: done
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

## Completion evidence — 2026-11-04

What changed:

- **Refresh policy.** Route changes no longer refresh the atlas. It refreshes when an atlas surface comes on screen, when the window returns with one showing, on credential events and on profile change. A surface opening within 1.5 s of a load reuses that snapshot. Generation tickets still reject stale loads. Course completion claims and credential imports now announce credential changes, as assessments and revocation already did.
- **Indexing.** The atlas model carries `nodeById` and `fieldById`, built once per load. Feedback diffs, the explorer and the sidebar's earned previews use them. The snapshot sits in a shallow, raw ref.
- **Rendering.** A drag moves a camera local to the explorer and publishes the view once, at drag end. The match list is memoised, so panning re-renders only the map. Edges outside the visible map are not drawn. Edge geometry, search text and overview summaries are computed once per model.
- **Match list window.** The list renders 50 rows, then 50 more on "Show N more skills" or on scrolling near its end. The summary still counts every match. New strings are translated by hand into all nine locales.
- **Scale fixture.** `atlasBackend('scale')` builds 1,500 skills over every subject of every field, with 3,880 prerequisite edges.

Red → green:

- `useSkillAtlas.test.ts`: no refetch on route changes; a refresh when a surface comes on screen or the window returns with one showing.
- `skillAtlas.test.ts`: node and field lookup by ID, including unassigned skills.
- `SkillAtlasExplorer.test.ts`: a drag moves the map live and publishes the camera once; only on-screen edges are drawn; the list renders a window, keeps it while selecting, and resets it on a new search.
- `Player.test.ts`, `ImportCredentialsModal.test.ts`: course completion and import announce credential changes.
- `atlas-performance.spec.ts` (Chromium): before the change, panning the scale fixture caused 150 long tasks, the longest 80 ms. After it, there were none in six runs.
- `PersonalSkillAtlas.test.ts`: the learning-for filter shows courses and tutorials after a tutorials-only browse, and the Goal button saves one goal for a double tap and reports a failed save. Both passed at once, as the behaviour already existed. Removing the kind-filter reset or the pending-goal guard makes them fail.

Checks: the targeted Vitest files, `npx playwright test` on the skill-atlas, personal-atlas and atlas-performance specs (Chromium and WebKit, 32 passed, 2 skipped), `npx vue-tsc -b --noEmit`, `npm run i18n:keys`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, `npm run check:tauri-commands`, `npm run check:csp`, `scripts/check-docs.sh`.

Deviations:

- The match list grows a window instead of using fixed-row virtualisation. Its rows keep full, wrapping labels, which fixed-height rows could not. Scrolling to the end of a very long list can still mount every row.
- The sidebar graph is not an atlas surface. It updates on credential events and profile change only.
- The perf spec is skipped on WebKit, which has no Long Tasks API.
- Existing tests that counted every list row now assert the window and the full count. The sidebar test now expects no refresh on navigation.

## Notes

- 2026-11-05: Four issuing paths did not announce credential changes, so the atlas stayed stale after them: graded plugin submissions (`plugin_submit_and_grade`), skill bootstrap confirmation, `useCredentials().issue` and role credentials. Each now announces after it succeeds; a bootstrap that claims nothing stays silent, as does every failure. Red → green: `PluginHost.test.ts` and `credentialIssuance.test.ts` (issue, bootstrap, role credential); red (no event), now green.
