# Review remediation: notifications, motion, skill atlas and deepening follow-ups

Status: active
Updated: 2026-11-04
Triage: ready-for-agent
Repos: alexandria

Follows a two-axis review (repo standards and originating specs) of everything merged since the AI chat actions landed: motion rollout, notifications and push, architecture deepening, skill-graph atlas, and the iOS/release fixes. The originating specs are [notifications](notifications-spec.md), [motion](motion-design-spec.md), [skill-graph atlas](skill-graph-atlas-spec.md) and [architecture deepening](architecture-deepening-spec.md). The atlas and deepening specs are implemented and therefore frozen. Their follow-ups, and any behaviour this work accepts after the fact, are recorded here.

## Problem Statement

A learner who switches off "Receive requests from organisations" still has their DID and push token registered with Alexandria Cloud, and still receives interview and offer pushes on their phone. Turning a setting off does not stop what it says it stops.

Several other parts of the app also don't do what their specs promised:

- **Phones:**
  - Opening the app from the background doesn't pick up activity that push missed.
  - Some notification text and every push-registration error appear in English whatever the learner's language.
- **Launch and loading:**
  - Launching the app shows a different-coloured native splash before the web opening, or two splashes on Android.
  - With Reduce Motion on, spinners freeze, so a loading button looks hung.
  - A slow start shows "Initializing" forever with no hint that it is slow.
- **Animation coverage:** most of the app (navigation, search, lists, home, goals, the catalog, daily learning, and the instructor, chat, live, settings, guardian and admin surfaces) has no motion at all, while the rest of the app does.
- **Skill atlas:**
  - Screen-reader users never hear that a skill is earned.
  - The map can stay dimmed with nothing on screen explaining why.
  - Skills with no prerequisites claim "prerequisite evidence available".
  - Field colours and regions reshuffle when a subject field is added.
  - Large graphs are slow to pan, and the atlas refetches on every route change.

Maintainers face drift between code and the project's documented rules:

- Notification types cross the Rust↔TypeScript boundary in camelCase.
- Internal modules pass `String` errors instead of module error enums.
- One credential refactor rule ("the issuer travels as one value") is routinely worked around, and a rebuilt issuer can carry the wrong DID after a key rotation.
- The guardian side of AI approvals still runs its own SQL.
- Several behaviour changes landed without a spec recording them.
- A crate README describes the wrong push token.

## Solution

Bring the shipped features back in line with their specs and the repo's rules, in four areas.

- **Notifications respect the learner's preferences and profile.**
  - Push is registered with Cloud only while both push and organisations are on. Turning organisations off retires the registration.
  - Every event, unread count and OS banner carries the profile that produced it.
  - A phone runs discovery as soon as the app resumes.
  - All notification text and push errors are translated.
  - Public deep links that nothing produces are removed.
  - Notification wire types become snake_case domain types built from a shared vocabulary.
- **Motion reaches the whole app and hands off cleanly from the OS.**
  - The native launch surface matches the web first paint on every platform, with no second splash.
  - Progress indicators stay visible and moving (slowly) in Reduced mode, and stay readable in Off mode.
  - A slow start or slow unlock says so.
  - Press feedback and contextual transitions cover the rollout's remaining surfaces through the shared motion policy and tokens.
- **The skill atlas is accessible, honest, stable and fast.**
  - Earned state is announced.
  - Any active focus or filter is always visible on the map.
  - Prerequisite wording is true for skills that have no prerequisites.
  - Every subject field keeps a stable colour and region.
  - Large graphs pan smoothly, and the atlas refreshes only when it is visible or its data changes.
- **The deepening refactor is finished.**
  - The guardian's AI approvals go through the AI access module.
  - Credentials are signed with a whole issuer.
  - Module interfaces return typed errors.
  - Behaviour changes the umbrella spec did not cover are recorded as decisions.
  - Ticket bookkeeping matches reality.

## User Stories

### Notifications and push

