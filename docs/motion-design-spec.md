# Motion and visual polish

Status: code implemented and reviewed; native acceptance open

## Confirmed direction

Make actions feel immediate and tactile, preserve spatial context, and reserve
expressive motion for opening, onboarding and meaningful learning milestones.
Keep the existing indigo identity, warm light surfaces and deep dark surfaces.
The accompanying research explains the platform and accessibility constraints.

- Opening: points assemble the existing geometric mark, its branches unfold,
  and it arrives at the destination's brand mark. The complete choreography
  takes at most 1.2 seconds, with no minimum display time. When local content
  is usable it accepts input immediately; remaining motion stays at the logo
  and cancels on interaction. Initialization never awaits animation.
- Only a fresh process/webview launch gets the opening. Foregrounding, restoring
  a resident window, unlocking a profile and ordinary navigation do not replay it.
- No profiles leads to onboarding; existing profiles lead to selection/unlock.
  Preserve account creation/restoration, recovery, role, age/guardian, lock,
  incompatible-data and deep-link requirements. Never animate private content
  out after locking; remove it immediately.
- Settings offer System, Reduced and Off, saved on the device. Respect the OS's
  reduced-motion request. Apply the same resolved policy before first paint and
  to CSS, imperative animations, graph motion, scrolling and celebrations.
- Brief press/selection feedback and contextual surface transitions are shared.
  Preserve pending control size, accessible name, focus, scroll and media state.
  New input wins; superseded animation cannot trigger a domain action.
- Small wins remain inline. Only course completion receives a larger, immediately
  dismissible celebration. Never imply pending/rejected credentials are earned,
  replay old achievements on hydration, or duplicate feedback across surfaces.
- Use the same visual idea on desktop and mobile, adapting native handoffs and
  effect complexity. Avoid duplicate OS/webview splashes and background flashes.
- Decorative motion settles; hidden surfaces stop owned animation work. Reading,
  answering, recording and routine synchronization remain calm. Preserve all
  information in Reduced/Off modes, including progress and error feedback.

## Implementation sequence

1. Shared policy, settings, tokens, controls, opening, onboarding and profile flows.
2. Persistent navigation, search, lists, home/goals/catalog and daily learning.
3. Existing personal atlas, learning feedback and completion presentation.
4. Remaining instructor, chat/live, settings, guardian and administrative surfaces.
5. Verification, independent standards/spec review, fixes and commit.

The atlas's existing Ready semantics, credential-backed earned Bloom, stable
layout and profile-safe state remain authoritative. No renderer replacement,
credential-policy change, new backend authority or animation dependency is needed.

## Verification

Exercise public behavior: preference persistence/OS changes/cancellation;
mounted startup, profile/onboarding and controls; mounted atlas/learning feedback;
and browser journeys for layout, focus, interruption and reduced motion.
Use red/green cycles at confirmed boundaries, targeted tests/typechecks during
implementation, then the full frontend suite and production build once complete.
Also run applicable i18n/CSP/native formatting and lint checks.

Browser validation covers Chromium/WebKit, light/dark, compact layouts and
keyboard/touch-equivalent interactions. Record native validation separately:
Windows WebView2, macOS/iOS WKWebView, Linux WebKitGTK and Android WebView,
including cold/warm/resume, native motion preferences and app/OS theme mismatch.
Do not equate browser coverage with native startup or performance verification.
There must be no forced launch dwell or continuous idle decorative animation.

Native-device performance is an acceptance boundary, not an optional follow-up.
Use release builds and isolated audit profiles on physical hardware. Record the
commit, OS/webview version, refresh rate, display scale, power/thermal state and
trace settings. Compare System against Off and the review baseline on the same
device; repeat at least ten cold launches and ten interaction sequences. Include
fresh onboarding, returning-profile unlock, dialog/search/navigation, atlas
exploration, completion, minimize/resume and a 30-second idle window. Capture:

- Process launch to usable controls (median/p95), separating initialization from
  decoration; animation must add no readiness gate.
- Input-to-present latency and frame pacing/hitches at the actual refresh rate;
  investigate any added stall above 100 ms or p95 above two frame intervals.
- CPU, GPU and memory during the opening and after settling; no owned decorative
  loops may continue at idle or when hidden. Separate backend/network activity.
