# Alexandria motion and visual polish plan

Status: superseded by [the implemented motion specification](motion-design-spec.md); retained as design history.
Prepared: 2026-10-27. Source audit: `420fcca9`.
Companion: [primary-source research and inspiration](motion-design-research.md).
Interactive studies: [motion and opening](motion-design-preview.html) and [fresh-install / existing-profile entry flows](onboarding-motion-preview.html). These show illustrative states, not a working app or approved redesign.

## Outcome and design direction

Make every action feel acknowledged, every transition explain a change, and every meaningful achievement feel earned. Alexandria should feel like a carefully made learning instrument: calm while reading, tactile while interacting, and briefly expressive at milestones.

Keep the existing indigo identity, warm light surfaces, deep blue dark surfaces, editorial headings, and geometric brand mark. Polish their application rather than introduce a new visual identity. Use three levels of expression:

1. **Everyday feedback:** fast, small, local. Buttons settle under a press; selection follows intent; fields communicate focus and validation; pending actions retain their identity.
2. **Spatial continuity:** panels emerge from their edge; related content changes in place; navigation preserves orientation. Persistent navigation and study surfaces stay stable.
3. **Signature moments:** the brand mark resolves at opening; a newly earned skill receives a single accent; course completion gets a short, dismissible celebration. These moments finish and leave useful information behind.

“Everywhere it makes sense” means auditing every interaction, including intentionally motionless ones. Typing, reading, timers, recording indicators, assessment work, and frequent background synchronization must remain calm. No ambient floating cards, repeated list cascades, compulsory logo waits, punitive error shaking, or invented achievement states.

