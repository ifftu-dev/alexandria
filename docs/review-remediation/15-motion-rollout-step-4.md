# Motion rollout step 4

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/14-press-feedback-and-rollout-step-2.md

## Task

Complete rollout step 4 through the shared policy, tokens, press treatment and existing transition components on these surfaces:

- instructor authoring and classroom
- chat message arrival
- live-call controls
- settings section changes and toggles
- guardian oversight and approvals
- admin surfaces

Expressive motion stays reserved for opening, onboarding and learning milestones. Chat and call motion never delays input or media. When this lands, propose an update to the motion spec's Status line (per the workspace docs rule).

Covers spec stories 39–44.

## Acceptance

- The desktop UI suite covers Reduced/Off behaviour on each step 4 surface.
- Chat message arrival shows nothing extra in Off mode.
- The existing chat and call tests pass unchanged.

## Completion evidence — 2026-11-04

What changed, by surface. Everything uses the existing tokens, `animateMotion`, `holdLeavingPlace`, `.pressable`, `.selectable` and the `motion-rise` / `motion-list` transitions; no new library or primitive. In Reduced and Off nothing moves and nothing is hidden.

- **Instructor authoring.** My Courses cards enter, leave and move as `motion-list` and take the press. In the composer, the stage switcher is `pressable` and `selectable` (`aria-current="step"`), and the next stage settles in over `--duration-base`. Outline chapters and lessons are `motion-list`, so added lessons rise in and a drag reorder moves the rows into place. Lesson rows take the press and show the selection indicator (`aria-current`). Inbox rows take the press and are a `motion-list`.
- **Classrooms.** Classroom cards enter as `motion-list` and take the press. Channels take the press and the selection indicator. New channel messages rise in. A channel's history, and another channel, show at once. Join requests leave as a `motion-list` when approved or denied.
- **Chat message arrival.** AI chat and call chat (tutoring and interview) messages rise in as `motion-rise`, entry only. The AI chat list is keyed by thread, so opening a chat shows its history at once. Sending, typing and following the answer to the live edge never wait for motion. Off shows nothing extra.
- **Live-call controls.** The microphone, camera, screen-share and leave buttons, and the call header actions, take the press. Their state colours settle over `--duration-fast` through the class. Media toggles call the backend on the first click, as before.
- **Settings.** Section items take the press and the selection indicator, and the new section settles in. `AppSwitch` takes the press, and its thumb slides over `--duration-base`. Theme and visibility options take the press and expose `aria-pressed`.
- **Guardian oversight and approvals.** Child cards enter as `motion-list` and take the press. Child detail tabs (`AppTabs`, now `pressable` with `aria-pressed`) settle the chosen panel in. A decided AI-model request settles into its new state.
- **Admin surfaces.** The app has none. Organisation administration lives in Alexandria Cloud.
- **Shared.** `.selectable` now accepts any `aria-current` value except `false` (page, step, true).

Red → green:

- `motion-step4.spec.ts` (desktop UI, Chromium and WebKit; System, Reduced and Off each), over the `motion-surfaces` fixture, which now also mounts Settings, a chat panel, the tutoring call, a classroom, guardian and instructor pages:
  - settings sections and switches: red (no section motion, no switch press), then green.
  - chat messages arrive without holding input or scroll: red (no arrival motion), then green.
  - live-call controls and call chat: red (no press on the call toggles, no arrival motion), then green.
  - classroom cards, channels and messages: red (no press, no arrival motion), then green.
  - guardian cards, tabs and approval: red (no press, no panel or decision motion), then green.
  - instructor cards, stages and outline: red (no press, no stage or outline motion), then green.
  - In Reduced and Off every check asserts no motion longer than the 0.01 ms floor and full opacity.
- `pressFeedback.test.ts` (AppSwitch, AppTabs) and `LiveSessionControls.test.ts` (Vitest): red against the previous components, then green.
- The existing chat and call tests pass unchanged.

Checks: full Vitest suite (91 files, 569 tests passed); full desktop UI suite (144 passed, 2 skipped by existing browser-specific guards); `npx vue-tsc -b --noEmit`; `npm run check:csp`; `npm run i18n:keys`; `npm run i18n:parity`; `npm run i18n:no-raw-text`; `npm run check:tauri-commands`; `scripts/check-docs.sh`.

Deviations:

- WebKit drops `:active` when a press lands on a clipped (truncated) descendant. Settings section items, channel names and outline and inbox rows hold no controls, so their content does not take the pointer and the press reaches the control.
- Classroom "load earlier messages" adds older messages through the same `motion-rise` list, so they also rise in above the reader.
- The interview call page is not mounted in the desktop UI suite. Its controls (`LiveSessionControls`) are covered by Vitest, and its chat uses the same transition as the tested tutoring chat.
- `AppTabs` buttons are now `type="button"`, so a tab inside a form no longer submits it.

## Notes

- 2026-11-05: Messages that "load earlier messages" inserts above the oldest shown message now appear at once; only new arrivals rise in. This replaces the classroom deviation above. Red → green: the classroom step in `motion-step4.spec.ts` loads an earlier message in every mode and expects no motion on it; red in System (2 transitions), now green.
- 2026-11-05: `AppTabs` is a `tablist` of `tab`s with `aria-selected` instead of `aria-pressed`. Only the selected tab is in the tab order; the arrow keys (mirrored right-to-left), Home and End move focus and selection. Panels are not wrapped by `AppTabs`, so there is no `aria-controls`. Red → green: `pressFeedback.test.ts` (roles, selection, roving focus, keys, right-to-left), and the guardian step in `motion-step4.spec.ts` (selection and arrow keys); red, now green. The atlas specs now find the skills page tabs by the tab role.