1. As a learner, I want switching off "Receive requests from organisations" to also stop organisation pushes to my phone, so that one switch means what it says.
2. As a learner, I want my push registration removed from Alexandria Cloud when I switch organisations off, so that Cloud no longer holds my DID and device token for a feature I declined.
3. As a learner, I want my push preference remembered while organisations are off, so that switching organisations back on restores pushes without another trip to Settings.
4. As a learner, I want Settings to say that push is paused because organisations are off, so that I understand why pushes stopped.
5. As a learner with no network when I switch organisations off, I want the removal to stay pending and retry until Cloud acknowledges it, so that going offline doesn't leave me registered.
6. As a learner sharing a device with other profiles, I want every unread count, inbox event and OS banner to belong to the profile that produced it, so that I never see another profile's activity.
7. As a learner who switches profile while a notification is being recorded, I want that notification dropped from the new profile's view, so that a profile switch never leaks or mislabels activity.
8. As a learner who marks notifications read or clears the inbox and then switches profile, I want the badge update tied to the profile I acted in, so that the next profile's badge isn't overwritten.
9. As a mobile learner, I want the app to look for missed activity as soon as I bring it back to the foreground, so that an interview invitation push failed to deliver shows up straight away.
10. As a mobile learner, I want resume-time discovery not to run twice at once with the background loop, so that I don't get duplicate network requests or duplicate inbox rows.
11. As an Android learner using a language other than English, I want the notification channel name and description in my language, so that system notification settings read naturally.
12. As a learner, I want push-registration failures explained in my language, so that I can fix the problem from Settings.
13. As a support reader, I want the untranslated technical detail of a push failure still recorded for diagnostics, so that translation doesn't hide the cause.
14. As a learner, I want external links unable to open, mark read or navigate my inbox rows, so that a web page can't act on my notifications.
15. As a learner, I want tapping a push to keep taking me to the right screen after the public notification links are removed, so that nothing I use breaks.
16. As a developer, I want the debug-only notification emitter listed as debug-only and refused in release builds, so that test tooling never ships as a feature.
17. As a developer, I want notification rows, push status and notification opens to be snake_case domain types on both sides, so that they follow the same rule as every other domain type.
18. As a developer, I want notification kinds generated from one vocabulary into the TypeScript types, so that adding a kind is one edit and the webview can't drift.
19. As a developer, I want a notification's kind and source typed rather than free strings, with unknown kinds from outside handled explicitly, so that typos don't compile.
20. As a developer, I want the interview-room route built in one place, so that inbox rows and Cloud discovery can't disagree about where a notification leads.
21. As a developer, I want the notification permission request to return a typed result whose values match its documentation, so that the webview handles every real case.
22. As a developer integrating the mobile push plugin, I want its README to say iOS returns an FCM token, so that I don't build against a hex APNs token that never arrives.

### Launch, motion and visual polish

23. As an iPhone learner, I want the launch screen to use the same light or dark background as the app's first frame, so that launch doesn't flash a different colour.
24. As an Android 12+ learner, I want the system splash and the app opening to look like one continuous moment, so that I don't see two splashes in a row.
25. As a desktop learner, I want the window to appear only when its first frame is ready, in the right theme colour, so that it never flashes white or black.
26. As a desktop learner, I want the window to appear anyway if the first frame is delayed, so that a slow webview never leaves the app invisible.
27. As a learner with Reduce Motion on, I want spinners to keep turning slowly, so that I can tell the app is working rather than hung.
28. As a learner with motion set to Off, I want progress shown as a still indicator with readable text, so that loading is clear without any movement.
29. As a learner, I want a loading button to keep an accessible label while it loads, so that screen readers announce what is happening.
30. As a learner whose app is slow to start, I want "Taking longer than expected" after about two seconds, alongside the current startup step, so that I know the app hasn't frozen.
31. As a learner unlocking a large profile, I want a "still unlocking" message if decryption takes a while, so that a long unlock feels honest rather than stuck.
32. As a learner, I want tactile press feedback on every control, not only standard buttons (nav items, tab bar items, list rows, search results, chips and icon buttons), so that the whole app feels equally responsive.
33. As a learner, I want the sidebar, mobile tab bar and mode switcher to show selection changes with a short, consistent transition, so that I keep my bearings while navigating.
34. As a learner, I want search to open, close and show results with gentle motion, so that the results appearing feels connected to what I typed.
35. As a learner, I want list items to enter, leave and reorder smoothly, so that I can follow what changed in a list.
36. As a learner, I want home, goals and catalog cards to arrive and respond with consistent motion, so that browsing feels polished.
37. As a learner, I want adding or removing a goal to visibly confirm itself, so that I know it took effect.
38. As a learner in a lesson, I want moving between lesson steps to preserve context with a contextual transition, so that daily learning feels continuous.
39. As an instructor, I want my authoring and classroom surfaces to share the same motion language as the learner surfaces, so that the app feels like one product.
40. As a learner in chat or a live call, I want arriving messages and call controls to animate subtly without delaying input, so that conversation feels alive but stays fast.
41. As a learner in Settings, I want section changes and toggles to respond with brief motion, so that changes feel confirmed.
42. As a guardian, I want oversight and approval screens to use the same calm motion, so that important decisions don't feel jarring.
43. As an administrator, I want admin surfaces to follow the shared motion policy, so that they stay consistent and respect my motion setting.
44. As a learner with Reduce Motion or motion Off, I want every new transition to follow my setting while keeping all information and feedback visible, so that accessibility holds everywhere.
45. As a learner, I want my motion preference kept per device and applied before the first paint, and still managed by the normal settings system after unlock, so that it behaves like my theme does.
46. As a learner using dark mode, I want badges and completion screens to use the app's light and dark colours rather than fixed hex values, so that both themes look right.
47. As a designer, I want every easing curve and duration to come from the shared motion tokens, so that tuning motion is one change.

