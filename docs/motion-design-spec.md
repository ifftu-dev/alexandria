# Motion and visual polish

Status: implementation in progress

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

Frontend verification: 446 tests passed; browser verification: 51 passed with
one existing WebKit touch-synthesis skip. Strict typechecking, locale parity,
raw-text and CSP checks passed. Native release compilation and performance
capture are in progress. A paired iPhone is currently unavailable to Instruments;
no Android physical device is connected. These targets remain unverified.
