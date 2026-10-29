# Motion design research

Status: reference research supporting the motion specification.

**Researched:** 2026-10-27  
**Scope:** Alexandria's Vue 3.5 + Tailwind 4 interface in Tauri 2 on Windows, macOS, Linux, Android, and iOS; launch, everyday interactions, and the personal skill atlas. Research only; no application changes.  
**Method:** Primary platform/design documentation, standards, library documentation/source, and first-party product accounts. **Documented** identifies source guidance; **local observation** identifies inspected code; **proposal** identifies an Alexandria design decision requiring validation. Product examples are inspiration, not evidence that animation improves learning outcomes. Native applications, source videos, and Alexandria's running UI were not visually or physically tested in this research.

## Recommended direction

**Proposal:** Make Alexandria feel responsive, calm, and encouraging: immediate input feedback, short transitions that explain where content went, and small celebrations tied to actual learning events. Treat opening as a handoff into usable content, not a mandatory logo sequence. Start with existing CSS, Vue transitions, and the existing graph renderer; there is no demonstrated need for another animation dependency.

Priorities for the implementation plan:

1. Make launch and motion preferences reliable before adding decorative effects.
2. Standardize buttons, dialogs, tabs, lists, and asynchronous feedback around a small shared vocabulary.
3. Make the atlas steady, interruptible, and idle when nothing changes.
4. Add localized delight after successful, verified learning events; measure whether it helps rather than assuming it does.