### Skill atlas

48. As a screen-reader user, I want each skill on the map, in the list and in previews to announce whether I've earned it, as well as its Bloom level, so that I get the same information sighted learners do.
49. As a learner, I want an on-map indicator showing any active prerequisites or dependents focus, its depth and its skill, with a way to clear it, so that a dimmed map always explains itself even with the inspector collapsed.
50. As a learner, I want skills with no prerequisites labelled "No prerequisites" rather than "Prerequisite evidence available", so that the inspector never claims evidence that doesn't exist.
51. As a learner, I want stronger emphasis to go only to skills whose real prerequisites I've earned, so that emphasis means "ready next".
52. As a learner, I want each subject field to keep its colour and map region when new fields are added, so that the map I've learned stays familiar.
53. As a learner, I want every subject field to have a distinct colour, so that fields stay distinguishable as the taxonomy grows.
54. As a learner with a large skill graph, I want panning and zooming to stay smooth, so that the atlas stays usable at scale.
55. As a learner, I want the atlas to refresh only while it is on screen or after my skills change, so that browsing elsewhere doesn't do needless background work.
56. As a learner, I want long match lists to stay responsive, so that searching a large atlas doesn't stall.
57. As a learner, I want the evidence highlight to finish its animation before it clears, so that the feedback I just earned isn't cut short.
58. As a learner coming from a skill, I want the catalog's "learning for" filter to keep working, so that finding courses for a skill stays one tap.
59. As a learner, I want the Browse "Goal" button to stay safe against double taps and to report errors, so that goals are never added twice or silently fail.
60. As a developer, I want the atlas's field name, field colour and Bloom label shown by one shared presenter, so that the sidebar graph and the explorer can't disagree.
61. As a developer, I want atlas layout spacing as named constants derived from region size, so that layout changes are readable and safe.
62. As a developer, I want atlas tab, Bloom rank and field ID values checked by guards rather than casts, so that bad values are caught instead of trusted.

### Architecture follow-ups

63. As a guardian, I want reviewing and deciding my ward's AI-connection requests to follow the same rules as the ward's side, so that approval behaves consistently.
64. As a developer, I want every read and write of guardian AI requests, including requests that arrive over P2P, to go through the AI access module, so that the rule has one home.
65. As a learner whose DID has rotated, I want credentials I earn from assessments and adaptive checks issued by my current DID, so that the issuer and subject never disagree.
66. As a developer, I want credential signing to take a whole issuer, with splitting into key and DID kept for non-credential signatures only, so that the "travel as one value" rule holds.
67. As a learner, I want to list my plugin reviews without unlocking my vault, so that reading my own data doesn't need signing keys.
68. As a developer, I want each extracted module to return a typed error enum, converted to a string only at the command boundary, so that callers can tell failures apart.
69. As a developer, I want the AI access refusal and the chat turn's dropped-block outcome to have distinct, descriptive names, so that the two are never confused.
70. As a developer, I want access refusals kept separate from Studio errors instead of converting both ways, so that a refusal stays a refusal.
71. As a maintainer, I want the under-13 refusal of AI checks, per-room call chat, the longer AI timeout, the attempt-gate timing and the Cloud module recorded as accepted decisions, so that the record matches what shipped.
72. As a maintainer, I want the credentials ticket's header to show it is done, so that the issue list reflects reality.
73. As a maintainer, I want the call-manager device retest recorded as human-verified (or its failures as new findings), so that the merge gate it promised is actually closed.
74. As a developer, I want the in-app path check shared by deep links and their parser, so that the link-safety rule has one implementation.
75. As a developer, I want the deep-link held target observed by tests through behaviour rather than exported from the production API, so that test hooks don't leak into the app.

