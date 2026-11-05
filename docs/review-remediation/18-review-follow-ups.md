# Review follow-ups

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

What the post-implementation review of tickets 01–16 left open, plus the tooling gaps CI exposed.

**Backend standards.**

- Guardian AI requests: the request type lives in `domain/` and its status is a closed enum, mirrored in `src/types/index.ts`.
- The profile session travels as a type of its own, not a bare string.
- The guardian decision and its peer-to-peer approval payload, and the notification payload built in two places, each become one shape.
- Push: the permission mapping exists once, and the push issuer comes from `Issuer::from_keystore`.
- `NotificationError::Profile` is split or renamed so a database failure is not called a profile failure.
- The notification vocabulary snapshot is owned by the notifications module, not AI chat; the AI access prose names `AccessDenied` correctly.
- The notification permission type no longer shares its name with the browser's `NotificationPermission`.

**Frontend standards.**

- Durations in components come from the motion tokens (story 47).
- One settle-in helper replaces the copies in the layout, settings, guardian and instructor pages.
- One motion list primitive replaces the repeated `TransitionGroup` blocks.
- The selection-indicator class no longer collides with `[data-selectable]`.
- The search palette renders results from one template.
- The notifications inbox list enters, leaves and moves like the other lists.

**Skill atlas.** Stronger emphasis goes only to skills whose real prerequisites are earned (story 51).

**Tooling and docs.**

- `scripts/check.sh` also runs the release workflow's Python tests.
- The WebKit press-feedback check stops flaking.
- `src/AGENTS.md` and `src-tauri/src/AGENTS.md` note the progress-marker guard and how synced settings are followed.

Ticket 17's device checks stay with a person.

## Acceptance

- Each item has a test at the module interface or the desktop UI suite, red before and green after, or is a pure refactor covered by existing tests.
- `scripts/check.sh` and the desktop UI suite pass.
- Text the webview sees is unchanged, or the change is listed here.

## Completion evidence (atlas) — 2026-11-05

What changed:

- **Ready next (story 51).** A not-yet-earned skill is ready next when it has at least one prerequisite and every prerequisite it counts is loaded and earned. Skills with no prerequisites, with partly earned prerequisites, or with a prerequisite that is unloaded or uncounted are not ready next. `prerequisitesSatisfied` now also needs every prerequisite loaded and the loaded count to equal `prerequisite_count`, so an earned but unloaded prerequisite no longer satisfies it. Ready next skills get a stronger map node (field-coloured border, a wider field edge and a field tint) and a tinted list row, with a decorative ◎. Their accessible name says "ready next" ("Sorting Algorithms, not yet earned, ready next, learning-outcome Bloom level 3 Apply"), and so do the evidence line, the inspector status and inspector relations. Every unearned skill is still counted as Ready. The class is in the node's `v-memo` dependencies, and the label comes from the cached node text.
- **Durations.** `src/components/skills/**` takes its transition durations from the `--duration-*` tokens.

Text the webview sees:

- New: "Ready next", "{name}, not yet earned, ready next[, learning-outcome Bloom level {level}]" and "Inspect {name}, not yet earned, ready next". The atlas legend's last sentence now reads "Every other skill is Ready; ◎ Ready next marks those whose prerequisites you have all earned." All are translated by hand into the nine locales; the other locales keep their "Ready" word (for example "Lista para empezar").

Red → green:

- `skillAtlas.test.ts`: ready next only with every counted prerequisite loaded and earned (none, partial, unloaded, uncounted, over-counted and already earned cases), and an earned but unloaded prerequisite no longer satisfies a skill.
- `SkillAtlasExplorer.test.ts`: only the ready next skill is emphasised in the list and on the map, with its accessible name, visible label, inspector status and relation names; the memoised node gains and loses the emphasis as the model changes.
- `skill-atlas.spec.ts` (Chromium and WebKit): Sorting Algorithms is ready next with the stronger node; Greedy Algorithms (partly earned), Graph Algorithms and Python (no prerequisites) stay Ready. It failed against the previous code.
- `motionDurations.test.ts`: the three `src/components/skills` entries failed, then passed.

Checks: full Vitest (98 files, 605 tests), `npx playwright test` on the skill-atlas, personal-atlas and atlas-performance specs (Chromium and WebKit, 34 passed, 2 skipped), `npx vue-tsc -b --noEmit`, `npm run i18n:keys`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, `npm run check:csp`, `scripts/check-docs.sh`.

Performance: `atlas-performance.spec.ts` saw no long tasks. With 20× CPU throttling on the scale fixture there were none in six runs on a quiet machine. With other builds running, the old and new code both had occasional single tasks of 50–65 ms, and outliers of about 200–300 ms, in an interleaved comparison of eight runs each.

## Completion evidence (frontend) — 2026-11-05