This direction adapts brief, interruptible feedback from [Apple](https://developer.apple.com/design/human-interface-guidelines/motion), quiet peer navigation from [Fluent](https://fluent2.microsoft.design/motion), the distinction between spatial motion and opacity/color effects from [Material](https://m3.material.io/styles/motion/overview/how-it-works), and outcome-driven reactions from [Duolingo](https://blog.duolingo.com/world-character-visemes/). These are design inspirations, not evidence of improved learning outcomes.

## What exists and where the work belongs

| Inspected surface | Current foundation / gap | Planned work |
| --- | --- | --- |
| [main.css](../src/assets/css/main.css) | Theme, radius, focus and 150/200/300 ms transition tokens exist; broad transitions and looping starfield effects coexist | Semantic motion roles, explicit properties, shared surface/spacing rules and static settled states |
| [AppButton](../src/components/ui/AppButton.vue) | Shared variants; loading replaces its content with a spinner | Preserve width and accessible name; press/release and pending/result states |
| [AppModal](../src/components/ui/AppModal.vue), [useModalFocus](../src/composables/useModalFocus.ts) | Nested fade/scale transitions and established modal focus management | One coordinated enter/exit contract; interruption and reduced-motion timing; keep existing focus protections |
| [AppTabs](../src/components/ui/AppTabs.vue), [AppSwitch](../src/components/ui/AppSwitch.vue) | Tabs switch classes; switches already move | Consistent selection movement, keyboard semantics, RTL and disabled states |
| [App.vue](../src/App.vue), [main.ts](../src/main.ts), [index.html](../index.html) | Empty pre-mount root, locale wait, spinner, profile gates and late `show()` call | Visible early boot shell, explicit lifecycle/failure states, nonblocking opening handoff |
| [Tauri config](../src-tauri/tauri.conf.json) | Main window is configured visible despite late `show()` comment | Measure native launch; reconcile window reveal and background behavior |
| [useTheme](../src/composables/useTheme.ts) | Reads `alexandria-theme-pre-unlock`; early HTML reads older `alexandria-theme` | Unify bootstrap/cache contract, including default/System behavior and CSP hashes |
| [AppLayout](../src/layouts/AppLayout.vue), [OmniSearch](../src/components/omni/OmniSearch.vue) | Persistent shell, immersive route exceptions, existing transitions | Quiet route changes, responsive menus/search, stable focus and scroll |
| [Player](../src/pages/learn/Player.vue), [CourseCompletionModal](../src/components/course/CourseCompletionModal.vue) | Lesson transitions/progress and rich completion effects already exist | Consolidate and tune; avoid adding a second celebration system |
| Personal graph surfaces and `useSkillGraphHover` (superseded by the atlas renderer) | Current baseline has per-frame interpolation and continuous redraw in large views | Consume the atlas integration's stable state; time-based finite effects and shared preference |

The [atlas spec](skill-graph-atlas-spec.md) is already an independent implementation stream. Re-audit its merged result before graph work; this plan must not reimplement its renderer or revert its decisions. All unearned skills remain **Ready**, earned Bloom retains its number/name, and prerequisite relationships and credential authority remain unchanged.

The parent audit visually inspected the existing Home light fixture and onboarding dark fixture using Playwright. Those fixtures use stubbed data and do not exercise full native startup. Other findings above are source observations, not claims about measured live behavior.

## Shared motion contract

Introduce a small policy module such as `src/composables/useMotion.ts`, a framework-independent token/preset module such as `src/utils/motion.ts`, and CSS tokens in the existing stylesheet. Names are proposed seams, not mandatory abstractions. CSS handles ordinary states; Vue Transition/TransitionGroup handles presence and bounded list changes; JavaScript is reserved for interruptible camera/gesture/measurement needs. Keep the existing dependency set for the first rollout.

### Timing and movement candidates

These are Alexandria starting values, not accessibility requirements or platform constants. Tune them on devices before spreading them across the app.

| Role | Initial values | Treatment |
| --- | --- | --- |
| Input acknowledgement | By next rendered frame | Focus/pressed/selected state changes immediately; no delayed activation |
| Press release / micro feedback | 100–140 ms | Optional scale 0.98 → 1 on a compact actionable surface; never whole-page scaling |
| Selection / toggle / small overlay | 160–200 ms | Position or color settles once; effects do not overshoot |
| Dialog / drawer / sheet | Enter 220–280 ms; exit 140–180 ms | Desktop dialog 4–8 px offset or very small scale; mobile sheet from bottom edge |
| Peer destination | 120–180 ms | Content fade, stable shell; no directional implication between unrelated sections |
| Deliberate atlas focus | 220–300 ms | Anchor the selected node; cancel on pan, zoom, another selection or reduced mode |
| Earned feedback | 320–480 ms | Local check/halo settles once; milestone choreography may last up to 650 ms |
| Opening | Up to 1,200 ms for the full expressive sequence | Readiness or input can end it sooner; zero minimum dwell |

Candidate ease-out: `cubic-bezier(.2,.8,.2,1)` for entry/settling; use a short accelerating exit and the existing standard curve for subtle color changes. Avoid simulating springs with repeated scale keyframes. A real retargetable spring is a later option for a specific drag interaction, not a prerequisite.

At most one focal animation should compete for attention. If a completion dialog is open, defer the underlying graph accent rather than animate both. Limit entrance staggering to a few decorative/supporting elements, with a total offset budget of about 80 ms; labels, controls, and accessible content are available immediately.

### Preference and cancellation

- Offer **System** (default), **Reduced**, and **Off** in appearance/accessibility settings. System follows the effective OS preference. Reduced uses immediate changes or opacity-only transitions up to 100 ms. Off uses immediate state changes. No setting overrides an OS request for reduced motion with full motion.
- Add a device-scoped `ui.motion` preference through the existing [settings registry](../src-tauri/src/settings/registry.rs); follow the established theme pattern for a minimal pre-unlock hint. Profile settings hydrate only when available. Reset profile-dependent animation/event state on lock. Decide and test the hint's lifetime explicitly so boot does not default to expressive motion before reading the preference.
- CSS and JavaScript read the same effective policy. Listen for native/media-query changes, recheck on resume, and cancel existing effects into their latest requested final state. Verify Android/Linux OS propagation; add a small native adapter only where actual device testing shows a gap.
- Reduced/Off remove travel, scale, graph flight, auto-scroll animation, ambient loops and confetti. Status text, selection, focus, progress values and earned markers remain. The current global 0.01 ms CSS rule is a fallback, not the complete policy.
- New input supersedes old animation. Unmount, lock, profile change, route change and hidden surfaces cancel owned work. Never place domain mutations, unlocks, focus restoration, or success persistence exclusively in an animation callback.
- Existing modal focus/scroll-lock behavior remains authoritative. Coordinate nested leave timing; exiting content cannot receive stray clicks/Tab focus. Do not use a global `out-in` route wrapper that adds sequential waiting, remounts media, or replays achievements.

## Opening sequence on every platform

### Creative concept: ideas connect, possibilities unfold

The opening should have a recognizable visual idea and a satisfying arrival. Everyday interaction timings remain short; this signature moment gets a distinct choreography. The revised launch values here supersede the earlier restrained timing candidates in the research note.

Use the existing [geometric Alexandria mark](../src-tauri/icons/alexandria-icon.svg). Six scattered points converge on its vertices while the outline connects. The inner stem and branches draw into place, an indigo tile forms behind them, and the complete mark travels to the real header icon as the destination appears. The movement ties the opening to Alexandria's connected-learning identity instead of presenting a disconnected logo fade. Keep the constellation small and finite; it does not represent real profile data or earned skills.

Suggested full sequence, **up to 1,200 ms** after the first themed frame:

- **0–430 ms — connect:** six points gather; the geometric perimeter starts drawing; one soft pulse establishes the focal point.
- **250–780 ms — form:** the stem/branches unfold, the existing indigo tile resolves, and the wordmark appears briefly. These stages overlap rather than queue.
- **760–1,200 ms — arrive:** the complete mark scales/moves into its actual header position while destination content appears with a small 10 px settle. No new logo is invented.

Use measured source/target rectangles for the docking movement, not fixed screen coordinates. If the resolved destination has no matching mark (for example a profile picker without a brand header), use a brief local dissolve into that safe destination; do not add chrome just to justify the animation. Cancel on resize/orientation changes rather than dock to a stale location.

This is a maximum choreography envelope, not a startup dependency. Content readiness or input can cut directly to the final UI at any stage. An already ready route must not acquire a forced splash. In production the interactive destination becomes usable as soon as authorization/local readiness allows; any residual decorative overlay must yield. A slower boot settles to a static completed mark and truthful status rather than looping. Reduced/Off mode reveals the destination directly with all status meaning intact. No replay on hot resume, sound, remote launch asset, or personalized data before unlock.

The revised interactive study explicitly plays the complete sequence, includes a slow-motion inspection option, and supports tap/Escape/Skip, live reduced-motion cancellation, resize cancellation and repeat replay. Its timer is only for judging choreography and does not prescribe production readiness. The demo docks into its illustrative header; that is not evidence of native launch behavior.

### State model and privacy

Use explicit stages: native presentation → themed boot shell → local initialization → resolved safe destination → interactive. Each stage can fail independently. Animation follows stage changes and cannot cause or delay them.

- The boot shell must paint before locale/profile initialization can stall it. Avoid adding a second theme/preference implementation; share the bootstrap contract and preserve CSP.
- Resolve incompatible data, onboarding, profile picker, guardian gate and locked/failed-lock branches before exposing protected content. Do not transition old private content out when locking; remove it immediately.
- Resolve queued cold-start deep links through the existing authorization path. Do not display Home as an intermediate animated destination. Warm deep links navigate without replaying branding.
- Define readiness as local usability of the correct route. P2P connection, remote catalogs, update checks and optional graph layout are not launch-animation prerequisites.
- After about 2 seconds of unresolved startup, show a truthful named phase or a localized “Taking longer than expected” state. This is an initial UX threshold to test, not a forced delay or fake percentage. Offer safe retry/relaunch only where it cannot race ongoing profile work. Failures need a persistent recovery path.
- Handle failures before Vue mount separately through an early static/native fallback and diagnostics. An App.vue error panel cannot catch bundle, locale-bootstrap or native setup failure before it mounts. Do not hide the sole window while waiting indefinitely for a frontend event.

### Platform behavior

| Entry/platform | Proposed behavior and implementation boundary |
| --- | --- |
| macOS / Windows / Linux cold process launch | First native window matches the neutral/themed boot surface; one Vue opening handoff. Audit [Tauri config](../src-tauri/tauri.conf.json), [native entry](../src-tauri/src/lib.rs), early HTML and theme code. Let the OS own window presentation. Prefer one window over a separate splash webview. |
| iOS cold launch | Keep [LaunchScreen.storyboard](../src-tauri/gen/apple/LaunchScreen.storyboard) a static approximation of the first UI, following Apple launch guidance. Match system appearance/safe areas; no code-driven animation or localized branding in the storyboard. Any brief brand treatment happens after the webview renders and cannot extend the native launch. Resolve custom app-theme mismatch with a neutral native handoff if required. |
| Android cold/warm reconstruction | Coordinate system SplashScreen/icon exit with the first themed webview frame. Audit [MainActivity](../src-tauri/gen/android/app/src/main/java/org/alexandria/node/MainActivity.kt), day/night resources, manifest and build dependencies. Avoid a second full Vue logo sequence after a native animated icon. Do not change native visibility/hold behavior just to complete the animation. |
| Reopen resident desktop window / mobile foreground | Restore the current view immediately; no ceremonial replay on focus/visibility. If security requires relock, show that gate immediately. |
| Reconstructed activity/webview | Restore authorized state where available; one restrained content reveal, not a new achievement or forced intro. |
| Browser development/preview | Same Vue behavior and motion preference, guarded native APIs. This does not declare a separate production web/PWA target. |

“Every launch” means every actual application entry gets a considered handoff; repeatedly foregrounding a resident reader should not interrupt it. Native resources must survive the project's regeneration/build workflow. Validate early-vs-profile theme choices across system/app disagreement rather than promise cross-platform splash parity from CSS alone.

## Interaction coverage and visual polish

Every row needs normal, pending, success, failure, disabled, Reduced and Off behavior where applicable. Carry changes into both shared primitives and bespoke controls; search results, forms and plugin-host buttons often bypass the shared button component.

| Area and code anchors | Motion and interaction treatment | Visual polish |
| --- | --- | --- |
| Buttons, links, inputs, switches: [UI primitives](../src/components/ui/) | Immediate pressed/focus state; subtle release; stable pending button width/name; checked state follows accepted local action; errors explain recovery | Normalize heights, icon weights, spacing and focus visibility; enlarge touch hit areas without bloating dense desktop controls |
| Navigation: [AppSidebar](../src/components/layout/AppSidebar.vue), [MobileTabBar](../src/components/layout/MobileTabBar.vue), [ModeSwitcher](../src/components/layout/ModeSwitcher.vue), [AppLayout](../src/layouts/AppLayout.vue) | Active indicator moves locally; peer destination fades; sidebar collapse retains selected item; account/mode changes show actual destination | Consistent selected/hover/focus styles, quiet separators, explicit learner/instructor/guardian identity; preserve native drag regions and mobile safe areas |
| Search/menus/dialogs: [OmniSearch](../src/components/omni/OmniSearch.vue), [AppModal](../src/components/ui/AppModal.vue), [InfoTip](../src/components/ui/InfoTip.vue), [usePanel](../src/composables/usePanel.ts) | Small origin-aware reveal; no animation queue on typing; cancel stale results; coordinated focus/escape/backdrop behavior | Aligned menu spacing, readable secondary labels, consistent elevation, anchored popovers and reachable sheet actions |
| Profiles/onboarding: [ProfileSelect](../src/pages/ProfileSelect.vue), [Onboarding](../src/pages/Onboarding.vue), [Starfield](../src/components/auth/Starfield.vue) | Selected tile can become the unlock panel; short step continuity; shorten ambient movement to one settling reveal, then static | Preserve centered hierarchy, tighten form spacing, clear password/loading/error identity; retain progress labels and avoid moving recovery phrases |
| Home/courses/goals: [Home](../src/pages/Home.vue), [course pages](../src/pages/courses/), [goal components](../src/components/goals/) | Local card press, short filter/list updates, same-size skeleton-to-content replacement; one meaningful goal-added acknowledgement | Preserve editorial heading + indigo hero; harmonize card padding/radii, thumbnails, empty/error states and one clear next action; compare populated and empty layouts |
| Learning/assessment: [Player](../src/pages/learn/Player.vue), [AssessmentRunner](../src/pages/learn/AssessmentRunner.vue), [course components](../src/components/course/) | Stable answer selection; accepted progress update; short lesson change preserving media/editor state; no ornamental motion while answering | Stable text measure, readable progress labels, consistent feedback panel, clear action hierarchy; no effects obscure explanations or correctness |
| Milestones/credentials: [CourseCompletionModal](../src/components/course/CourseCompletionModal.vue), [Credentials](../src/pages/dashboard/Credentials.vue), [useCourseCompletion](../src/composables/useCourseCompletion.ts) | Refine existing celebration; local skill accent for small wins; larger bounded treatment only for course completion; dismiss/continue immediately | Credential identity, issuer/status and next action stay readable; distinguish course completion, pending issuance and accepted credential; never celebrate pending/rejected as earned |
| Skill atlas: [atlas spec](skill-graph-atlas-spec.md), personal skills/sidebar/modal | Preserve camera and settled positions; anchored focus, contextual inspector, one authoritative new-earned accent; stop finite redraw work | Theme-aware field colors, readable earned Bloom/name/check, consistent inspector/chip surfaces; integrate after atlas work lands |
| Instructor tools and classroom: [instructor](../src/pages/instructor/), [classrooms](../src/pages/classrooms/) | Save/publish/review show truthful pending and result; bounded reorder animation where reorder exists; no delayed destructive action | Consistent editor toolbars, review panels, row density and save status; keep drafts and selection stable |
| Chat, tutoring, interviews: [chat](../src/components/chat/), [tutoring](../src/pages/tutoring/), [interviews](../src/pages/interviews/) | New message reveal only near the live edge; preserve scroll when reading history; quiet connection state; immediate mic/camera control feedback | Stable video tiles, legible captions, clear persistent recording/privacy indicators; do not animate or remount media surfaces for cosmetic transitions |
| Settings, network, sync, plugins, opinions, governance, guardian, errors | Consistent disclosure/pending/result patterns; distinguish saved locally from synced remotely; static truthful counters | Shared form/row patterns, coherent status badges, readable long dialogs, localized recovery actions; no celebratory financial/trust-state implication |

For continuous controls, keep scrubbing/dragging directly attached to input; animate only the release if useful. Native file dialogs, OS permission prompts and third-party plugin contents retain their own behavior. The host's open/pending/error boundaries can be polished without injecting animation into plugin documents.

### Visual-system pass

- Consolidate surface hierarchy: background → card → overlay. Use existing theme tokens, subtle borders and consistent shadows. Fix contrast before adding glow; avoid animated full-window blur or shadows.
- Use a small spacing rhythm (existing 4/8 px units), aligned control baselines, consistent 8/12/16 px radius roles where appropriate, and a limited type hierarchy. Preserve script-specific fonts, long labels and Urdu/RTL; animations follow logical inline direction.
- Make button loading occupy the same footprint; reserve media/skeleton aspect ratios. Retain accessible names and `aria-busy`/status semantics instead of replacing the action with an unlabeled spinner.
- Keep focus rings immediate and unobscured. Use roughly 44 px comfortable touch targets; validate WCAG target-size rules rather than claim that one pixel rule guarantees conformance. Audit text contrast at every animated state, not only endpoints.
- Add static, useful empty/offline/error visuals using existing iconography. No looping empty-state illustrations. Do not animate counts from zero on every visit or fake skeleton progress.
- Review global body color transitions during theme switching; prefer coherent immediate surface changes or a short controlled fade. No circular theme wipe or moving whole-window theme reveal.

## Delivery sequence

These are implementation-sized increments, not PRs created by this planning task. Each should be independently reviewable and shippable. Foundation precedes adoption; native launch and atlas work have their own prerequisites.

| Increment | Scope / dependencies | Exit criteria |
| --- | --- | --- |
| 0. Baseline and inventory | Capture five-platform startup, representative flows and motion/accessibility gaps; inspect merged atlas status | Recorded builds/devices, route/control checklist, startup milestones, idle graph cost and frame traces; no invented performance baseline |
| 1. Foundation and specimen fixture | Motion tokens/policy/settings, pre-unlock resolution, explicit CSS properties, representative control/overlay fixture | System/Reduced/Off agree across CSS/JS; changing mode mid-effect settles; no new runtime dependency; i18n/settings checks pass |
| 2. Shared controls and visual system | Increment 1; buttons/fields/tabs/switches/dialogs/menu surfaces; static typography/spacing/focus/contrast pass | Pending controls do not resize/lose names; repeated open-close works; keyboard/RTL/touch checks pass; no focus/scroll-lock regressions |
| 3. Opening across platforms | Increment 1 + native baseline; early boot/theme, stage/error handling, native resource handoffs | All five packaged platforms: correct safe route, no duplicate/forced splash, no theme flash, no private content, recovery from slow/failing startup; native regeneration checked |
| 4. Navigation and daily learning | Increment 2; shell/search/profiles/Home/catalog/goals/player/assessment | Stable shell, scroll/media preservation, responsive search, calm reading/assessment; loading/error/empty variants covered |
| 5. Atlas and milestones | Increment 1/2 + completed atlas integration; authoritative event semantics and existing completion modal | No idle decorative work; direct reduced camera; no feedback on initial hydration, remount, old sync or revocation; valid new event celebrates once |
| 6. Remaining surfaces and release verification | Increments 2/4/5; instructor/chat/live/settings/admin/guardian/plugin-host coverage | Every route/control category classified, platform QA evidence complete, measured performance/accessibility gates met |

First implementation slice: the motion policy plus a specimen containing a button, switch, tabs, dialog/sheet and opening handoff. This establishes the feel at low cost before screen-by-screen adoption. Bring those primitives to the real app next, then ship the platform launch work as a coherent increment.

## Acceptance and measurement

### Functional / accessibility

- Test the public behavior of the effective motion policy and changes during animation; avoid tests that only repeat duration constants.
- Test rapid open-close-open, repeated submit, back during navigation, typing through async results, profile lock/change during pending work and a second milestone event. Latest valid intent wins; stale callbacks do not resurrect content.
- Verify keyboard focus, screen-reader status and dialog containment/restoration throughout transitions, including immediately after Reduced/Off. Make disappearing surfaces inert appropriately. Visible selection and result text must convey all meaning without motion or color.
- Test long localized labels, RTL, 200% text/zoom where supported, short desktop windows, mobile keyboard, safe areas, orientation changes and touch without hover. Include no profiles, multiple profiles, guardian gate and incompatible data.
- Test completion semantics at existing authoritative event boundaries: pending/rejected/revoked credentials, local completion awaiting issuance, initial hydration, duplicate delivery and genuine newly accepted achievement. Do not change trust policy for visual convenience.
- Automatically starting ornamental motion settles within 5 seconds; this plan prefers under 650 ms for ordinary effects and up to 1,200 ms for the full opening. Audit existing ticker/starfield/celebration loops against WCAG rather than claim Reduced mode alone satisfies pause/stop requirements. Focus and announcements never wait for ornamental motion.

### Performance and platform gates

Proposed engineering targets, to be calibrated after baseline:

- **Zero forced launch dwell.** Added assets/initialization must not materially regress first usable route; investigate a paired median or p95 regression greater than 5% and distinguish noise before accepting it. Capture at least 20 cold and 20 warm runs on designated reference devices; report build/runtime and dispersion.
- Typical interaction animations aim for smooth display-rate delivery; at 60 Hz the total frame interval is 16.7 ms. Examine p95 frame intervals, long tasks and dropped frames on a slower Android phone plus a high-refresh device. No new >50 ms main-thread task attributable to choreography in representative traces.
- After visible effects settle, **zero continuous decorative animation loops**. Canvas should redraw for actual interaction/data/theme changes, not remain active just to maintain a static picture. Pausing must not disable visible graph interaction.
- Prefer transform/opacity; profile intentional small width/height changes. No permanent `will-change` on every card, large animated blur, remote startup assets, or global transform of the app wrapper.
- Browser regression coverage runs Chromium and WebKit, both themes, reduced motion and compact layouts. Native packages run Windows WebView2, macOS WKWebView, supported Linux WebKitGTK, Android WebView and iOS WKWebView. Include OS/app theme disagreement and native motion settings changing while open.
- Preserve and extend [desktop UI validation](desktop-ui-validation.md). Planned checks: focused Vitest behavior tests, `vue-tsc -b --noEmit`, frontend tests/build, desktop UI suite, i18n parity/raw-text checks and CSP checks when bootstrap/style assets change. Rust/native edits require applicable formatting/lint/tests and packaged-device verification. Do not run unrelated heavyweight checks for this documentation-only plan.

Roll out incrementally using the shared policy/presets as the rollback seam. A problematic flourish can be disabled without changing actions, data, focus or navigation. Record real task usability: does the user locate the next action, track where a panel went, and continue without waiting? Animation count is not a success metric.

## Planning evidence and remaining uncertainty

Delivered in this task: source audit, primary-source research, a written implementation plan and a local interactive motion study. No application animation, native resource or dependency was changed. The study's completion and startup states are simulated; its values are illustrative.

Study validation: previewed at 728 px dark and 360 px light; checked completion/progress semantics, keyboard tab selection, resetting a pending result, opening completion, Reduced mode, live OS preference changes and horizontal overflow. No page-script errors were observed in the Chromium interaction checks. The revised opening also passed static-poster/no-idle-work, full sequence, repeated replay, tap/Skip/Escape, live reduced-motion, resize cancellation and narrow-layout checks. The docked mark aligned with its measured header target within one CSS pixel. Local document links and whitespace checks passed. These checks validate the study, not application accessibility or native animation performance.

Still required during implementation: five-platform native startup recordings, measured baselines, actual OS-motion propagation, assistive-technology checks, populated production-like data, generated native resource persistence and review against the final atlas integration. Exact timings and the small amount of brand expression remain tuning decisions, not externally mandated values. Default to the direction above unless user review changes it; no further discovery interview is required to start the first implementation slice.

## Entry-flow study

The companion entry-flow study follows the inspected onboarding sequence: Welcome → roles → birthdate → password/account creation → recovery backup → goals → skill bootstrap → completion. Restore omits the new recovery-backup step; selecting the parent role adds child linking. Existing profiles start at the picker, then a selected avatar moves into the unlock screen; Add profile enters onboarding with a path back to the picker. The opening docks into the welcome mark on a fresh install and the header mark on the picker.

The walkthrough uses fixed adult/sample identity fields, hidden recovery-word placeholders, local-only fake pending results, and sample profiles. It does not perform authentication, credential issuance, file upload, profile mutation, or native biometric access. Child/guardian activation, localization, actual recovery and error handling remain production flows, not established by this visual study. Source references: [Onboarding.vue](../src/pages/Onboarding.vue), [ProfileSelect.vue](../src/pages/ProfileSelect.vue), [ProfileTile.vue](../src/components/profile/ProfileTile.vue).

Checked the full eight-step learner walkthrough, restore and parent variants, both opening destinations, picker/unlock/back, conditional biometric button, adding a profile, cancellation on scenario changes, reduced motion during pending setup, and desktop/mobile layout. These are prototype checks, not application test results.