## Implementation Decisions

### Notifications

- **Push eligibility.** Push registration is eligible only when the push preference is on, organisations are on, and a Cloud origin is present.
  - When organisations are off, push reconciliation treats the state like push disabled. It retires routing first, marks registrations for deletion, and sends the existing signed DELETE until Cloud acknowledges.
  - It never changes the stored push preference.
  - Changing the organisations setting triggers a push reconcile, the same way changing the push setting already does.
- **Push status** gains `paused_by_organisations` (boolean). Settings shows it as a translated explanation linking to the organisations switch.
- **Profile-bound events.**
  - Recording and announcing take the profile session from the lease of the job that produced the event. Nothing reads the profile session at send time.
  - The new-event payload, the unread-count payload and the OS banner's profile tag all use that captured session.
  - If the lease is no longer current when the announcement would go out, the announcement and banner are dropped. The row stays in its own profile's store.
  - Mark-read, mark-all-read and clear emit their count update under the lease they ran with.
- **Resume discovery.**
  - A new command runs one discovery pass for the current profile.
  - The webview calls it from its existing resume handler on mobile, alongside push re-registration.
  - Discovery passes are serialised: a resume pass that overlaps the background loop's pass joins it instead of starting a second.
  - Results go through the existing de-duplication, so push and discovery can't double-record.
- **Localisation.**
  - The Android channel name and description come from the app's locale resources in the profile's language when the channel is created. They are refreshed when the language changes.
  - Push-registration failures become a small set of stable error codes, translated by the webview in all supported locales.
  - The raw failure text is kept as a separate diagnostic field.
- **Deep links.**
  - The public `alexandria://notifications` and `alexandria://notification/<id>` links, and the inbox's auto-open-on-query behaviour, are removed.
  - Native push taps and held-tap replay keep routing through the internal route they already use. Before removal, confirm that no native path depends on the public links.
  - If a producer needs a public notification link later, a new spec adds it and documents it in the deep-link docs.
- **Debug emitter.** The debug-only test command stays. It is refused outside debug builds and is listed as a debug-only command in this spec.
  - Debug-only commands: `notifications_emit_test`, which records a sample notification of a given kind for manual device testing. No UI calls it.
- **Wire types.**
  - The notification row, push status and notification-open types become domain types, snake_case in both Rust and TypeScript, as the workspace rules require. Event payload keys follow.
  - This is IPC-only. The Cloud wire contract (the seven FCM data keys and the Cloud delivery API) is unchanged.
- **Vocabulary.**
  - Notification kinds and sources move onto the existing vocabulary macro, which generates the TypeScript union.
  - The row's kind and source are typed. A kind arriving from outside that the app doesn't know is rejected at the boundary or mapped to the generic kind there, never carried as a free string.
- **Interview-room route.** One function builds it, used by both the inbox and Cloud discovery.
- **Permission request.** It returns a typed outcome (granted, denied, or prompt-unavailable), with documentation that matches the values it actually returns.
- **README.** The mobile push plugin README describes iOS token retrieval as an FCM token derived from the APNs token.

### Launch and motion

- **Native launch handoff.** Each platform's native launch surface uses the same light and dark background as the web first paint, with the same mark the opening uses:
  - **iOS:** a named light/dark colour asset in the launch screen.
  - **Android 12+:** the splash theme's background and icon. The web opening starts with the mark already in place, so it continues the system splash rather than replaying a reveal.
  - **Android below 12:** the window background colour.
  - **Desktop:** the window is created hidden with the theme background and shown on the first web paint, with a short fallback timeout that shows it anyway.