What changed:

- **Durations from tokens (story 47).** Component styles (45 declarations outside the atlas), main.css (7), the completion modal's scripted confetti and credential stagger, and 64 Tailwind `duration-<n>` utilities (now `duration-(--duration-…)`) read the `--duration-*` tokens, mapped to the nearest one. A new `--duration-stagger` (60 ms) sets the step between staggered entries, and bare Tailwind transitions default to `--duration-fast`. `motionDurations.test.ts` reads every component and main.css and fails on a literal duration or delay in styles, style bindings, scripted `ms` values or Tailwind utilities. Its allowlist, each with a reason: the Reduced/Off floor, negative phase offsets of loops, and loop periods.
- **`useSettleIn`** (`useMotion.ts`) returns one settle-in that cancels the last on each call and on unmount. It replaces the copies in the layout, settings, guardian child detail, composer, onboarding and guardian AI request rows. `useMotion` now creates its settings watch on first use in a detached scope, so importing motion helpers touches no settings.
- **`MotionList`** (`components/ui`, in the barrel) replaces the 13 `motion-list` `TransitionGroup` blocks: Home (4), goals, catalog, classrooms, join requests, guardian children, My Courses, instructor inbox and the composer outline (2).
- **Selection class.** `.selectable` is now `.selection-indicator`, in main.css, seven components, `pressFeedback.test.ts` and `src/AGENTS.md`.
- **Search palette.** Grouped results and empty-query recents render from one row template (recents are an untitled section with the clock icon). The `*:pointer-events-none` fix and its comment stay.
- **Notifications inbox.** The list is a `MotionList`: rows enter, leave and move in full motion and change at once in Reduced and Off. The `motion-surfaces` fixture mounts the inbox and keeps backend event listeners, so the spec can announce a row and resync after another window removes one.
- **Press flake.** Sidebar items, mode switcher segments, Home goal cards, lesson rows and the dialog close button hold no controls, so their content takes no pointer. The catalog chip has no child elements. `pressedStyle` presses only once the control's centre is steady and hits the control. If a loaded browser drops the press (nothing is `:active`), it presses again, at most three times. The pressed-style assertions are unchanged.
- **Docs.** The Motion bullet in `src/AGENTS.md` names both guards, `MotionList`, `useSettleIn` and `.selection-indicator`.

Red → green:

- `motionDurations.test.ts`: red with 55 style entries (3 of them in the atlas, converted by the atlas work) and 3 inline or scripted ones, now green.
- `useMotion.test.ts` (useSettleIn: frames, cancel on the next call and on unmount, skipped in Reduced) and `MotionList.test.ts` (positioned group, attributes, leaving item held in place, not held in Off): red, then green.
- `pressFeedback.test.ts`: red against the old class name, now green.
- `motion-surfaces.spec.ts` "notification inbox rows enter, leave and move" (Chromium and WebKit; System, Reduced, Off): red in System (no motion), now green.
- `OmniSearch.test.ts` (new) covers both renderings. It was green before and after the refactor.
- Press flake: under parallel load the press check failed on several controls in both browsers. Diagnostics showed nothing `:active` while the pointer was down over the control. After the change, the motion-surfaces and step 4 specs ran three times under stress (3 repeats, 12 workers). In the first run, two WebKit press checks timed out after the press had registered. Dev-server reloads from concurrent edits broke two other tests in that run. The next two runs passed, 246 of 246 each. On CI, the flake stays to be watched.

Checks: full Vitest (98 files, 605 tests passed); full desktop UI suite (154 passed, 2 skipped by existing browser guards); `npx vue-tsc -b --noEmit`; `npm run check:csp`; `npm run i18n:keys`; `npm run i18n:parity`; `npm run i18n:no-raw-text`; `npm run check:tauri-commands`; `scripts/check-docs.sh`.

Visible behaviour changes (no text changes):

- Durations moved to the nearest token. Most 150 ms hovers and bare Tailwind transitions are now 120 ms. Tailwind 75/100 ms → 120, 300 → 320, 500 → 650. The Player progress bar is 560 → 650 ms, the sidebar live ping 600 → 650, the course thumbnail zoom 400 → 320, the governance phase bar and vote gauge 600/500 → 650, and the opening starfield drift 480/560/640 → 320/650/650.
- Completion modal: emblem 600 → 650 ms, fade-ups 500 → 650, chips 400 → 320, entry fade 250 → 200. Delays are 60/120/180/240/300/420 ms, previously 50/100/150/220/280/400. Credential rows are staggered 60 ms apart (previously 90).
- Onboarding steps and decided guardian AI requests now use the shared settle-in frames (0.65 opacity, 6 px), previously 0.5/8 px and 0.6/4 px.
- The notifications inbox now animates its rows in full motion.