- OS Reduced Motion and device Off: readable final states, no spatial effects,
  no second native/webview opening and no replay after resume.

Use Instruments on Apple platforms, Perfetto on Android, and native platform
traces on Windows/Linux. Browser animation counts and simulator results are
supplementary evidence. Missing physical targets remain explicitly unverified;
never substitute desktop emulation for mobile power/GPU measurements.

## Evidence

Source `7db4c17f`: 450 frontend tests and 59 browser checks passed, with one
existing WebKit touch-synthesis skip. Strict typechecking, locale parity,
raw-text, CSP and backend-command checks passed. Independent reviews found no
remaining code defects or documented-standard violations; small pre-existing
atlas presentation duplication remains a nonblocking judgment call.

Physical Mac audit: Apple M4 Pro, 48 GB, macOS 27.2 (26B118), WebKit
22625.2.8.11.9, 120 Hz display at 2x scale, AC power, Low Power Mode disabled,
nominal thermal state before capture. The optimized release executable ran in
an ad-hoc audit bundle with isolated test data. This is not installer validation.

- System and Off each completed ten fresh-process launches and ten welcome/role
  round trips. Launch-to-accessibility-observation medians were 781 ms and 773 ms;
  nearest-rank p95 values were 939 ms and 811 ms. The same ten-launch audit of
  baseline `827a687d` measured 929 ms median and 1549 ms p95. These are automation
  upper bounds with warm OS caches, not instrumented startup or input-to-present
  times. The ordered, small samples do not establish a general startup speedup.
- The System idle sample used 0.02 CPU seconds in 30.62 seconds for the app
  process (0.065% of one core). Its WebContent/GPU/network processes showed no
  increase at the CPU counter's precision. Off showed no measurable increase
  in any of those processes over 30.72 seconds. CPU counters do not measure
  GPU utilization. Both modes had zero running web animations after settling.
  The baseline retained 13 infinite web animations and used 6.90 CPU seconds
  across the same app/WebKit process roles in 30.75 seconds (22.44% of one core).
  This comparison covers the fresh-onboarding screen, not authenticated work.
- Native OS Reduce Motion resolved System to Reduced with no running web
  animations; restoring the OS setting resolved back to full motion live.
  Minimize/resume delivered hidden/visible events without replaying the opening.
- Targeted Instruments Hitches captures are readable. Correlating compositor
  swaps with the app's commits gives frame-lifetime p95 of 40.81 ms for System
  and 40.73 ms for Off, compared with 40.71 ms in the baseline. These are pipeline
  [frame lifetimes](https://developer.apple.com/documentation/xcode/understanding-hitches-in-your-app),
  not inter-frame spacing or measured input-to-present latency;
  they do not close the two-frame criterion. Corresponding GPU durations have
  p95 of 1.35 ms, 1.07 ms and 1.55 ms; no correlated stage exceeded 100 ms.
  Compositor work is shared with other desktop activity, so this does not
  establish exclusive app GPU cost or a frame-pacing pass.

Mobile preparation at source `0a2fa78e` (motion code unchanged from `7db4c17f`):
arm64 iOS release compilation, development signing and wireless installation
succeeded on the physical iPhone 17, iOS 27.2 (24B119). The separate
`org.alexandria.node.motionaudit` app preserves the installed regular app and its
data. Android arm64 release compilation and APK packaging also succeeded, with
the same isolated package ID, release minification, local test signing and
shell profiling enabled; the APK is not debuggable. Both use their production
mobile video features. Packaging used the repository Tauri CLI and Java 21;
the older global CLI omitted Android features, and Java 25 failed with the
project's Gradle version. Those failed attempts are not validation evidence.
Signed baseline `827a687d` artifacts are also prepared for both mobile targets
under the same audit IDs and release configuration. Android packaging uses
preserved, successfully compiled release libraries for both versions, skipping
the redundant Gradle Rust rebuild. Baseline apps have not been installed.

Native acceptance remains open. Returning-profile unlock, authenticated
atlas/completion journeys, first-paint theme handoffs and other physical targets
remain unverified. The iPhone's developer service can install the audit app,
but Instruments still reports the phone offline. The OnePlus is currently
absent from ADB discovery and its previous wireless addresses are unreachable.
No mobile runtime measurements or Windows/Linux physical-device results are
claimed. Build, signing and installation checks do not satisfy performance
acceptance.