- **Progress keeps moving.**
  - The global reduced-motion rule exempts progress indicators through one shared marker: spinners, loading buttons and progress bars.
  - In Reduced (setting, or System with the OS preference), spinners keep their slow rotation.
  - In Off, progress indicators are static and paired with visible or accessible text.
  - A loading button always keeps an accessible name.
- **Slow start.**
  - The startup screen shows a translated "Taking longer than expected" line after about two seconds, without replacing the current step.
  - Profile unlock shows a translated "still unlocking" line after the same delay, alongside its single honest decryption stage.
- **Shared press feedback.**
  - One shared pressable treatment, built from the motion tokens, applies to interactive controls outside the standard button: navigation items, tab bar items, list rows, search results, chips and icon buttons.
  - Controls opt in through a shared class or wrapper, not per-component CSS.
- **Rollout steps 2 and 4** are completed through the shared policy, tokens and existing transition components. No new animation library.
  - **Step 2:** persistent navigation selection indicators; search open/close and result entry; list enter/leave/move; home, goal and catalog card entry and press; goal add/remove confirmation; lesson step transitions in daily learning.
  - **Step 4:** instructor authoring and classroom surfaces; chat message arrival and live-call controls; settings section changes and toggles; guardian oversight and approvals; admin surfaces.
  - Every new transition honours Reduced and Off and never delays input. Expressive motion stays reserved for opening, onboarding and learning milestones, as the motion spec directs.
- **Motion tokens are the single source.** The motion composable reads easing curves and durations from the CSS tokens instead of repeating them. Component animations use duration tokens. Timers tied to an animation, such as the atlas evidence highlight, derive from the same token as the animation.
- **Motion preference storage.**
  - The preference becomes a device-scoped setting in the settings registry.
  - The pre-unlock local cache stays, so it can apply before first paint, and mirrors that setting after unlock, as the theme already does.
- **Colours.**
  - The top-bar badge foreground uses the error-foreground token.
  - Completion-modal fallbacks use light/dark tokens.
  - The atlas field palette moves into the design tokens with light and dark values.

### Skill atlas

- **Accessible names.** Nodes, list items and previews build their accessible name from skill name, earned or not-yet-earned state, and Bloom level number and name. The checkmark is decorative.
- **Focus indicator.** An on-map focus indicator, always visible regardless of inspector state, shows the active prerequisites/dependents focus, its depth and its skill, with a clear action. The Bloom summary stays as is.
- **Prerequisites satisfied.** This means "has at least one prerequisite, and all are earned". Skills with none show "No prerequisites" and get no readiness emphasis.
- **Stable field slots.**
  - Each subject field gets an explicit, stable atlas slot in the bundled taxonomy, exposed on the subject-field domain type in both Rust and TypeScript. Colour and region order come from the slot, never from sorted position.
  - New fields take the next unused slot.
  - The palette has at least as many distinguishable light/dark colours as there are bundled fields, with a documented rule for wrapping beyond that.
  - Check whether the slot affects the taxonomy digest used for Cloud sync, and update the digest accordingly.
- **Refresh policy.** The atlas refreshes while an atlas surface is mounted and visible, and on profile change or credential/skill events. Route changes elsewhere no longer trigger it.
- **Performance.**
  - Nodes and fields are indexed by ID once per model load.
  - Panning updates a view transform without re-rendering the match list.
  - The match list is windowed.
  - Edges are culled to the viewport like nodes.
  - Overview field summaries are computed once per model, not per render.
- **Shared presenter.** One atlas presentation module owns field name, field colour and Bloom label for the sidebar graph and the explorer. Layout spacing is named constants derived from region size. The skill-return shape is a named type.
- **Guards.** Tab, Bloom rank and field ID values from the router or the DOM go through guards.
- **Accepted additions.** The catalog's "learning for a skill" filter (a skill ID query that shows a banner and resets the kind filter) and the Browse goal button's single-pending-goal protection are kept and recorded here as accepted behaviour.

### Architecture follow-ups