## Completion evidence (backend) — 2026-11-05

What changed:

- **Guardian AI requests.** `GuardianAiRequest` and its status are in `domain/guardian_ai.rs`. The status is the closed `GuardianAiRequestStatus` vocabulary (`pending`, `approved`, `declined`), mirrored as `GuardianAiRequestStatus` in `src/types/index.ts`. `ai_access` re-exports both and reads the status column into the enum. The column already has a CHECK on the same three strings, so no stored row changes meaning.
- **One decision shape.** `GuardianDecision` (also in `domain/guardian_ai.rs`) is what `ai_access` returns and what the guardian sync seals. `ApprovalDecisionPayload` is gone. Its field names are the peer-to-peer wire format, and a test pins them.
- **Profile session.** `profile::scope::ProfileSession` is a newtype serialised as its string (`#[serde(transparent)]`). Admission, `ProfileLease::session_id`, `ProfileOperations::{session, admit}`, `notifications::{announcement, announce, emit_changed}`, `InboundDatabase::run_in_session` and `pinned`, inbound UI events, `P2pEvent::GuardianLinked`, `P2pEvent::SettingsSynced`, the Cloud discovery slot and `get_profile_session_token` carry it. Only the IPC session header builds one from webview text.
- **One Cloud notification.** `notifications::CloudEvent::notification` builds the copy, route, dedupe key and payload (`id`, `event_id`, optional `cloud_origin` and `role_label`) for both Cloud discovery and push.
- **Push.** The holder proof key comes from `credentials::Issuer::from_keystore`, split with `into_parts` because a proof is not a credential (`push::issuer`, also used by Cloud discovery). The permission prompt maps through `NotificationPermissionOutcome::of_prompt` alone. Push registration matches on its outcome.
- **Errors.** `NotificationError::Executor` holds a failed database job or an executor refusal. `NotificationError::Profile` now holds only admission, lease and profile id failures. Both still display the underlying text.
- **Vocabulary ownership.** `notifications::vocabulary` owns `scripts/notification-vocab.json` (`notification_vocab_snapshot_is_current`, rewritten by the new `npm run generate:notification-vocab`). `scripts/chat-vocab.json` holds the chat lists only. `scripts/check-tauri-commands.mjs` reads each snapshot on its own and names the matching generate script when a generated file is stale. The snapshot check and the round-trip check are shared helpers in `crate::vocabulary`. The `Surface::ChatState` doc now names `AccessDenied::AttemptInProgress`.
- **Permission name.** The Rust vocabulary, the generated TypeScript type and their importers (`useNotifications`, `NotificationsPanel` and its test) use `NotificationPermissionOutcome` and `NOTIFICATION_PERMISSION_OUTCOMES`. The snapshot key is `notification_permission_outcomes`. The wire strings are unchanged.
- **Tooling and docs.** `scripts/check.sh` runs the release workflow regression tests (`python3 -m unittest discover -s scripts -p 'test_*.py'`). The Settings bullet in `src-tauri/src/AGENTS.md` says how synced settings are followed.

Red → green:

- `ai_access` guardian tests, rewritten to expect `GuardianAiRequestStatus`: they failed to compile, then passed. New `domain::guardian_ai` tests cover the status strings and the decision's wire fields.
- `notifications::tests::a_cloud_event_records_the_same_row_from_discovery_or_a_push`: failed to compile, then passed.
- `push_tests::a_failed_database_job_is_an_executor_failure_not_a_profile_one`: failed to compile, then passed.
- `check-tauri-commands.test.mjs` "a notification snapshot missing the notification kinds fails the guard": failed, then passed. "a broken notification snapshot leaves the chat vocabulary checked" is new.
- `profile::scope::tests::a_session_crosses_the_bridge_as_a_plain_string` pins the wire form. The session change, the push changes and the rename are refactors covered by the existing notification, push, Cloud discovery, inbound, guardian and scope tests.

Checks: `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `cargo test -p alexandria-node` (1,750 lib tests and all integration tests passed); `cargo check -p alexandria-node --tests --features personhood-lab`; `cargo check -p alexandria-node --target aarch64-apple-ios --features tutoring-video-ios`; `npm run check:tauri-commands` and its regression tests; `npx vue-tsc -b --noEmit`; `scripts/check.sh --fast`, which passed every step; `scripts/check-docs.sh`.

Text the webview sees: unchanged. The renamed type and constant are TypeScript names only.

Not done: `GuardianApproval` (the ward's own view, from `ai_connection_approvals`) still carries `status: String`, and `PendingApproval` still has a field-for-field copy in `ApprovalRequestPayload` (`id` vs `request_id`). Neither was in this ticket. The notes in tickets 06 and 13 that name `scripts/chat-vocab.json` and `NotificationError::Profile` describe what shipped then and are left as written.