These are proposed priorities, supported by Apple's advice to keep feedback purposeful, brief, optional, and cancellable, and Microsoft's emphasis on responsive motion and fleeting delight. Neither source prescribes an Alexandria timing scale. [Apple HIG: Motion](https://developer.apple.com/design/human-interface-guidelines/motion), [Microsoft: Motion in Windows](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/motion).

## Fit to the inspected checkout

**Local observations at HEAD `420fcca9`:** This is a targeted source inspection, not the parent's full application audit. Other agents may subsequently change these files.

- [package.json](../package.json) already supplies Vue `^3.5.25`, Tailwind `^4.2.1`, Tauri APIs `^2.11.1`, and `force-graph` `^1.51.1`. [main.css](../src/assets/css/main.css) has 150/200/300 ms transition tokens, some `transition: all`, width-based progress transitions, and infinite decorative starfield animations. Its global reduced-motion rule shrinks CSS durations to 0.01 ms; a later rule also disables starfield animation.
- [main.ts](../src/main.ts) awaits locale initialization before mounting. [index.html](../index.html) applies cached theme/background before the module loads, but initially has an empty `#app`. [App.vue](../src/App.vue) initializes profiles, resolves routing and profile settings/account state, then sets `ready`, starts deep-link handling, and calls `show()`. Its initialization branch has a spinner and text; the catch path redirects toward profile selection. It does not present a distinct boot-error state.
- Despite the `show()` comment, [tauri.conf.json](../src-tauri/tauri.conf.json) sets the main window's `visible` to `true`. **Inference:** source alone does not prove a delayed first reveal or a flash-free launch. A native capture must establish what appears before Vue mounts. The inline theme bootstrap also participates in the project's CSP configuration; preserve that protection when changing it.
- [AppModal.vue](../src/components/ui/AppModal.vue) has nested fade/scale transitions and a focus-management composable. [AppButton.vue](../src/components/ui/AppButton.vue) replaces its slot with a spinner while loading. **Proposal:** preserve button dimensions and an accessible action label across pending states; verify nested modal timing and focus during rapid reversal.
- [Skills page](../src/pages/skills/Index.vue) and [SkillGraphModal.vue](../src/components/layout/SkillGraphModal.vue) explicitly use `autoPauseRedraw(false)` and call `zoomToFit(400, 40)` after simulation stops. `useSkillGraphHover.ts` (since superseded by the atlas renderer) interpolates canvas opacity each frame. [SidebarSkillGraph.vue](../src/components/layout/SidebarSkillGraph.vue) has a separate compact renderer. These JavaScript/canvas effects are not governed by CSS transition durations.
- [iOS launch storyboard](../src-tauri/gen/apple/LaunchScreen.storyboard) currently contains a system-background view; [Apple project settings](../src-tauri/gen/apple/project.yml) reference it. Android's [day theme](../src-tauri/gen/android/app/src/main/res/values/themes.xml) and [night theme](../src-tauri/gen/android/app/src/main/res/values-night/themes.xml) extend a MaterialComponents theme without explicit splash attributes. This does not mean Android lacks a system splash.
- The [confirmed atlas specification](skill-graph-atlas-spec.md) already requires a still settled layout, reduced motion, preserved camera/selection, no replayed achievements, and no idle redraw. It also requires **Ready**, not **Locked**, for unearned skills. Recommendations below preserve those decisions and do not endorse older graph semantics still present in code.

## What the platform sources establish

### Apple: continuity and user control

**Documented:** Motion should explain feedback and follow the direction/meaning of the interaction. Apple cautions against unnecessary animation on frequent interactions and asks that people be able to interrupt it. The accessibility HIG specifically recommends reducing automatic/repetitive motion, zooming, scaling, and peripheral movement under Reduce Motion; fades can replace spatial transitions, and springs can be tightened to reduce bounce. [Motion](https://developer.apple.com/design/human-interface-guidelines/motion), [Accessibility](https://developer.apple.com/design/human-interface-guidelines/accessibility).

**Documented:** iOS launch screens should resemble the initial interface, match appearance/orientation, and avoid localized text or promotional branding; macOS does not require a launch screen. Apple recommends restoring prior state. Launch storyboards use basic UIKit classes and cannot connect to app code or custom classes. A Vue animation therefore belongs after the native launch screen, not inside it. [Launching](https://developer.apple.com/design/human-interface-guidelines/launching), [Xcode: Specifying your app's launch screen](https://developer.apple.com/documentation/xcode/specifying-your-apps-launch-screen).

**Proposal:** Align native, pre-Vue, and first Vue backgrounds. Use a single brief content reveal only when useful; restore a reader or atlas without an introductory camera flight. Keep any brand-led welcome in optional first-run onboarding.

### Android and current Material: native launch, retargetable motion

**Documented:** Android 12+ supplies a system splash on cold/warm starts, not hot starts. Its entrance is system-controlled; icon and exit treatments can be customized. Animated icons use AVD resources; the phone recommendation is at most 1,000 ms, with an animation that remains coherent when skipped. Icon duration does not determine how long the splash remains onscreen. The compat API is available for older supported Android versions. These are native facilities, not CSS hooks. [Android: Splash screens](https://developer.android.com/develop/ui/views/launch/splash-screen).

**Documented:** Material's current motion overview describes the M3 Expressive physics system introduced in May 2025 as replacing the earlier easing/duration system. It offers expressive and standard schemes and separates spatial movement from effects such as opacity/color, where overshoot is inappropriate. Springs support interruption and retargeting. [Material: Motion physics system](https://m3.material.io/styles/motion/overview/how-it-works).

**Proposal:** Borrow continuity and restrained spatial response, not blanket bounce. Existing CSS transitions remain a practical web implementation; do not describe Alexandria's cubic-bezier tokens as current Material spring tokens. A true spring should be justified by a gesture/retargeting need, with an independently verified implementation and webview support. An Android native splash animation should yield as soon as the app can usefully display content, never delay it to complete branding.

### Windows / Fluent: context determines movement

**Documented:** Microsoft's motion principles emphasize continuity, consistency, responsiveness, and brief purposeful delight. Its Windows timing examples include 167/250/333 ms, tied to specific uses; these are not universal cross-platform requirements. Fluent recommends quick fades for top-level destinations to avoid suggesting a hierarchy or moving large surfaces unnecessarily. [Motion in Windows](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/motion), [Fluent 2: Motion](https://fluent2.microsoft.design/motion).

**Proposal:** Fade between peer sections; slide a mobile inspector from its actual bottom edge; open/dismiss related surfaces consistently. Do not attempt to animate the operating system's window-opening behavior from CSS. Linux should receive the same reliable web-content behavior, with its window manager left in control of native presentation.

## Launch lifecycle: one handoff, no forced waiting

**Documented terminology:** Android distinguishes process creation (cold), partial reconstruction such as activity recreation (warm), and foregrounding resident UI (hot). It separates time to initial display from time to full display. These definitions are useful measurement vocabulary, not proof that every Tauri platform emits identical lifecycle events. [Android: App startup time](https://developer.android.com/topic/performance/issues/launch-time).

**Proposed Alexandria behavior:**

- **Cold start:** native launch/window background → immediate themed boot surface → resolved safe route → locally usable content. Resolve profile/lock/guardian gates before exposing private content. “Ready” means the destination can accept appropriate input; it does not mean all network synchronization finished. Do not block on an atlas camera animation, update check, P2P connection, or decorative asset.
- **Warm reconstruction:** restore the route, reading position, atlas camera, and selection when still authorized. Treat visual reconstruction as content restoration, not a fresh achievement event. Handle a native splash if the OS presents one; do not append another mandatory splash in Vue.
- **Hot start / resume / desktop reactivation:** preserve the resident view and reconcile stale data quietly. Do not replay entry choreography on focus, visibility changes, or route remount. If policy requires relocking, show the lock surface immediately. Existing desktop second-instance handling already unminimizes/focuses the window in [lib.rs](../src-tauri/src/lib.rs); retain that directness.
- **Deep link:** resolve authorization and the requested destination before any route-specific reveal. Do not briefly expose an unrelated dashboard or another profile's cached graph. The current deep-link initialization order is a constraint to preserve.
- **Slow startup:** keep a truthful visible state and offer recovery when a phase fails. Provisional product threshold: after roughly 2 seconds, replace an unexplained spinner with a named phase or “Taking longer than expected” plus a safe recovery path. This is a proposal, not an OS guideline or a mandatory minimum display time. Report measured progress only when the backend supplies it.
- **Offline startup:** show locally available learning content and an offline/reconnecting state once local prerequisites are ready. A remote failure should not masquerade as endless local initialization. An invalid network profile or locked database still requires its actual error/gate; “offline” must not bypass it.
- **Failure before Vue mounts:** an in-Vue retry panel cannot handle that failure. Define a separate native/static fallback or diagnostic route appropriate to the actual failure mode. Do not hide the only window indefinitely while awaiting a frontend event that may never arrive.

**Documented Tauri constraint:** The official splashscreen tutorial demonstrates coordinating frontend/backend completion and hiding/showing two windows; its sleeps deliberately simulate work. It is an example, not a requirement to add timers or a second webview. [Tauri: Splashscreen](https://v2.tauri.app/learn/splashscreen/).

**Proposal:** Prefer the existing main surface unless measurements justify a separate desktop splash. Verify platform APIs, capabilities, native mobile resources, and failure handling before borrowing the tutorial. A page-level transition can only cover the period after the webview is capable of rendering it. Any change under generated Android/Apple projects must survive the repository's regeneration/build workflow.

## Accessibility is a behavior contract

**Documented:** WCAG 2.2 SC 2.3.3 is **AAA**: nonessential interaction-triggered motion can be disabled. SC 2.2.2 is **A**: automatically starting moving/blinking/scrolling content that lasts more than five seconds alongside other content generally needs pause/stop/hide, unless essential. The criteria address different triggers; reduced-motion CSS alone is not a blanket conformance claim. [Normative WCAG 2.2](https://www.w3.org/TR/WCAG22/#animation-from-interactions), [Understanding 2.3.3](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html), [Understanding 2.2.2](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html).

**Documented:** `prefers-reduced-motion: reduce` signals a preference to remove or replace problematic motion. `MediaQueryList` exposes changes, enabling JavaScript consumers to respond while the app runs. WebKit recommends alternative indicators when removing motion would remove meaning. [CSS Media Queries 5](https://www.w3.org/TR/mediaqueries-5/#prefers-reduced-motion), [CSSOM View: MediaQueryList](https://www.w3.org/TR/cssom-view-1/#the-mediaquerylist-interface), [WebKit: Responsive Design for Motion](https://webkit.org/blog/7551/responsive-design-for-motion/).

**Proposed policy:** A shared preference with **System** (default) and **Reduced** lets users reduce motion even where native settings are not reliably surfaced. Sample before any entry animation, subscribe to changes, and use the resolved value in CSS, canvas, camera helpers, scroll behavior, and any native custom animations. Recheck on resume. Verify OS-to-webview propagation on every platform; do not assume Android animator controls or Linux desktop settings map perfectly to this query. Android's native `areAnimatorsEnabled()` is a separate platform API, not proof of WebView CSS behavior. [Android ValueAnimator](https://developer.android.com/reference/kotlin/android/animation/ValueAnimator).

In Reduced mode, replace camera flights with direct positioning, settle layout without visible drifting, remove press scaling/bounce/parallax/ambient loops, and replace moving progress decoration with labeled status. Use immediate changes or a short opacity change where comfortable. Preserve highlight, selection, text, earned checkmarks, and every action. Switching the preference mid-animation should settle safely into the current requested state.

**Documented:** WCAG SC 4.1.3 (AA) requires qualifying status messages to be programmatically identifiable without moving focus. [Understanding Status Messages](https://www.w3.org/WAI/WCAG22/Understanding/status-messages.html). **Proposal:** success/error/progress text remains after decorative motion; announce meaningful status politely, avoid frame-by-frame announcements, and reserve urgent alerts for genuinely urgent failures. Keep focus stable on routine updates and restore it appropriately when dialogs close. Never make an invisible outgoing control remain an accidental keyboard/pointer target.

## Proposed interaction vocabulary and initial values

These are **Alexandria tuning candidates**, not extracted platform requirements. Start from the existing 150/200/300 ms scale; tune on devices before broad rollout.

- **Press/selection:** immediate visible acknowledgement; optional 100–150 ms local release response. Touch/pen may warrant a slightly more tactile response than pointer hover. Keep keyboard focus immediate and persistent. Avoid making text, rows, and icons all bounce on every click.
- **Menus, tooltips, dialogs:** 150–200 ms fade with at most a small 4–8 px contextual offset; exit 100–150 ms. Preserve the modal focus contract. Larger mobile bottom sheets may use up to 250–300 ms for meaningful travel and must respond to interruption.
- **Peer navigation:** 150–200 ms fade of the destination content while persistent shell/navigation remain stable. Parent/detail movement is optional only where the hierarchy and return path are clear. No stagger across an entire page of controls.
- **Lists/search:** use short insertion/removal or reorder feedback only where tracking the change helps. Preserve stable IDs, scroll anchoring, and input focus. Rapid typing should not queue a succession of result animations. Do not animate every item on every refresh.
- **Atlas focus/reset:** 200–300 ms maximum candidate for deliberate camera navigation, zero in Reduced mode; cancel/retarget on a new selection or manual pan/zoom. Do not refit after every simulation stop, resize, or credential update when a valid user camera exists.
- **Achievement:** one localized 300–500 ms accent around the changed skill or checkmark after an authoritative earned/level improvement event. Keep the current earned Bloom number/name and check visible. Do not replay on bootstrap, profile unlock, remount, or sync of historical achievements. Revocation/downward correction receives accurate status, not celebration.
- **Pending → success/error:** acknowledge the action immediately, keep the action's identity/space, and show pending until the real result arrives. Success may add a single small check transition; error gets readable explanation and retry/undo where supported. No fake success before durable acceptance, punitive shaking, or blocking celebration. Distinguish local save from remote synchronization when both exist.

**Proposal for easing:** use a decelerating curve for entry/settling and a short accelerating exit where appropriate; retain the current standard curve for small state changes until compared on devices. Share semantic roles such as feedback, surface, navigation, and achievement, rather than scattering literal durations. Expressive spring motion is a later, specific enhancement, not a prerequisite for polish.

## Vue and rendering: practical implementation boundaries

**Documented:** Vue's `<Transition>` supports conditional elements/components and CSS or JavaScript hooks. `mode="out-in"` delays insertion until leave completes. Nested timing can require explicit durations; enter cancellation exists, while leave cancellation is only available with `v-show`. JavaScript-only hooks require completion callbacks. [Vue: Transition](https://vuejs.org/guide/built-ins/transition.html).

**Proposal:** Use reusable transition presets around the actual changing surface. Avoid a global `out-in` sequence that makes each route wait or resets persistent state. If explicit durations are used, reduce those alongside CSS; a visually instant animation must not leave a timer holding removal/focus. Clean up on unmount and superseding intent, not only in a leave-cancelled hook. Domain completion, unlocking controls, and recording success must not depend on `transitionend`.

**Documented:** `<TransitionGroup>` supports list insertion, removal, and reordering, requires unique keys, applies classes per item, and has no transition modes. [Vue: TransitionGroup](https://vuejs.org/guide/built-ins/transition-group.html). **Proposal:** use it for bounded DOM lists where movement helps tracking; it does not animate force-graph canvas nodes. Avoid index keys, giant entrance cascades, or animating every virtualized row mount.

**Documented:** Google's rendering guidance favors transform/opacity and advises investigating properties that trigger layout/paint. `will-change` should be used sparingly after identifying a problem, not applied permanently across the interface. [web.dev: High-performance CSS animations](https://web.dev/articles/animations-guide).

**Proposal:** Replace broad `transition: all` with selected properties. Reserve space for pending/content states. Profile progress width, shadows, blur, and large translucent overlays rather than assuming GPU acceleration. Do not transform the entire application wrapper: it risks changing positioning/hit-testing relationships around overlays and dense canvas UI. For custom frame loops, calculate progression from elapsed time, retain a cancellation handle, and stop on teardown; the platform provides frame timestamps and cancellation. [HTML Standard: Animation frames](https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#animation-frames).

**Documented graph caveat:** `autoPauseRedraw(true)` stops idle redraw after simulation halts; `pauseAnimation()` freezes rendering **and interaction**, while `resumeAnimation()` restores both. `warmupTicks` dry-runs layout before render, and cooldown settings stop simulation. Camera methods accept transition durations. These capabilities are documented upstream and in the installed package README. [force-graph API](https://github.com/vasturiano/force-graph#readme).

**Proposal:** Preserve stable atlas coordinates, keep automatic idle redraw enabled where possible, and explicitly schedule finite redraw work for hover/achievement effects. Simply changing the current flag can truncate per-frame opacity interpolation, so verify hover completion and repaint after theme/selection changes. Do not pause an interactive visible graph merely to implement Reduced mode. Hidden/unmounted graphs can pause or dispose; visible settled graphs must still accept clicks, keyboard-equivalent controls, and zoom. Large synchronous warmups can move jank into startup: benchmark realistic graph sizes before using them as a cure.

## Concise inspiration board

These are first-party descriptions, not recordings observed during this task. Transfer the interaction technique, not colors, logos, characters, proprietary assets, or claims of effectiveness.

- **Apple — gesture-consistent, brief feedback.** The HIG describes revealing/dismissing surfaces in directions that make sense and allowing cancellation. **Transfer:** the skill inspector follows its edge and yields to the next selection; returning from a lesson preserves place. [Apple Motion](https://developer.apple.com/design/human-interface-guidelines/motion).
- **Material — spatial movement versus effects.** The current system separates spring-based movement from non-overshooting color/opacity effects. **Transfer:** an optional tactile sheet response with steady text/selection colors, not a bouncy atlas camera. [Material motion system](https://m3.material.io/styles/motion/overview/how-it-works).
- **Fluent — a quiet top-level transition.** Fluent explicitly recommends quick fades for peer destinations. **Transfer:** Library, Skills, and other peer sections can feel connected without moving the entire study workspace. [Fluent Motion](https://fluent2.microsoft.design/motion).
- **Duolingo — reactions driven by real exercise state.** Its engineering/art account describes a character state machine that responds to correct/incorrect outcomes and stops speech animation when an exercise is completed early. **Transfer:** success feedback follows actual accepted results and is interrupted when the learner continues. Borrow the state-driven behavior, not the mascot or an assumption that Alexandria needs Rive. [Duolingo: How we animate world characters](https://blog.duolingo.com/world-character-visemes/).

## Verification and unresolved questions

**Documented compatibility boundary:** Tauri uses WebView2 on Windows, the device's Android WebView, and WebKit-family engines on macOS/iOS/Linux; capabilities depend on installed runtime/OS versions. One Chromium browser preview cannot establish parity. [Tauri: Webview Versions](https://v2.tauri.app/reference/webview-versions/).

**Proposed real-device acceptance work for the implementation owner:**

- Capture packaged launches on Windows, macOS, a supported Linux distribution, Android, and iPhone: cold, warm reconstruction, hot resume, second-instance activation, deep links, offline, slow local storage, and failed initialization. Include light/dark and custom app theme versus OS theme. Verify no blank flash, duplicated splash, forced minimum wait, or exposure across profile locks.
- Exercise all input modes actually supported: mouse/trackpad, keyboard, touch, screen reader. Test rapid open-close-open, back during navigation, repeated submit, search while loading, resize/rotation during a sheet transition, and profile switch during graph loading. The latest valid intent wins; no stale callback restores old content.
- Toggle native motion preferences before launch and while open, including Android disabled animator settings and the chosen Linux desktop setting. Confirm the webview observes changes; test the in-app Reduced override independently. Check static status/achievement meaning and graph interaction, not merely short CSS durations.
- Trace startup milestones separately: native process/window, webview content, locale/profile readiness, first usable route, and optional network completion. Collect repeated cold/warm samples and report median plus tail latency with device/runtime/build details. Do not impose a cross-device millisecond promise before baselining.
- Profile frame pacing on a slower Android device and older supported Apple hardware, plus a high-refresh display. Inspect layout/paint work, input response during animation, and CPU after the atlas settles. Check both small and realistically large graphs and long translated/RTL labels. Proposed success criterion: no continuous decorative or canvas work when a surface is settled/hidden; controls remain responsive throughout.
- Verify reduced-motion and rapid interruption with component tests at public behavior boundaries. Use native recordings and runtime traces for launch, perceived smoothness, and idle cost; unit tests alone cannot establish those properties.

**Limits / decisions still to validate:** No measured startup or frame-time baseline was collected. OS preference propagation, native launch appearance, exact Tauri lifecycle integration, generated-project persistence, and screen-reader behavior remain device checks. The inspected code predates any subsequent atlas integration work. Apple HIG pages were read through their official Markdown representations where the web reader returned JavaScript-only shells; Material's motion overview was available through indexed first-party content. This report avoids claiming current Linear implementation details or adding a runtime based on marketing examples. All numerical Alexandria timing/recovery proposals require validation; none is a conformance guarantee or a reason to make people wait.