- **Guardian AI requests.** The AI access module gains guardian-side operations: record an inbound request, list requests for the overview, record a guardian decision, and mark a request delivered. The guardian commands and the P2P guardian handler call these. No SQL on guardian AI requests remains outside the module.
- **Whole issuer.**
  - Credential issuance paths take the issuer as one value from the point it is unlocked to the signing call.
  - Splitting into key and DID is reserved for non-credential signatures, such as directory proofs and organisation responses, as the issuer type already documents.
  - Rebuilding an issuer from a bare key in issuance paths is removed. That rebuild is what lets a rotated DID be stamped with the key-derived DID.
- **Learner DID without unlock.** Plugin review listing reads the learner's DID from the profile identity without unlocking the vault.
- **Typed errors.**
  - The notifications, credentials, AI access, enrollment, aggregation and AI chat operation modules each expose a thiserror enum at their public interface and may use anyhow internally.
  - Commands keep returning `Result<T, String>`, converting at the boundary.
  - The Studio access refusal is rewritten on thiserror.
- **Names.**
  - The Studio access refusal is renamed to say it is an access denial.
  - The chat turn's outcome is renamed to say a fenced block was dropped.
  - The two-way conversion between access refusals and Studio errors is removed. Callers map explicitly.
- **Accepted behaviour changes** (recorded because the deepening umbrella promised none outside its single exception):
  - **Under-13 refusal.** Learners under 13 are refused learning-goal, job-description, document-claim and search AI checks. This is a policy decision; propose an ADR if it should bind future work.
  - **Per-room call chat.** Call chat uses one gossip topic per room. Builds from before and after this change can't exchange call chat. Accepted pre-launch, with no compatibility shim.
  - **AI timeout.** The tutor and Studio model timeout is 600 seconds.
  - **Attempt gate.** It runs when an answer completes, and a failed save is recorded as a storage-error failure.
  - **Cloud module.** It and issuer rotation are kept as prerequisites of the credentials module.
- **Bookkeeping.**
  - The credentials ticket's header moves to done/implemented with a dated closing note.
  - The call-manager ticket gets a dated note recording the human device retest result.
  - The motion spec's Status is reconciled once rollout steps 2 and 4 land.
  - Any of these edits to existing docs follow the workspace rule to propose them before making them.

### Frontend hygiene

- One shared in-app path check is used by the deep-link composable and the deep-link parser.
- One shared helper turns an unknown thrown value into a message.
- One localised-field helper replaces the inbox's near-identical title and body functions.
- The deep-link held target is removed from the composable's production return value.

## Testing Decisions

A good test exercises a module's public interface or the user-visible UI, and asserts outcomes a learner or caller could observe: rows recorded, requests sent, events emitted, text shown, accessible names announced. It doesn't assert private functions, SQL or internal call order. Every story above maps to at least one test at one of these seams.

- **Rust module interfaces**, against an in-memory database with migrations run, plus the existing fake Cloud HTTP server used by the push and Cloud-discovery tests:
  - **Notifications:**
    - Organisations off retires registration, keeps the push preference, and leaves the removal pending until it is acknowledged.
    - Organisations back on re-registers.
    - Announcements under a stale lease are dropped.
    - Count updates carry the acting profile.
    - Resume discovery joins a pass already running and doesn't double-record.
    - Unknown kinds are handled at the boundary.
    - Push error codes are stable.
  - **Credentials:** issuance with a rotated issuer stamps the rotated DID, and no issuance path accepts a bare key.
  - **AI access:** the guardian-side operations round-trip, and a decision by one guardian link can't touch another's request.
  - **Typed errors:** each module's refusal variants are asserted by variant, not by string.
  - Prior art: the existing inline test modules in notifications (push, Cloud discovery, opens), credentials (lifecycle, issuer) and AI access, and the command/direction guard script.
- **Vitest for composables and components:**
  - Notifications composable: the resume handler calls discovery and registration once, and the profile session is respected.
  - Skill atlas composable: refreshes only while visible or on relevant events, indexes by ID, and rejects stale loads.
  - Motion composable: reads tokens, mirrors the device setting, applies before unlock.
  - Atlas explorer and personal atlas components: accessible names, the focus indicator with the inspector collapsed, prerequisite wording.
  - Spinner and button: Reduced keeps the slow rotation, Off is static with a label.
  - Opening and startup: the slow-start line after the delay.
  - Prior art: the existing notifications, course-completion and omni-search composable tests, and the motion tests for the modal, completion modal, opening, sidebar skill graph and atlas explorer.
- **Desktop UI Playwright suite** with its mocked backend, on Chromium and WebKit:
  - Atlas accessibility and the collapsed-inspector indicator.
  - Stable field colours with an added field.
  - A large-graph fixture (about 1,500 skills across all fields, with several thousand edges) panned without long main-thread tasks over 50 ms on Chromium.
  - Reduced and Off modes across the step 2 and step 4 surfaces.
  - Press feedback on non-button controls.
  - The slow-start line.
  - Removed notification deep links do nothing.
  - Prior art: the existing skill-atlas and personal-atlas specs and the reduced-motion transition test.
- **Manual device checks**, recorded as dated notes:
  - Native launch handoff on iOS, Android 12+, Android below 12 and desktop: cold and warm launch in light and dark, with no colour flash and no second splash.
  - The Android channel name in a non-English locale.
  - The call-manager retest.

## Out of Scope

- Native performance acceptance for motion. It remains open under the motion spec.
- The device and provider release checks still open in notifications ticket 06.
- Any Alexandria Cloud change. Push deregistration uses the existing DELETE endpoint, and the FCM payload contract is unchanged.
- Changing wire protocol IDs or network profile values.
- New public deep links for notifications.
- A new animation library, or expressive motion outside opening, onboarding and learning milestones.
- The intermittent call-manager CI test failure on the release branch. It is unrelated to these findings and is handled with the release.

## Further Notes

- **Release order.** The push privacy fix (ticket 01) ships in the next patch release, ahead of every other ticket.
- **Human-only checks** are the native launch handoff, the Android channel locale and the call-manager device retest. They live in a `ready-for-human` ticket (17). Its results are also summarised in this spec's Notes as dated entries.
- **Sub-tickets** live in `docs/review-remediation/`, numbered in risk order:

| Ticket | Scope | Blocked by |
|---|---|---|
| [01](review-remediation/01-push-respects-organisations-opt-out.md) | Push respects the organisations opt-out (next patch release) | none |
| [02](review-remediation/02-profile-bound-notification-events.md) | Profile-bound notification events | none |
| [03](review-remediation/03-discovery-on-mobile-resume.md) | Discovery on mobile resume | none |
| [04](review-remediation/04-localised-notification-text-and-push-errors.md) | Localised notification text and push errors | none |
| [05](review-remediation/05-remove-public-notification-links.md) | Remove public notification deep links | none |
| [06](review-remediation/06-notification-domain-types-and-vocabulary.md) | Notification domain types and vocabulary | 01, 02 |
| [07](review-remediation/07-motion-progress-slow-start-and-tokens.md) | Motion progress, slow start, tokens, preference storage | none |
| [08](review-remediation/08-native-launch-handoff.md) | Native launch handoff | none |
| [09](review-remediation/09-atlas-accessibility-and-stable-fields.md) | Atlas accessibility, honest wording, stable fields | none |
| [10](review-remediation/10-atlas-performance-and-refresh.md) | Atlas performance and refresh policy | 09 |
| [11](review-remediation/11-guardian-ai-requests-through-ai-access.md) | Guardian AI requests through AI access | none |
| [12](review-remediation/12-credentials-signed-by-a-whole-issuer.md) | Credentials signed by a whole issuer | none |
| [13](review-remediation/13-typed-module-errors.md) | Typed module errors | 06, 11, 12 |
| [14](review-remediation/14-press-feedback-and-rollout-step-2.md) | Press feedback and motion rollout step 2 | 07 |
| [15](review-remediation/15-motion-rollout-step-4.md) | Motion rollout step 4 | 14 |
| [16](review-remediation/16-frontend-hygiene-and-bookkeeping.md) | Frontend hygiene and tracker bookkeeping | none |
| [17](review-remediation/17-human-device-verification.md) | Human device verification (`ready-for-human`) | 04, 08 |

## Notes

- 2026-11-03: Created from the standards and spec review of the work merged since the AI chat actions. Seams confirmed: Rust module interfaces, Vitest for composables and components, the desktop UI Playwright suite, and manual device checks for native launch. Scope: every finding, including the judgement-call smells and motion rollout steps 2 and 4.
- 2026-11-03: Decided that the push privacy fix ships in the next patch release, ahead of the rest. Human-only checks moved to ticket 17. Sub-tickets 01–17 created.
