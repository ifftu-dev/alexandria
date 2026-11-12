# Sentinel — Assessment and Interview Integrity

Status: live
Updated: 2026-11-12

> On-device integrity system for Alexandria's learning, assessment, and interview surfaces. Biometric and behavioural data is processed on-device and not persisted — only derived scores (0-1), anomaly flags and verification results are stored locally. The single exception is learner-consented **assessment appeal** evidence: on a *flagged assessment* session, and only after the learner is shown the flag and explicitly chooses to keep it, the flagged snapshots' evidence is retained on-device for 14 days so they have something to contest with. Interview-purpose sessions never enter this evidence-staging path. Declining, or not answering, persists nothing. Three paths reach a server, and the learner starts each: releasing evidence to contest an assessment flag (see [Review and adjudication](#review-and-adjudication)), the summary of a [candidate integrity check](#candidate-integrity-check) they agreed to run for an organisation's interview — counts and a score, never evidence — and, under a sponsor's `llm` probe policy they consented to, the skill and the text of the options they chose (see [Probes](#probes)).

**What Sentinel cannot see.** Monitoring cannot see a second person, in the room or helping remotely, or a second device out of view. No signal, profile or rung changes that, and every [coverage statement](#coverage-statements) says so. Sponsors who need that assurance add their own interview; an interview held in Alexandria can add a [live segment](#live-segment).

Vocabulary (trust profile, verification rung, coverage statement, and the rest) is defined in the repo [GLOSSARY.md](../GLOSSARY.md). The design record is [sentinel-trust-profiles-spec.md](sentinel-trust-profiles-spec.md).

## Design Principles

1. **Privacy-first** — All behavioral data (keystrokes, mouse movements, video frames, microphone audio) is processed entirely on-device. Only numeric scores, categorical flags and verification results are stored in the local database.
2. **Non-punitive by default** — Sentinel informs rather than punishes. A single warning or critical flag sends a session to `review`, never to a verdict. `flagged` needs two independent channels over a certified threshold or a failed verification step, and only the backend-recorded assistant flags suspend. Interview-purpose findings do not suspend or end the live interview workflow.
3. **Measured before it counts** — Every signal is advisory until a reviewed certification admits it with a rate measured on the [measurement corpus](#measurement-and-certification). An advisory signal can send a session to review; it contributes nothing to the fusion that decides `flagged`. No certification is in force yet, so every signal is advisory today.
4. **On-device ML only — backend-resident.** All ML runs in the Rust backend. The paste classifier, the YuNet face detector and the MiniFASNet liveness model use `tract` (pure-Rust ONNX inference) with weights embedded at compile time via `include_bytes!`; no command loads replacement weights. The per-user keystroke autoencoder, mouse-trajectory CNN and gaze calibration train + score via `candle` (Apache-2.0, HuggingFace). The face embedder remains pure-pixel LBP math, and voice checks are signal processing with no model. The frontend collects raw events and per-window aggregates and forwards them over Tauri IPC; scores and flags are derived in Rust.
5. **Incremental trust** — Behavioral profiles build over time. New users start with generous defaults; consistency scoring activates after 10+ samples. Months of practice and lessons count through [longitudinal consistency](#longitudinal-consistency).
6. **Evidence is the learner's to release, and to take back** — A flag may travel; the evidence behind it may not, unless the learner releases it themselves. Release is learner-initiated: a review console may not offer a control that asks for it, because a request that can be refused leaks the refusal. What is released stays theirs to withdraw, and deleting it on their own device destroys the copy they sent. See [Review and adjudication](#review-and-adjudication).
7. **Honest coverage** — A session runs one of four fixed [trust profiles](#trust-profiles), and the credential states what that profile checked and what it did not. Capability absence is never treated as clean, and hardware raises the floor while [verification rungs](#verification-rungs) raise the ceiling on any device.

## Architecture Overview

```
┌── Frontend (Tauri WebView, Vue 3) ─────────────────────────────────┐
│  useSentinel() — a collector, not a scorer                         │
│    keystroke / pointer / focus / clipboard buffers                 │
│    response tracker (utils/sentinel/responseProcess.ts)            │
│    camera tick: raw RGBA frame over binary IPC                     │
│    LBP face embedder (pixel math, local)                           │
│    collectFeatures() → WindowFeatures per snapshot window          │
└─────────────────────────────────────┬──────────────────────────────┘
                                      │ Tauri IPC
                                      ▼
┌── Backend (src-tauri/, crate alexandria-node) ─────────────────────┐
│  alexandria_studio::integrity  (Tauri-free, re-exported as         │
│    sentinel::scoring): derive_window → flags, fusion, profiles,    │
│    rungs, coverage statements, measurement (corpus, evaluator)     │
│  sentinel::snapshots      commit windows, decide session status    │
│  sentinel::trust_profile  selection, lock, degradation, floor      │
│  sentinel::verification   probe / interview results in the chain   │
│  sentinel::consistency    longitudinal consistency at grading      │
│  sentinel::surface        assertion facts, sponsor review queue    │
│  sentinel::camera_pipeline + inference_worker (YuNet, MiniFASNet)  │
│  sentinel::voice_activity voice onsets, second voice (DSP)         │
│  sentinel::companion      paired attested phone (iroh ALPN)        │
│  sentinel::attestation    App Attest, Play Integrity               │
│  commands/sentinel_ml     paste classifier, keystroke AE, mouse CNN│
│                                                                    │
│  SQLite (sqlcipher): integrity_sessions, integrity_snapshots,      │
│    integrity_verifications, sentinel_user_models                   │
└────────────────────────────────────────────────────────────────────┘
```

All processing happens client-side. There is no server-side component in the application itself — Alexandria is a fully decentralized desktop and mobile application. Integrity scores are stored locally in SQLite; the current implementation does not publish Sentinel snapshots over P2P. A commercial review queue may later receive *scores* for a flagged session, bounded by [Review and adjudication](#review-and-adjudication); processing never moves off the device.

## Signal Taxonomy

The client sends raw per-window features (`WindowFeatures`: counts, means, the latest native probe reading, model outputs); the backend derives every score and flag from them (`alexandria_studio::integrity::derive_window`). The composite weights below build the descriptive `integrity_score`; they no longer decide a session's outcome (see [Fusion and session outcome](#fusion-and-session-outcome)).

| Signal | Source | Type | Composite weight | Description |
|--------|--------|------|--------|-------------|
| `typing_consistency` | Rule | 0-1 | 0.20 | Deviation of the window's dwell, flight and speed from the stored typing profile |
| `mouse_consistency` | Rule | 0-1 | 0.15 | Velocity deviation from stored mouse profile |
| `is_human_likely` | Rule | bool | 0.15 | Velocity variance check (bots have constant speed) |
| `tab_switches` | Rule | count | 0.15 | Tab focus changes during assessment (webview) |
| `app_switch` | Native | event | — | OS frontmost-app change (NSWorkspace / GetForegroundWindow / X11); emitted on `sentinel://focus` |
| `paste_events` / `pasted_chars` | Rule | count | 0.10 | Clipboard paste activity |
| `face_present` / `face_count` | Rule | bool/int | 0.15* | Face checks while the camera runs (* camera profiles only) |
| `ai_keystroke_anomaly` | AI | 0-1 | 0.05† | Autoencoder reconstruction error |
| `ai_mouse_human_prob` | AI | 0-1 | 0.05† | CNN human vs bot classification |
| `ai_face_similarity` | AI | 0-1 | 0.05† | LBP histogram cosine similarity |
| `ai_face_match` | AI | bool | — | Whether face matches enrollment (drives `face_mismatch` flag when false) |
| `ai_paste_anomaly` | AI | 0-1 | 0.05† | ONNX classifier — probability snapshot is paste / typing-bot / LLM-paste-edit. Drives `paste_classifier_anomaly` (≥0.95) and `paste_classifier_critical` (≥0.99) flags. |
| `gaze_offscreen_ratio` | AI | 0-1 | — | Fraction of camera ticks the learner's gaze was estimated off-screen (look-away detection). Drives `gaze_wander` (>0.30), `device_glance` (≥3 downward glances), `gaze_occluded` (>0.40 occluded). See §Gaze. |
| `display_count` / `external_display` / `split_screen` / `screen_captured` | Native | int/bool | — | Display arrangement via `sentinel_display_topology` (Tauri monitors on desktop; `UIScreen` + window bounds on iOS; `DisplayManager` + multi-window on Android). Sampled at start, per snapshot window, and on `sentinel://display` nudges (window moved / scale changed). Android also reports a cumulative `native_transitions` counter so a Split View toggle between two samples is still seen. See §Platform coverage. |
| `foreign_accessibility_services` / `adb_enabled` / `obscured_touches` | Native (Android) | int/bool | — | `sentinel_android_environment`: enabled non-system accessibility services, debug-bridge state, and touches delivered while another window was drawn over ours. The assessment shield (`FLAG_SECURE` + `setHideOverlayWindows`, API 31+) is engaged only while an assessment element is current. The obscured-touch counter only counts while the shield is up and is drained once at session start, so a session never inherits taps from before it began. |
| `hidden_overlays` / `overlay_windows_scanned` | Native (desktop) | int | — | `sentinel_hidden_overlay`: windows another process excluded from screen capture (`kCGWindowSharingState`, `GetWindowDisplayAffinity`), X11 override-redirect windows, Windows click-through topmost windows; minus our own windows and an allowlist of password managers and OS shell surfaces. |
| `camera_device_virtual` | Rule | bool | — | The camera track's label matches a known virtual-camera product. The label itself is shown in the dev view only and never persisted. |
| `liveness_real_prob` / `liveness_checks` / `liveness_spoof_checks` | AI | 0-1 / int | — | MiniFASNetV2 presentation-attack score on the YuNet face box, computed inside `sentinel_score_gaze` on trigger ticks. A tick with `real_prob < 0.5` counts as a possible spoof. See §6 Liveness. |
| `watched_processes` / `watched_categories` | Native (desktop) | int/list | — | `sentinel_process_scan`: watchlist matches by category (`interview_cheat`, `remote_desktop`, `virtual_camera`, `virtual_machine`, `ai_assistant`, `screen_share`). The full process list never leaves the backend. |
| `global_hotkeys` / `phantom_hotkeys` | Native (desktop) | int | — | `sentinel_hotkeys_*`: listen-only modifier-combo monitor. Records only key-downs with Cmd/Win/Super, Ctrl or Alt held — never plain keys, Shift-only combos, or key-ups. A phantom is a combo the OS saw that the webview never received while our window was focused, excluding OS-owned shortcuts. Assessment purpose only; starts only if the OS already granted listening. |
| `response.items[]` | Rule | per-visit aggregates | — | Response-process timing per item visit: time to first input, time to commit, revisions, idle bursts, answer changes after idle. See [Response-process signals](#response-process-signals). |
| `gaze_down_then_response` / `voice_then_response` | Rule | count | — | Cross-modal: a response within 3 s of a gaze-down or a second-voice cue, one per cue |
| `longitudinal_divergence` | Backend (grading) | verdict | — | The outcome sits well above what the learner's history on the skill predicts. See [Longitudinal consistency](#longitudinal-consistency). |

† Added to the composite only when advisory AI scoring is toggled on (see §Runtime Toggle).

The weights are relative coefficients, not fixed percentages of the composite.
The backend divides the weighted sum by the sum of coefficients actually included,
then clamps the result to [0, 1]. The five base rule terms sum to 0.75; camera
presence adds 0.15 when the window carries camera checks. Available enabled AI
terms add 0.05 each. Missing optional signals contribute neither a score nor a
denominator term.

**Retired score inputs.** A snapshot request is `{ session_id, element_id, features }`. Any other field — client-computed scores, client flags, the composite, the developer-tools term — is refused before anything is written ("client-computed integrity scores are no longer accepted; refresh the client"). Historical snapshots and their stored outcomes are preserved, not rescored. Developer-tools detection is retired as both a score input and a misconduct rule; the legacy nullable `devtools_score` column stays for old rows.

## Rule-Based System (Active)

### Client-Side collector (`useSentinel.ts`)

- **Activation**: Starts when a learner opens the course player (enrolled), begins any assessment attempt (standalone, with no enrollment), an instructor starts a consented interview with Sentinel enabled, or a candidate agrees to an organisation's integrity check before joining an interview held in Alexandria. Interview monitoring observes the conductor device, preferring its interviewer participant and falling back to the candidate in a candidate-only plan.
- **Start sequence**: `integrity_start_session` → `integrity_attest_session` → optional companion pairing → `integrity_select_profile` (see [Trust profiles](#trust-profiles)) → first snapshot.
- **Snapshot interval**: Random 15-45 seconds. Each window `collectFeatures()` builds a `WindowFeatures` and submits it; nothing is scored in the webview.
- **Profile storage**: the behavioural baseline is saved through `sentinel_save_behavioral_profile` per user and device
- **Profile update**: Exponential Moving Average with alpha=0.2 (alpha=0.5 during training wizard)
- **Consistency scoring**: Activates after `sampleCount > 10`

The calibration wizard's training metrics keep small TypeScript analyzers; they never reach a snapshot.

### Flagging Logic

Per-window rules in `alexandria_studio::integrity::derive_window`. Every threshold is carried over from the retired client scorer and is unmeasured until the corpus certifies it.

1. Composite below 0.40 → `low_integrity` warning (channel `composite`: derived from the other scores, never independent evidence)
2. Consistency score below 0.35, or keystroke autoencoder anomaly ≥ 0.65 → `behavior_shift` warning
3. Excessive tab switching (> 10) → `tab_switching` info
4. Excessive pasting (> 500 chars) → `paste_detected` warning
5. Bot-like mouse variance, or mouse CNN human probability < 0.5 → `bot_suspected` critical
6. Face absent (camera running) → `no_face` info
7. Multiple faces → `multiple_faces` warning
8. Face identity mismatch (enrolled) → `face_mismatch` critical
9. Prolonged absence (5+ consecutive checks) → `prolonged_absence` warning
10. Frequent absence (>50% of checks in snapshot window) → `frequent_absence` info
11. Paste classifier ≥ 0.95 → `paste_classifier_anomaly` warning
12. Paste classifier ≥ 0.99 → `paste_classifier_critical` critical
13. Gaze off-screen ratio > 0.30 → `gaze_wander` warning
14. ≥ 3 downward off-screen glances in the window → `device_glance` critical
15. Gaze-occluded ratio > 0.40 (eyes hidden while the camera runs) → `gaze_occluded` warning
16. Native OS focus moved to another application → `app_switch` warning (distinct from the webview-only `tab_switching`)
17. Second display attached → `external_display` info
18. Display arrangement changed mid-session (monitor plugged, mirroring toggled, Split View entered, or the Android transition counter advanced) → `display_change` warning
19. App shares the screen (iPad Split View / Slide Over / Stage Manager, Android multi-window / picture-in-picture), including a toggle between two samples → `split_screen` critical
20. Screen being recorded or mirrored (iOS `isCaptured`; desktop duplicate monitor geometry) → `screen_captured` critical
21. Non-system accessibility service enabled (Android) → `foreign_accessibility_service` warning
22. USB or wireless debugging enabled (Android) → `debug_bridge_enabled` warning
23. Touch delivered while another window overlaid ours (Android) → `obscured_touch` critical
24. Capture-excluded, override-redirect or click-through overlay window from another process (desktop) → `hidden_overlay` critical
25. Camera is a known virtual-camera product → `virtual_camera` warning
26. Interview cheat tool process running (desktop) → `cheat_tool_process` critical
27. Remote-desktop host, virtual-camera driver or VM guest agent running (desktop) → `unauthorized_process` warning
28. Desktop AI assistant running → `ai_assistant_running` info
29. Screen-share or meeting app running → `screen_share_running` info
30. ≥ 1 phantom hotkey in the window (desktop) → `phantom_hotkey` warning
31. ≥ 3 phantom hotkeys in the window (desktop) → `phantom_hotkey_repeated` critical
32. More than half of ≥ 3 liveness ticks in the window scored as a possible spoof → `spoof_suspected` warning
33. Alexandria's AI chat or lesson tutor reached the backend while the attempt was open → `in_app_assistant_during_attempt` critical. Recorded by the backend, never by a client: the request is refused, and the flag is written into the session's committed snapshot chain once per session, carrying the session's current score so the average is unchanged.
34. A connected external (MCP) assistant called while the attempt was open → `mcp_call_during_attempt` critical, recorded the same way.
35. The locked profile runs the camera and the window carried no camera checks → `camera_degraded` warning (added by the backend, see [Lock and degradation](#lock-and-degradation))
36. A first visit's time to first input below the item's reading floor → `response_below_reading_floor` info
37. Every first visit in the window carries a uniform added lag of 3 s or more over the learner's baseline → `response_relay_lag` info
38. Idle for 20 s, then the answer arrives in a burst → `response_idle_burst` info
39. An answer changed straight after an idle stretch → `answer_change_after_idle` info
40. A response within 3 s of a gaze-down → `gaze_down_then_response` info
41. A response within 3 s of a second voice → `voice_then_response` info
42. At grading, the attempt outcome diverges from the learner's history → `longitudinal_divergence` info (not a window flag; see [Longitudinal consistency](#longitudinal-consistency))

Flag severity and channel are authoritative on the backend (`alexandria_studio::integrity::flags`). Unknown flags default to info and to a channel of their own, so client/server version skew never escalates a session. Signals 36–42 replace two flags [ai-chat-spec.md](ai-chat-spec.md) §13A had planned: `impossible_speed` became `response_below_reading_floor`, and `practice_assessment_divergence` became `longitudinal_divergence`.

### Fusion and session outcome

Signals fall into independent evidence **channels**: two signals in one channel are one piece of evidence. The channels are `typing`, `pointer`, `clipboard`, `focus`, `display`, `device_environment`, `overlay`, `process`, `hotkey`, `camera_presence`, `camera_coverage`, `camera_identity`, `gaze` (including `gaze_down_then_response`), `response_process`, `voice`, `composite`, `assistant` and `longitudinal`.

Each window a signal fires adds its certified log-likelihood ratio to its channel, and a channel whose total reaches the profile's threshold is over threshold. A signal with no certified ratio adds zero and is listed as advisory. The ratios come from a certification (`sentinel::scoring::active_manifest()`); none is in force, so today no channel is ever over threshold and every signal is advisory.

The decision rule (`alexandria_studio::integrity::decide`):

- **Suspended**: a tamper flag (`in_app_assistant_during_attempt`, `mcp_call_during_attempt`), recorded only by the backend. An interview-purpose session is set to `flagged` instead.
- **Flagged**: a failed verification step (a failed probe), or two channels over threshold.
- **Review**: any warning or critical flag, or one channel over threshold. Info flags never send a session to review.
- **Active**, then **completed** once it ends: anything else.

Severity no longer counts towards an outcome: the old "1 critical or 3 warnings" and "2 critical" rules are retired, and the composite only matters through the `low_integrity` warning. The per-flag trust penalty is gone.

**Where it runs:** the backend re-decides status from every committed flag on each `integrity_submit_snapshot`, under the session's locked profile (sessions that never selected one are scored as `core`). Status only moves up. A `suspended` or `completed` session refuses further snapshots; `review` and `flagged` keep accepting them, since neither is a final verdict. `integrity_end_session` takes no client scores: an `active` session becomes `completed` and every other status stands. Severity counters stay denormalized on `integrity_sessions.critical_count` / `warning_count`, and per-snapshot `anomaly_flags` persist as JSON on `integrity_snapshots.anomaly_flags`. Each submit returns the status it left behind as `session_status`, plus the window's `consistency_score` and the fusion summary (`FusionSummary`, not persisted). Once the app sees `suspended` it stops sending snapshots, tells the learner (live indicator and assessment header), and at submit ends the session without a final snapshot; grading still runs.

Grading is the one place a finished session is decided again: `longitudinal_divergence` is known only once the attempt is scored, so both grading paths re-run the decision with it before issuance, and the status can still only move up.

An interview-purpose session follows the same rule but is never `suspended` and Sentinel does not stop the room. Its final row is review context; it does not gate the interview assistant or make a hiring decision.

### Platform coverage and ceilings

The native signals above are per-OS `#[cfg]` modules under `src-tauri/src/sentinel/`; a platform without a source reports the signal as absent, never as clean.

| Capability | Windows | macOS | Linux X11 | Linux Wayland | iOS | Android |
|---|---|---|---|---|---|---|
| Response-process signals | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Display topology | ✓ | ✓ | ✓ | ✓ (count only) | ✓ | ✓ (+ transition counter) |
| Split-screen | – | – | – | – | ✓ | ✓ |
| Screen capture / mirroring | heuristic | heuristic | – | – | ✓ | prevented (`FLAG_SECURE`) |
| Hidden-overlay scan | ✓ | ✓ | – | – | n/a | prevented (`setHideOverlayWindows`, API 31+) |
| Process watchlist | ✓ | ✓ | ✓ | ✓ | – | – |
| Phantom hotkeys | ✓ | ✓ (needs Accessibility) | ✓ | – | – | – |
| Accessibility services / debug bridge | – | – | – | – | – | ✓ |
| Device attestation (`device_attested`) | companion phone | companion phone | companion phone | companion phone | ✓ App Attest | ✓ Play Integrity (needs response keys) |
| Join a desktop session as companion phone | – | – | – | – | ✓ | ✓ |

Wayland is a stated ceiling: the compositor exposes neither the windows nor the keys of other clients, so only the display count is available there. X11 is the same ceiling for overlays — override-redirect alone is how every tooltip, menu and IME popup is drawn, and X11 has no capture-exclusion or click-through flag to read — while the process watchlist and phantom hotkeys still apply. Mobile prevents overlays and capture where it can rather than detecting them; desktop detects because it cannot prevent. Everything in this table except attestation is client-reported; a desktop reaches `device_attested` only through a companion phone (see §Integrity Assurance).

### Sentinel Dev live panel

An unlocked app can open the Sentinel Dev panel with Command–Shift–S
(Control–Shift–S on other desktop platforms), or the Sentinel Dev button in the
standalone assessment runner. It displays the existing monitoring session's
telemetry without entering diagnostics or consuming the attempt. It is available
in release builds. **Start camera preview** explicitly requests local camera
access and displays face/gaze overlays. In a standalone skill assessment it also
drives the session's face/gaze checks; stopping or closing releases that camera
opt-in. Outside an assessment, scores show unavailable and the panel explains
that monitoring is idle. Preview-only frames do not enter the assessment evidence
staging buffer. Profile lock closes the panel and stops capture.

The native-check rows (Android environment, overlays, processes, hotkeys) show
— until the session's first snapshot. After it, a check that returned nothing
shows n/a rather than a clean zero: the probe failed, or the platform does not
provide it. The Android environment report runs only on Android, the process
scan only on desktop, and the overlay scan and hotkey monitor only on macOS,
Windows, and Linux with an X11 display. Each reading names the probe that
produced it (for example `cgwindow`, `nsworkspace`, `x11`). Hotkeys show —
until the hotkey monitor has replied, and only assessment sessions ask it. A
session the backend has suspended shows as suspended rather than active.
Starting a session clears the previous session's readings.

On macOS, trusted main-frame capture uses the normal WebKit/OS permission prompt.
Plugin frames still require the host's recorded media grants. Capture and
inference failures are shown in the panel; late permission results after closing
are stopped rather than leaving a hidden camera running.

Standalone assessment submission freezes answers while monitoring is live, then
persists a final snapshot and ends the session before grading. A failed final
snapshot or session teardown prevents issuance and allows retry with the frozen
answers. See [the assessment demo setup](demo/setup.md).

### Diagnostics transition

The release workflow uses explicit diagnostics entry/exit from the profile menu;
the normal desktop menu contains no always-visible developer submenu. Entry
during an active assessment saves fixed-form selections (adaptive answers are
already persisted per turn), ends Sentinel, and only then asks the backend to
end the attempt and enable diagnostics. A save or cleanup error leaves
diagnostics disabled and presents retryable feedback.
Diagnostics itself adds no misconduct flag or penalty and does not assign a
failing grade to the unfinished attempt. That attempt still counts toward the
assessment's normal attempt limit and cooldown because its questions were shown.

The backend refuses entry while a linked integrity session is unfinished, marks
open attempts with a distinct `ended_at` / `end_reason = diagnostics` state, and
rejects later answer, grading, or finalization calls for those attempts. While
the process-local mode is active, a persistent banner exposes the supported
reload, web-inspector, Sentinel live-view, and CLI-install actions. Exit, profile
lock, or application restart disables the mode; exit unmounts diagnostic views
and closes open web inspectors. CLI-management commands also enforce the mode.

Neither hiding developer tools nor offering diagnostics makes a user-controlled
device tamper-proof. Existing backend authorization and IPC capability checks
remain required in either mode.


## Trust profiles

A trust profile is the fixed set of signals a session runs. The set is small and fixed so sessions stay comparable and the measurement matrix stays finite. Logic: `alexandria_studio::integrity::{select_profile, evaluate_floor}`; app side: `sentinel::trust_profile`.

| Profile | Signal sets | Needs |
|---|---|---|
| `core` | Core signals: behaviour, focus, clipboard, display, overlay, process, hotkey, response process, backend-recorded assistant flags | Nothing. No camera inference and no compute budget, so it runs on every device |
| `attested` | Core + `device_attestation` | An attestation that verifies: iOS, Android with response keys, or a desktop with a [companion phone](#companion-phone) |
| `camera` | Core + `camera` | A camera, camera consent, and a benchmark within the `camera` budget |
| `full` | Core + `device_attestation` + `camera` | Everything above, within the `full` budget |

### Selection

At session start the backend, not the client, picks the profile. The client calls `integrity_select_profile` after `integrity_attest_session` (and after any companion pairing) with its capability report, per-capability consent and the role it is assessing for. The order is attestation result → capability report → device benchmark → policy floor and consent → the highest permitted profile that meets the floor, else the highest permitted. Every excluded profile carries its reasons: `not_attested`, `no_camera`, `camera_not_consented`, `not_benchmarked`, `over_budget`.

The selection record (profile, inputs, exclusions, signals not run, benchmark result and both budgets) is stored on `integrity_sessions.profile_selection_json` and folded into the commitment chain as its first link, before any snapshot. Selection is refused once the profile is locked or once anything is committed.

Consent is per capability (camera, microphone, companion phone) and recorded on the session (`consent_camera`, `consent_microphone`, `consent_companion_phone`). A profile that needs an unconsented capability is not selectable, and a companion pairing made without companion consent is dropped at selection. The microphone is used only by [voice checks](#candidate-integrity-check); no profile requires it.

### Lock and degradation

The profile is locked for the session. Losing a signal later (camera covered, permission revoked, inference stopped) is recorded, never renegotiated:

- Under `camera` or `full`, every window without camera checks gets `camera_degraded` (warning, channel `camera_coverage`). The camera counts towards a policy floor only while at least 90% of committed windows are free of it (`MIN_CAMERA_COVERAGE`, an unmeasured operating constant); below that, issuance refuses a floor that needs the camera.
- Under `core` or `attested`, camera features in a window are dropped as not run, never scored.
- Attestation loss is not a window flag: issuance re-verifies and lists `device_attestation` as degraded when it no longer verifies.

Capability absence is never clean: a signal the profile does not include is listed as not run in the credential.

### Device benchmark and budgets

The benchmark runs only when a camera is reported and consented: a synthetic 224×168 frame, one warm-up, then the median of three passes per stage (`sentinel::camera_budget::benchmark`). A failure excludes the camera profiles as `not_benchmarked`.

| Profile | Detect | Liveness | Track | Detection cadence |
|---|---|---|---|---|
| `camera` | 250 ms | 60 ms | 15 ms | every 3 s |
| `full` | 150 ms | 40 ms | 10 ms | every 3 s |

The budgets assume a low-end device class at about a tenth of an Apple M4 Pro performance core; that assumption is not yet measured on a device. Because it runs at session start, the benchmark measures contention as well as hardware: under heavy load a fast machine can be admitted to `camera` but not `full`.

### Policy floor and the learner notice

A sponsor role's `min_trust_profile` is the policy floor. It is met by signal-set inclusion, not by order: `camera` does not meet an `attested` floor. The floor is judged at selection, again at attempt start (an unmet floor refuses the attempt, and no attempt is consumed), and again at issuance, which refuses unless its judgement agrees with the recorded one. Interview invitations carry no floor.

Before the first item the standalone runner shows `TrustProfileNotice`: the chosen profile, the signals not run and why, the floor result, and what would raise the profile (pair a phone, use an attested device, connect or allow the camera, use a faster device). When the floor is unmet the runner stops monitoring before `assessment_start_attempt` and shows the notice instead; nothing is served.

## Verification rungs

The verification rung is positive evidence that the learner did the work, independent of the device (`alexandria_studio::integrity::verification_rung`). For an assessment session it is a strict ladder: a session stands on the highest rung whose evidence holds along with every rung below it. Missing or skipped evidence leaves the session a rung lower and never counts against the learner. An interview check has no attempt, probes or history to climb on, so its rung is `live` when a live segment was recorded and `none` otherwise; its surface carries no coverage statement, since each statement also claims the assessment rungs beneath its rung. A failed probe is a failed verification and raises the session to `flagged`.

| Rung | Evidence |
|---|---|
| `none` | — |
| `process` | Every served item was observed and the `response_process` channel is not over threshold. An item is observed when the response tracker reported a visit to it (`integrity_observed_items`, each id once, ids the attempt never served ignored) or the submitted attempt holds an answer for it, which covers answers restored on resume and left alone. An item left blank and never visited is left out. The tracker must have reported at least one served item. Every attempt is drawn by the backend's adaptive, exposure-weighted draw (see [skills-and-reputation.md](skills-and-reputation.md) §6.2), so that half needs no separate record |
| `probed` | The probe step ran and every probe result is satisfied |
| `consistent` | Longitudinal consistency is satisfied for the attempt |
| `live` | A satisfied live segment (verification step `interview`) in the same session |

### Response-process signals

The collector (`createResponseTracker`, `src/utils/sentinel/responseProcess.ts`, exposed as `useSentinel().responses`) keeps each item visit's timeline on the client and submits only per-visit aggregates (`WindowFeatures.response`). They work the same for a choice click, typed text, a plugin event or a spoken answer.

- **Reading floor**: (800 ms + 20 ms per character of prompt and options) × (1 + 0.15 per difficulty level above the easiest). It is not conditioned on correctness: answer keys never reach the client and windows commit before grading. Spoken answers have no floor, because answering a heard question at once is ordinary turn-taking.
- **Baseline**: the last 12 first-visit timings from earlier monitored sessions on the device, kept in the behavioural profile; with fewer than three, the first three items of the attempt. Interview sessions neither read nor write it.
- **Signals**: `response_below_reading_floor`, `response_relay_lag`, `response_idle_burst`, `answer_change_after_idle`, `gaze_down_then_response` and `voice_then_response` (rules 36–41 above). All are info and advisory.
- **Coverage**: wired in the standalone assessment runner (choice items) and the candidate interview check (spoken answers). Lesson quizzes and plugin events do not feed it yet.

### Probes

A probe is a short follow-up made from the learner's own submission, run after submission and before the session ends. The time box starts when the probe renders, and latency to first input is recorded with the result.

- **Policy**: `none`, `reseed`, `authored`, `plugin` or `llm`, set per assessment (`question_banks.probe_policy`) and per role (`IssuancePolicy.probe_policy`); a role policy other than `none` wins. Combining policies per item type is not built.
- **Which items**: only items answered with full marks, at most two per attempt.
- **Grader contract**: graders may export `alex_probe` beside `alex_grade` (`src-tauri/src/plugins/grade_contract.rs`). Input: item content, the learner's submission and a seed; output: a question, a machine-gradable expectation and a time box (5 s–10 min). The response is graded by the item's own grader, so Sentinel never knows the item format. The built-in MCQ grader implements it natively and in wasm; the editor graders do not, so `plugin` is skipped for their items. Plugin credentials still require the exact bundled grader bytes ([ADR 0003](adr/0003-credential-authority.md)).
- **`reseed`**: the same item with a new seed, time box 1.5 × the attempt's mean time per item, clamped to 15 s–5 min. **`authored`**: instructor-written follow-ups and seeded variants per item, authored over IPC (`assessment_item_probe_*`; no instructor UI yet, and they do not travel with items over sync).
- **`llm`** (enterprise): the role names a saved model connection on the device that runs the attempt (`IssuancePolicy.probe_connection_id`). It is a separate one-shot run type, not chat: chat, the tutor, practice and the MCP broker stay refused during the attempt, and the call still passes the age and guardian rules. The outbound message holds only the skill name and description and the text of the option(s) the learner chose; the item's prompt, other options and key never leave. It passes the item-bank guard first. The learner consents at attempt start (`assessment_attempts.llm_probe_consent`); declining skips the probe and never refuses the attempt. The model is asked for one single-answer choice question with four options and its key; the answer is validated, then sent back once with the key withheld, and the model must choose the same key again or the probe is skipped `invalid_probe`. It is graded by the item's own grader, with a 60 s time box. The key is still only the model's word, so an answer that misses it never fails verification and never flags the session: it is recorded skipped `unconfirmed_key` and sends the session to `review`. It does not satisfy `probed`. Each generated probe records its format, model, connection location, prompt hash, latency and key-check latency in the chain. A bank-level `llm` policy has no connection and is skipped. The structured-decision provider never generates probes and does not score them.
- **Record**: each result is a row in `integrity_verifications` (step `probe`, the policy, outcome `satisfied` / `failed` / `skipped`, reason, detail) folded into the commitment chain. Policy `none` still records one skipped step (`policy_none`) so the chain shows the policy. Skip reasons: `policy_none`, `no_eligible_items`, `provider_unavailable`, `no_authored_probe`, `timeout`, `not_shown`, `not_answered`, `not_consented`, `unsupported_item`, `bank_guard`, `invalid_probe`, `unconfirmed_key`. A grader that cannot run (not installed, not runnable here, bytes that do not match) skips the probe `provider_unavailable`. A response that breaks the submission contract before any grader sees it (not a JSON object; for a choice item, positions that are not a list of the item's options) fails it, with `error: invalid_response` in the detail. A choice response with no selection is an unanswered question and grades as wrong.
- **At issuance**: a role probe policy other than `none` requires that the probe step ran under it with no failed probe; skipped is accepted. A satisfied probe is required only through `min_verification_rung: probed`.

### Longitudinal consistency

At grading, `sentinel::consistency` compares the attempt outcome with the learner's own history on the skill, read from the profile database: practice first answers (right without a hint or reveal), scored lessons tagged with the skill, and graded items of earlier attempts, all from before the attempt started and within 90 days, weighted by recency (14-day half-life). Unscored completed lessons add a little expected success but never count as history on their own.

The verdict is `satisfied` (the `consistent` rung), `insufficient_history` (rung not reached; nothing counts against the learner) or `divergent`: the outcome sits well above what history predicts. Divergence raises `longitudinal_divergence` (info, channel `longitudinal`, advisory until certified); it is a fusion signal, not a failed verification. Nothing new is stored: the verdict is derived again from the database when needed.

### Live segment

In an interview held in Alexandria with a candidate integrity check, a panel seat's signal feed has **Live segment done**. The candidate's running check records it through `hiring_interview_live_segment` as a satisfied verification (step `interview`, policy `panel`), once per segment. It is refused unless the candidate accepted the interview with `mode = alexandria` and an integrity check, the session's purpose is `interview` and it has not ended. It is the candidate app's own record, folded into its chain; the panelist does not sign it.

Limit: the rung is a strict ladder and an interview session has no attempt, so live evidence in an interview check does not lift that session above `none`. Reaching `live` on a credential needs an attempt and an interview in one session, which no flow creates yet. The interview report sent to Cloud does not carry the verification.

## AI Models (Advisory)

Every model output is a feature like any other: it raises a flag only through the rules in §Flagging Logic, and it reaches a decision only through a certified ratio. The keystroke autoencoder, the mouse CNN and the LBP face embedder are candidates for removal once the real corpus shows whether they move precision (ticket 14 of the trust-profiles spec).

### 1. Keystroke Autoencoder

**Location**: `src-tauri/src/sentinel/keystroke_ae.rs`

**Architecture**: 4→8→4→8→4 autoencoder with ReLU activations. Real autograd via [candle](https://github.com/huggingface/candle); training runs in the Rust backend with `candle_nn::optim::SGD`.

**Input features** (per digraph pair): `[dwellMs1, dwellMs2, flightMs, speedRatio]` — normalized using per-user mean/std computed during training.

**Training**: On-device from calibration wizard data or accumulated assessment data. Requires 20+ digraph samples. 80 epochs, learning rate 0.005.

**Inference**: Reconstruction error averaged across all digraphs in the snapshot window. Mapped to [0,1] via sigmoid calibrated against training loss (5× `trainLoss` ≈ 0.5 anomaly score). A 0.05 floor is applied to `trainLoss` to prevent ratio blow-up when users train with very consistent typing.

**Anomaly threshold**: 0.65 — at or above this, the typing pattern doesn't match the enrolled user, and the backend raises `behavior_shift`.

**Storage**: weights persist as a JSON blob in `sentinel_user_models` (composite PK `user_address, device_fp_prefix, model_kind='keystroke_ae'`). Backing store is the already-sqlcipher-encrypted SQLite DB — replaces the legacy `localStorage` location. Never broadcast over P2P.

### 2. Mouse Trajectory CNN

**Location**: `src-tauri/src/sentinel/mouse_cnn.rs`

**Architecture**: Conv1D(3→8, k=5) → ReLU → MaxPool(2) → Conv1D(8→16, k=3) → ReLU → MaxPool(2) → Dense(160→32) → ReLU → Dense(32→1) → Sigmoid

**Input**: 50-point trajectory segments with 3 channels (dx, dy, dt), normalized per-segment.

**Training**: Dense layers trained via backprop. Conv layers act as random feature extractors (reservoir computing approach — deliberate trade-off for on-device training speed). Negative examples are 5 synthetic bot patterns: constant velocity, linear interpolation, sine wave, jittered straight line, instant teleport.

**Inference**: Average prediction across all 50-point segments in the buffer. Output is probability of human input.

**Human threshold**: 0.50 — below it the backend raises `bot_suspected`.

**Storage**: weights persist as a JSON blob in `sentinel_user_models` (`model_kind='mouse_cnn'`). Conv kernels are not stored — they are deterministic given the seed, recomputed on load. Only the dense-head parameters (~5 KB) round-trip through the DB.

### 3. Face Embedder (LBP)

**Location**: `src/utils/sentinel/face-embedder.ts`

**Algorithm**: Local Binary Pattern histograms with 4x4 spatial binning over detected face region. 59 uniform LBP bins per grid cell → 944-dimensional embedding, L2-normalized.

**Face detection**: YCbCr skin-color segmentation with bounding box extraction.

**Enrollment**: Running average of embeddings over 5+ frames during calibration wizard.

**Verification**: Cosine similarity between enrollment embedding and live frame embedding.

**Match threshold**: 0.70

**Storage**: ~8KB (944 floats) in the learner's device-local behavioural profile, saved through `sentinel_save_behavioral_profile` to the SQLCipher-encrypted profile database (`sentinel_user_models`); a legacy `localStorage` copy is deleted on load. Never broadcast over P2P.

**Advantage over skin-ratio**: Can distinguish between different people, not just "face present vs absent". Robust to lighting changes (LBP is contrast-invariant).

### 4. Paste / Typing-Bot Classifier (ONNX)

**Location**: `src-tauri/src/sentinel/paste_classifier.rs` (tract inference), `src-tauri/src/sentinel/features.rs` (12-dim feature extractor)

**Architecture**: MLP 12 → 32 → 16 → 1 (sigmoid). Trained offline on synthetic adversarial data (see [Synthetic training blobs](#synthetic-training-blobs)). Per-sample normalization mean/std is **baked into the first linear layer** so the on-device forward pass takes raw 12-dim features without preprocessing.

**Input features** (windowed over the snapshot's keystroke buffer):

| # | Feature |
|---|---------|
| 0 | mean dwell ms |
| 1 | std dwell ms |
| 2 | mean flight ms |
| 3 | std flight ms |
| 4 | fraction of digraphs with flightMs < 5 (paste-burst rate) |
| 5 | max consecutive near-zero-flight run length, normalized to /200 |
| 6 | char rate (chars/sec), normalized to /50 |
| 7 | dwell coefficient of variation (std/mean) |
| 8 | flight coefficient of variation |
| 9 | paste event count, capped + normalized to /10 |
| 10 | pasted character count, capped + normalized to /1000 |
| 11 | keystroke buffer length, normalized to /200 |

The Rust extractor in `sentinel::features` and the Python featurizer in `tools/sentinel-train/featurize.py` are **bit-identical**. If you change one, change the other in the same commit or the trained model will silently mispredict. There is no third copy on the frontend any more — the JS side only buffers raw events and forwards them via IPC.

**Inference runtime**: `tract-onnx` 0.21 in the Rust backend. Pure Rust, no WASM, no CDN. The ONNX bytes are embedded at compile time via `include_bytes!("../../resources/sentinel/paste-v1.onnx")` so there is no filesystem race or asset-protocol handshake. No command accepts replacement model bytes, so the parser only sees this embedded artifact.

**Platform support**: every Tauri target the Rust crate compiles for — macOS, Linux, Windows, iOS, Android. The previous WKWebView / Android-WebView gate is **gone** because nothing about ML runs in the WebView anymore.

**Anomaly thresholds**:
- 0.95: emits `paste_classifier_anomaly` (severity: warning)
- 0.99: emits `paste_classifier_critical` (severity: critical). Both are channel `clipboard`; on their own they send a session to review

**Model artifact**: `src-tauri/resources/sentinel/paste-v1.onnx` (~4.6 KB, weights inline; PyTorch `dynamo=False` export so tract can parse without sidecar `.data` files). Pinned via `src-tauri/resources/sentinel/paste-v1.onnx.sha256` lockfile (CI verifies on every push). A retrained model ships by replacing this artifact and its lockfile in an app release (see §Model Updates).

**Storage**: No per-user state. The classifier is a global attack detector — calibration is done via the existing `KeystrokeAutoencoder` for per-user personalization. There is no blend between the two: `sentinel_score_paste` returns the raw ONNX score, and `ai_paste_anomaly` and `ai_keystroke_anomaly` fold into the descriptive composite as **independent** advisory terms, each weighted `AI_ADVISORY_WEIGHT = 0.05` (`alexandria_studio::integrity::features`).

**Training**: `tools/sentinel-train/{featurize,train,eval}.py`. Outputs ONNX opset 17. Default 30 epochs, AdamW lr=3e-4, batch=128, label smoothing 0.05, 2× class weight on the `llm_paste_edit` class.

**Holdout gate**: A trained model only ships if `macro_tpr >= 0.92`, `macro_fpr <= 0.03`, `paste_macro` TPR ≥ 0.98, and `llm_paste_edit` TPR ≥ 0.85. The synthetic-only v1 release achieves macro TPR=1.0 / macro FPR=0.0; expect lower numbers when real-world holdout data joins (Phase 5).

### 5. Gaze / Second-Device Detector

Phase 1 is implemented: YuNet face detection at the frame's own resolution, landmark tracking between detections, geometric head-pose proxies, and an optional per-user candle calibration model. All of it is OSS and licence-clean, with no external gaze dataset. Phase 2 is deferred (see the end of this section).

**Location**: `src-tauri/src/sentinel/face_detect.rs` (YuNet detection via tract), `src-tauri/src/sentinel/gaze.rs` (head-pose + calibration), `src-tauri/src/commands/sentinel_gaze.rs` (IPC).

**Purpose**: catch the second-device cheat class — glancing down at a phone or sideways at a second monitor during an assessment. Honest scope: it flags off-screen *gaze*, so it catches look-away behaviour but not an earpiece + reader, nor a device propped directly beside the webcam. A strong add, not a gate on its own.

**Pipeline** (all backend, frames never leave the device):

1. **Face detection** — YuNet (`face_detection_yunet_2023mar`, OpenCV Zoo, **MIT**) embedded via `include_bytes!`, run through `tract`. The bundled `yunet.onnx` is re-exported with its fixed 640×640 shapes dropped (weights untouched; recipe `scripts/sentinel/export-yunet-dynamic.py`), so it runs at the frame's own size padded to a multiple of 32, with one plan cached per padded shape. Anchor-free 3-stride decode (8/16/32) yields bbox + **5 landmarks** (right/left eye, nose, right/left mouth) + score, then NMS. Output contract (12 tensors) is asserted in `model_contract_holds`.
2. **Head-pose proxies** — derived geometrically from the 5 landmarks (dep-free, no solvePnP): `yaw` = horizontal nose offset from the eye midpoint over inter-ocular distance; `pitch` = nose position between the eye line and mouth line; `roll` = eye-line angle. Plus a coarse `iris` offset (darkness-weighted centroid in an eye box).
3. **Gaze estimate** — two paths:
   - **Uncalibrated**: threshold the raw yaw/pitch proxies against a generous on-screen cone. No enrollment needed; coarse look-away detection.
   - **Calibrated**: a per-user `5 → 16 → 2` MLP (candle SGD) maps `[yaw, pitch, roll, iris_dx, iris_dy]` to a normalized screen point; off-screen = predicted point outside the unit square + margin. Trained from the wizard's 9-point capture.

**Calibration is license-clean by construction**: it trains on the user's *own* 9-point look-at-the-dot capture, so no external gaze dataset (e.g. Gaze360 / MPIIGaze, which forbid commercial models-trained-on-dataset) is ever involved.

**Storage**: calibration weights persist as a JSON blob in `sentinel_user_models` (`model_kind='gaze_calib'`), same convention as the keystroke AE / mouse CNN. Never broadcast over P2P. Reset by `sentinel_reset_user_models`.

**Flags** (derived per snapshot window by the backend from the window's gaze tallies, see §Flagging Logic):
- `gaze_wander` (warning) — off-screen ratio > 0.30
- `device_glance` (critical) — ≥ 3 downward off-screen glances in the window (the phone-in-lap tell)
- `gaze_occluded` (warning) — eyes hidden (no usable face geometry) for > 0.40 of ticks

The per-window `gaze_offscreen_ratio` is persisted on `integrity_snapshots` for dashboard observability.

**Frontend**: `Player.vue`'s camera loop calls `sentinel.scoreGaze(video)` (fire-and-forget) alongside the existing LBP presence check. It downscales the frame to ≤224px and sends the raw RGBA bytes as a binary IPC body with `x-sentinel-frame-width/height`, `x-sentinel-user`, `x-sentinel-device` and `x-sentinel-preview` headers (`src/utils/sentinel/frame.ts`); a JSON frame is refused. The 9-point calibration is a step in `SentinelTrainingWizard.vue`.

**Camera pipeline** (`sentinel::camera_pipeline`, `sentinel::inference_worker`): detection runs every 3 s and the five landmarks are patch-tracked between detections, falling back to detection when tracking or face geometry fails; between detections the face count carries over, so a second face is seen within one cadence. Inference runs on its own `sentinel-camera` thread with one pending frame per lane (session ahead of preview); a newer frame replaces a waiting one, and the replaced frame is answered `{ "dropped": true }`, which is not counted as a check. On an Apple M4 Pro a 224×168 tick fell from 102 ms to a 3.9 ms mean (`cargo run --release --example sentinel_camera_bench`).

**Phase 2 (deferred)**: precise point-of-regard via a MediaPipe iris model (Apache-2.0, needs a tflite→ONNX conversion); a gaze × keystroke-timing correlation signal (the high-confidence "copying" flag); face *recognition* via ArcFace (runtime-trivial on tract, but weights are license-gated — buy an InsightFace commercial license).

### 6. Liveness / Presentation-Attack Detector

`2.7_80x80_MiniFASNetV2` from minivision-ai's Silent-Face-Anti-Spoofing (**Apache-2.0**), exported to ONNX with softmax folded in (`scripts/sentinel/export-minifasnet.py`) and embedded via `include_bytes!` as `src-tauri/resources/sentinel/minifasnet-v2-80.onnx` with a `.sha256` lockfile CI verifies. Closes the gap the virtual-camera label heuristic leaves: a photo, a replayed video or a rendered face fed through any camera device.

**Input**: the YuNet face box scaled 2.7× about its centre and slid back inside the frame (the upstream `CropImage` rule), bilinear-resized to 80×80, BGR channel order, raw 0..255 values (upstream's `ToTensor` does not divide by 255). **Output**: softmax over `[attack, real, attack]`; class 1 is real. It runs inside `sentinel_score_gaze` on the frame the gaze check already holds, so no extra frame crosses IPC and nothing is persisted. It runs on a trigger, not every tick: a burst of three reads on a new face track, a box change, a score drop of 0.15 or more, a tracking loss, or a 30 s refresh.

**Advisory by design**: one low read is lighting or a webcam artefact as often as an attack, so the backend raises `spoof_suspected` only when more than half of at least three ticks in a snapshot window score below 0.5. Two parity tests pin it: the tract output against the PyTorch softmax, and the Rust preprocess against the upstream cv2 pipeline on a synthetic frame. On the upstream labelled samples with YuNet boxes, the real sample scores 0.9998 and both attack samples score fake.

### Synthetic training blobs

`alexandria synth-sentinel` (`cli/src/synth/`) generates the labelled keystroke data the paste classifier trains on, and the holdout it is gated against. Generation is deterministic: ChaCha20 seeded per label, and the same seed produces byte-identical output, pinned by Blake2b golden-hash tests. Training and holdout sets use disjoint seed ranges. One JSON blob is written per label and run (`cli/src/synth/blob.rs`, `schema_version` 1):

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | int | Blob format version; bumped on breaking changes |
| `model_kind` | string | `keystroke` |
| `label` | string | `paste_macro`, `typing_bot_constant`, `typing_bot_jitter`, `llm_paste_edit`, `remote_control` or `human_baseline` |
| `synth_seed` | int | Seed that produced the blob |
| `synth_version` | string | Generator version (currently `v2`); bumped when distributions change |
| `notes` | string | Free-form description |
| `samples[]` | array | One typing sequence each: parallel arrays `digraphs` (string), `dwell_ms`, `flight_ms`, `speed_ratio` (float) |

Field names are snake_case. `tools/sentinel-train/featurize.py` reads these blobs.

### Runtime Toggle

AI model outputs do not contribute to the descriptive composite by default; they still raise flags through §Flagging Logic whenever the model ran. A per-profile toggle (`setSetting('sentinel.ai_scoring_enabled')` in the profile-scoped settings DB, exposed in the Sentinel dashboard Profile tab) folds them into the composite at a 0.05 weight each:
- `(1 − ai_keystroke_anomaly)` × 0.05
- `ai_mouse_human_prob` × 0.05
- `ai_face_similarity` × 0.05 (only when the window carries camera checks)
- `(1 − ai_paste_anomaly)` × 0.05 (only if both the master AI toggle AND the per-signal `sentinel_paste_classifier_enabled` toggle are on)

The four advisory coefficients sum to at most 0.20 before normalization. Their
final share depends on the other included coefficients; they are not four fixed
five-percentage-point additions to the final score. Toggle off if false-positive
rate spikes.

**Per-signal opt-out**: the paste classifier has its own toggle in the Sentinel dashboard ("Paste Classifier (ONNX)") backed by `setSetting('sentinel.paste_classifier_enabled')` in the profile-scoped settings DB. Defaults to `on`; flipping it off keeps the other AI signals contributing. Useful if the paste classifier specifically generates FPs.

## Model Updates

The paste classifier has one model source: `src-tauri/resources/sentinel/paste-v1.onnx`, embedded into the Rust binary at compile time and loaded once by the `sentinel::paste_classifier` `OnceLock`. `sentinel_paste_classifier_info` always reports `bundled`. A retrained model ships by replacing the artifact and its SHA-256 lockfile in an app release; see [sentinel-runbook.md](sentinel-runbook.md). The face detector (`yunet.onnx`) and the liveness model (`minifasnet-v2-80.onnx`) follow the same rule: three bundled artifacts, one ship path, each with a lockfile CI checks.

The earlier community prior library and runtime weights replacement are deleted: Sentinel DAO proposals, approved-status ratification, placeholder signatures, the prior gossip mirror, and the kill switch and version blocklist that guarded replacement weights. Their tables are gone from the schema. Inbound `/alexandria/sentinel-priors/1.0` messages are rejected before any database access until the topic is removed.

## Measurement and certification

A signal earns weight only through measurement. The measurement corpus holds labelled sessions (scripted red-team attacks and consented clean sessions, never ordinary use); the offline evaluator replays them through the same `derive_window` and fusion code the app runs and prints per-signal precision, recall, false-positive rate and per-window likelihood ratio per profile, plus the fusion's confusion matrix. A certification is a reviewed manifest admitting signals to a profile with their measured rates, the operating threshold and the corpus digest. CI replays every committed certification and fails when an admitted signal falls below its certified rate. Synthetic corpora serve contract tests only and never certify.

No real certification exists yet, so `sentinel::scoring::active_manifest()` is uncertified and every signal is advisory. Formats, the evaluator, the capture path and the red-team scripts are in [sentinel-corpus-runbook.md](sentinel-corpus-runbook.md); code in `crates/alexandria-studio/src/integrity/measurement/`.

## Integrity Assurance

Trust in a credential has two independent axes, recorded separately in its signed `integrity` block (see [protocol-specification.md](protocol-specification.md) §14.9.5):

- **Device axis** — `assuranceLevel`: what the hardware can prove about the client. `"local"` (figures are device-reported; a determined attacker who controls the client could suppress them) by default; `"device_attested"` when a platform attestation bound to the session verifies at issuance (see §Device attestation below). `"anchored"` and `"high_assurance"` remain reserved ladder values with no verified production path: a sponsor role cannot require them, and an `IssuancePolicy.requiredAssuranceLevel` naming either refuses issuance.
- **Verification axis** — `verificationRung`: positive evidence the learner did the work (see [Verification rungs](#verification-rungs)).

Beside them the assertion names the `trustProfile`, the signals run, degraded and not run, the probe outcome summary and the `coverageStatementDigest`. All of it is re-derived from the session at issuance (`credentials::issue::build_integrity_assertion`), never copied from stored client text.

The sponsor policy (`IssuancePolicy`) gains `min_trust_profile` (the policy floor), `min_verification_rung`, `probe_policy` and `probe_connection_id`. The old `min_camera_coverage` knob is gone: the camera's coverage minimum is a property of the `camera` and `full` profiles (see [Lock and degradation](#lock-and-degradation)). Issuance refuses with `TrustProfileBelowFloor`, `PolicyFloorDisagrees`, `RungBelowMinimum`, `ProbePolicyNotRun` or `ProbeFailed` when one of these is not met.

### Coverage statements

A coverage statement is a fixed text per trust profile and verification rung (4 × 5 = 20, generated in `alexandria_studio::integrity::coverage`): what a session under that profile and rung checked, what it did not, the platform ceilings, how the signals' weight was established, and, once a certification is in force, its reference and the profile's certified rates. Every statement says that monitoring cannot see a second person or a second device, and that no sound is recorded or transcribed.

A statement is content-addressed: its digest is `blake3:` over its compact JSON bytes, and the assertion carries that digest. The app bundles every statement, so the credential detail view resolves the digest offline (`integrity_coverage_statement`), and exported credential bundles carry the statements their credentials name (`coverage_statements`). The text is the contract: statement lines are quoted in English in every locale, because translating them would change what the digest covers, and changing a line changes every digest it is part of (bump `COVERAGE_VERSION`). Earlier text versions are not kept in code, which is why bundles carry the statement itself. Until a certification exists, every statement says that every signal is advisory.

### Commitment chain

Every link folds into a running hash (`fold_commitment`, `domain::integrity_commitment`): `root_n = blake2b(tag | root_{n-1} | canonical(link_n))`. The links, in order: a companion phone pairing when there is one (`companion|…`), the trust profile selection (`trust_profile|…`), then each committed snapshot, with each verification result (`verification|…`, probes and live segments) folded in as it is recorded. Changing or reordering any link changes the terminal `commitment_root`. Per-snapshot hashes persist on `integrity_snapshots.commitment_hash`, per-verification hashes on `integrity_verifications.commitment_hash`, the running root on `integrity_sessions.commitment_root`, and the signed `integrity` block carries it as `commitmentRoot`. It is local tamper evidence, not an independent witness. `longitudinal_divergence` is the one input decided after the session ends, so it is not in the chain; it is derived again from the database whenever it is needed.

### Device attestation (`device_attested`)

Sentinel's flags say what the client saw; device attestation says the client was ours. It is the only rung above `local` with a verified path. Phones attest themselves; a desktop reaches it only through a [companion phone](#companion-phone).

- **Capture.** Right after `integrity_start_session` the frontend calls `integrity_attest_session`. iOS: `DCAppAttestService` generates a Secure Enclave key and attests it over the session nonce; when the session ends, the same key signs the terminal commitment root (an assertion), so the platform's statement covers the snapshot stream and not only the start. Android: a Play Integrity classic request for the nonce. Desktop reports `supported: false`.
- **macOS cannot attest itself.** `DCAppAttestService` compiles for macOS, but Apple documents `isSupported` as `false` on every Mac, including Mac Catalyst apps and iOS apps on Apple silicon, whatever the distribution channel. macOS is therefore not wired.
- **Verification happens on the device that issues**, with no service in between. Apple's App Attestation Root CA is embedded (`sentinel::attestation::APPLE_APP_ATTEST_ROOT_PEM`, SHA-256 fingerprint `1CB9823B…6242C932`). Play Integrity tokens are opened with the developer's response keys from the network profile (`play_integrity_response_keys`); while those are absent, Android tokens are kept on the session and the session stays `local`.
- **App Attest checks:** the x5c chain to the root and each certificate's validity; the nonce extension `1.2.840.113635.100.8.2` equals `SHA-256(authData ‖ clientDataHash)`; key id equals `SHA-256(public key)`; `rpIdHash = SHA-256("VLMNL3V44U.org.alexandria.node")`; counter 0; AAGUID `appattest` (`appattestdevelop` is accepted only in debug builds); credential id equals key id. The assertion must verify under the attested key over the commitment root with the counter advanced.
- **Play Integrity checks:** `A256KW` / `A256GCM` JWE around an `ES256` JWS; our nonce; our package name in both the request and app details; `PLAY_RECOGNIZED`; `MEETS_DEVICE_INTEGRITY`.
- **Stored, then re-verified.** `integrity_sessions.attestation_json` (migration 14) holds the platform, nonce, app id and the opaque blobs. Issuance never copies the stored `assurance_level`: `build_integrity_assertion` recomputes the nonce and re-verifies, falling back to `local` with a logged reason and listing `device_attestation` as degraded.
- **Policy.** `IssuancePolicy.requiredAssuranceLevel: "device_attested"` is accepted for sponsor roles; issuance refuses when the stored attestation does not re-verify. A `min_trust_profile` of `attested` or `full` asks for the same evidence through the profile.
- **Ceiling.** The level says "a genuine build of the app on a genuine device", not "no second device in the room" and not "this person". A rooted or jailbroken device that defeats the platform check fails into `local` rather than into a false `device_attested`.

### Companion phone

A desktop session can be joined by the learner's own phone, which lends it the phone's platform attestation (`sentinel::companion`, IPC `integrity_companion_*`).

1. The learner ticks **Pair my phone first** in the desktop assessment runner. After `integrity_attest_session` (unsupported on desktop) the desktop shows a one-time code: the session id and start, the learner's DID, the desktop's iroh address, a 32-byte secret and a 5-minute expiry. There is no QR code.
2. On the phone, Sentinel dashboard → **Join from phone**, paste the code. The phone joins only if the code names the DID of its own unlocked profile. It attests `SHA-256(tag ‖ session nonce ‖ learner DID ‖ phone endpoint id)` with its platform attestation, signs the result with the learner's DID key, and dials the desktop over a dedicated ALPN (`alexandria/sentinel-companion/1`) on the existing iroh endpoint. App to app: no relay or monitoring change.
3. The desktop checks the secret, that the connection is authenticated as the phone endpoint the binding names, the DID signature, and the platform attestation against a nonce it recomputes from its own session row. A failed attestation is refused; an Android phone without response keys is stored at `local`. The verified attestation is stored on `attestation_json` with a `companion` binding and folded into the chain before the profile selection, which then needs companion consent (a pairing without it is dropped). Issuance re-verifies it and credits it only when the binding names the profile's own learner.
4. At session end an iOS phone signs the desktop's terminal commitment root.

**Ceiling.** The pairing proves that a genuine app on the learner's genuine phone joined the session. It does not prove that the phone stayed in the room or watched anything: the end signature covers the root the desktop presents, which the phone cannot recompute. The protocol and desktop ingestion for the phone acting as the session's camera are built, but no phone-side camera producer exists in the UI, so desktop sessions pair with the camera off. Real-device verification is ticket 16 of the trust-profiles spec.

### Deleted attestation paths

The unverified anchor setter (`integrity_set_anchor`), which let any caller mark a session `anchored`, and the local committee co-sign path (`integrity_record_attestation`, `integrity_get_assurance`, the Sentinel DAO committee check and the unwired co-sign gossip handler) are deleted with the `legacy-integrity-attestation` feature. Issuance ignores the stored `assurance_level` and `anchor_ref` columns. An independent witness must return through verified committee outcome certificates.

## Database Schema

```sql
integrity_sessions       -- One per monitored session; status, assurance_level, commitment_root,
                         --   attestation_json (incl. companion binding), purpose=assessment|interview,
                         --   trust_profile, profile_selection_json, policy_floor_json,
                         --   consent_camera|consent_microphone|consent_companion_phone, response_items
  ├── integrity_observed_items -- Item ids the response tracker reported, once per session
  ├── integrity_snapshots      -- Committed windows: derived scores, anomaly_flags, ai_paste_anomaly,
  │                            --   gaze_offscreen_ratio, commitment_hash
  └── integrity_verifications  -- Probe and live-segment results folded into the chain
assessment_probes        -- Served probes per attempt: copy, time box, latencies, grade, generation_json
assessment_item_probes   -- Instructor-authored follow-ups and seeded variants per item
question_banks.probe_policy, assessment_attempts.llm_probe_consent,
interview_criteria.follow_ups_json
integrity_evidence, integrity_evidence_consent, integrity_evidence_release,
integrity_flag_notice    -- Learner-consented appeal evidence and its release trail
sentinel_user_models     -- Per-user candle weights (keystroke_ae, mouse_cnn, gaze_calib) and the
                         --   behavioural profile (JSON blobs)
sentinel_holdout_refs    -- References to the encrypted holdout set
```

Stored in local SQLite. See [Database Schema](database-schema.md) for full DDL.

## IPC Commands

| Command | Description |
|---------|-------------|
| `integrity_start_session` | Start integrity monitoring; `enrollment_id` is optional and `purpose` defaults to `assessment` or accepts `interview` |
| `integrity_attest_session` | Ask the platform to attest the session nonce (iOS App Attest, Android Play Integrity); stores the result and reports `supported` / `stored` / `assurance_level`. Desktop reports unsupported |
| `integrity_select_profile` | Pick and lock the session's trust profile from the capability report, consent and role; runs the device benchmark when a camera is reported and consented; returns the selection and the policy floor as judged now |
| `integrity_get_session` | Get session with scores |
| `integrity_end_session` | End the session (takes no client scores); on iOS the attested key, and an iOS companion phone, first sign the terminal commitment root |
| `integrity_submit_snapshot` | Submit one window's raw `WindowFeatures`; the backend derives scores and flags and returns the snapshot with `session_status`, `consistency_score` and the fusion summary. Any client score or flag is refused |
| `integrity_list_sessions` | List all sessions |
| `integrity_list_snapshots` | List snapshots for a session |
| `integrity_verification_summary` | Verification results recorded in a session's commitment chain |
| `integrity_coverage_statement` | Resolve a coverage statement digest offline from the statements bundled with the app; `null` when this build does not carry it |
| `integrity_coverage_statements` | Every bundled coverage statement under the certification in force |
| `integrity_companion_offer` / `integrity_companion_await` / `integrity_companion_cancel` | Desktop: open a one-time code for the learner's phone, wait for it to join and store its verified attestation, or withdraw the code |
| `integrity_companion_join` / `integrity_companion_send_camera` / `integrity_companion_leave` | Phone: join a desktop session as its learner, send one window of camera scores (never frames), or leave |
| `assessment_probes_start` / `assessment_probe_render` / `assessment_probe_answer` / `assessment_probes_finish` | Run the probe step after submission: prepare (or generate) probes, start a time box on render, grade an answer, mark the rest skipped |
| `assessment_item_probes_list` / `assessment_item_probe_save` / `assessment_item_probe_delete` | Author follow-ups and seeded variants per item |
| `list_role_session_reviews` | A role's finished sessions ranked for review by fusion posterior, each raised signal with its certified rates or marked advisory |
| `hiring_interview_live_segment` | Record a live segment the panel marked done in the candidate's running integrity check |
| `sentinel_voice_listen` / `sentinel_voice_stop` | Turn voice checks on the call's microphone stream on (consent-checked, interview sessions only) or off |
| `tutoring_send_interview_signal` | Send an interview signal over the call: the panel's question marker or live segment, the candidate's answer timing or second voice |
| `interview_set_criterion_followups` / `interview_followups_for_signal` | Author follow-ups per interview criterion; suggest the next one for a signal |
| `sentinel_score_paste` | Extract 12-dim features + score via tract. Single round-trip per snapshot. |
| `sentinel_paste_classifier_info` | Loaded model source + version (always `bundled`) |
| `sentinel_train_keystroke_ae` | Train (or fine-tune) the per-user keystroke autoencoder via candle |
| `sentinel_score_keystroke_ae` | Score keystrokes against the user's AE; `-1.0` if not yet trained |
| `sentinel_extract_digraphs` | Pull `DigraphFeatures` from raw keystroke events |
| `sentinel_train_mouse_cnn` | Train the per-user mouse-trajectory CNN dense head via candle |
| `sentinel_score_mouse_cnn` | Score mouse points; `-1.0` if not yet trained |
| `sentinel_load_behavioral_profile` / `sentinel_save_behavioral_profile` | Read or store the device-local behavioural baseline and face embedding |
| `sentinel_user_models_status` | List per-user model rows (epochs, samples, loss, updated_at) |
| `sentinel_reset_user_models` | Wipe per-user AE + CNN + gaze-calib weights for `(user, device)` |
| `sentinel_detect_face` | Run YuNet on a raw RGBA frame body; return bbox + 5 landmarks + score per face |
| `sentinel_extract_gaze_features` | Head-pose + iris features for the best face (wizard calibration capture), from a raw RGBA frame body |
| `sentinel_score_gaze` | Queue a raw RGBA frame on the camera thread: detect or track, liveness on trigger, per-user calibration; returns the gaze estimate and face count, or `dropped` |
| `sentinel_train_gaze_calib` | Fit/refit the per-user gaze calibration MLP from 9-point samples |
| `sentinel_display_topology` | Native display arrangement: count, external, mirrored, split-screen, screen captured, Android transition counter |
| `sentinel_android_environment` | Android: accessibility services, debug bridge, shield state, obscured-touch count (Android only; `Err` elsewhere) |
| `sentinel_set_assessment_shield` | Android: engage/release `FLAG_SECURE` + `setHideOverlayWindows` for the current activity |
| `sentinel_take_obscured_touches` | Android: return-and-clear the count of touches delivered through an obscuring window |
| `sentinel_hidden_overlay` | Desktop: scan for capture-excluded / override-redirect / click-through windows owned by other processes |
| `sentinel_process_scan` | Desktop: watchlist matches by category; the process list stays in the backend |
| `sentinel_hotkeys_status` | Desktop: monitor capability + permission state plus the OS-owned combo vocabulary; never prompts |
| `sentinel_hotkeys_request_permission` | Desktop: request listening permission (macOS Accessibility prompt); called only from the wizard |
| `sentinel_hotkeys_start` / `sentinel_hotkeys_stop` | Desktop: start/stop the listen-only modifier-combo monitor (idempotent) |
| `sentinel_hotkeys_drain` | Desktop: return-and-clear normalised combos recorded since the previous drain |

## UI

### Sentinel Dashboard (`/dashboard/sentinel`)

4-tab interface:
- **Overview**: Live engine status, no-profile prompt, session statistics
- **Sessions**: Paginated session list with outcome badges (including `review`), expandable details
- **Signals**: Composite weight breakdown, threshold documentation. The flag catalog is data-driven from `FLAG_CATALOG` in `Sentinel.vue`, mirroring the backend catalog; every flag has a learner-facing description and trigger in the nine locales
- **Profile**: Stored behavioral profile details, AI model training status

On phones the dashboard also has **Join from phone** (`/dashboard/sentinel/companion`), where a [companion phone](#companion-phone) pastes the desktop's code.

### Training Wizard

6-step calibration flow (similar to Face ID setup):
1. **Welcome** — Explains what Sentinel does and the privacy guarantee
2. **Typing Calibration** — User types a reference paragraph; system captures digraph timing
3. **Mouse Calibration** — Click-target game with 8 randomly placed targets
4. **Awareness** — Explains what signals are monitored during assessments, including the display, overlay and process watchers; on desktop this step hosts the only prompt for the phantom-hotkey listening permission
5. **Camera** (optional) — Face detection test + LBP enrollment
6. **Review** — Summary of calibrated profile + AI model training results

### Learning Page Integration

`useSentinel` lifecycle hooks in the course player:
- `start(enrollmentId, camera, purpose, options)` on mount — `enrollmentId` for an enrolled course, or `null` for a standalone assessment attempt (the assessment runner always starts Sentinel). It awaits attestation, any companion pairing and profile selection; `trustProfile` exposes the selection
- `setElement()` on element navigation
- `stop()` on unmount
- `setCameraOptedIn(bool)` when the learner accepts/declines camera verification on an assessment element; the camera is turned off when the locked profile does not run it
- `reportCameraDevice(label)` right after `getUserMedia` resolves, so a virtual-camera label is checked against the marker list without the label leaving the frontend
- A `setInterval(3000)` in `Player.vue` calls `sentinel.verifyFace(videoEl)` while the camera stream is live; the hidden `<video>` element is torn down on disable or unmount
- `responses.present()` / `responses.input()` / `responses.cue()` feed the [response tracker](#response-process-signals)

The standalone runner (`/assessment/:skillId`) first asks `assessment_open_roles`; when a published sponsor role covers the skill, the learner picks the role (or none) before `sentinel.start`. A role whose policy floor includes the camera (`camera_required`) has the camera turned on first (`useCameraPresence`). A role with a generated-probe connection asks for the follow-up consent, and on desktop the learner can tick **Pair my phone first**. The role is stored on the attempt (`assessment_attempts.role_assessment_id`) and reported back on resume. Before the first item the runner shows the [learner notice](#policy-floor-and-the-learner-notice). After submission it runs the probe step (`AssessmentProbes.vue`), then ends the session and grades.

### Sponsor surfaces

- **Role policy** (`RoleIntegrityPolicy`): minimum trust profile, minimum verification rung and follow-up (probe) policy, each with plain language beside it, and below them the coverage statement a credential under those minimums names. A saved model connection can be chosen for generated follow-ups (`GeneratedProbeConnectionField`), which sets `probe_policy: llm`; the UI shows the connection's name, never a provider's. Role cards list the three minimums.
- **Review queue** (`SessionReviewList`, the role's issue tab): `list_role_session_reviews` rescores each finished session of the role's attempts from its committed windows under its locked profile and ranks them by fusion posterior — the summed channel log-likelihood ratio, which orders sessions the same as a posterior under any shared prior, so no prior is assumed — then by status, then by signal windows. Each raised signal shows its certified precision and recall, or is marked advisory. While nothing is certified, the queue is ordered by status.

### Credential detail and bundles

The credential detail view shows the trust profile, the verification rung, the signals run, degraded and not run, the probe summary, and the [coverage statement](#coverage-statements) resolved from `coverageStatementDigest`, or says this build does not carry it and shows the digest. Exported bundles carry `coverage_statements`: every statement their credentials name, once, in digest order (omitted when empty, so older bundles still parse).

### Interview Assistant Integration

The instructor-facing interview workspace calls
`start(null, cameraOptedIn, 'interview')` only after the participant associated
with the conductor device has recorded consent for Sentinel. Camera checks need
the separate camera choice. It uses the same event buffers, native focus
monitor, model pipeline, snapshot cadence and diagnostics as an assessment.

The scope is local: Sentinel can observe only the conductor device. It does not
silently activate on a remote candidate's device. Interview-purpose snapshots
store derived scores and flags, but the backend does not stage their camera
frames for the assessment appeal-evidence flow. See
[Interview Assistant](interview-assistant.md#sentinel).

### Candidate integrity check

For an interview held in Alexandria, an organisation can ask in its signed
invitation for the candidate to run Sentinel on their own device during the
call. The candidate decides in the waiting room (`/interview-room/:id`), with
nothing preselected, and separately whether to allow camera checks and voice
checks; saying no still lets them join. The voice-checks choice is the
session's microphone consent. A yes starts `start(null, camera, 'interview')`
before the room is joined (`useInterviewCheck`), the call screen shows that the
check is on, and face and gaze checks look at the call's own self-preview only
when the camera was allowed.

**Voice checks** (`sentinel::voice_activity`) are signal processing, not a
model: frame energy against an adaptive noise floor, plus autocorrelation for
voicing and pitch. They listen to the call's own microphone stream, tapped in
front of the Opus encoder, so the microphone is never opened twice. They stop
while the call is muted, and ignore the microphone while the far end is audible
and for 300 ms after, because desktop calls have no echo cancelling. The
backend allows them only for a running interview-purpose session with
microphone consent. A "second voice" is a voice whose median pitch is more than
4.5 semitones from the first voice heard on the device: a second speaker with a
similar pitch is missed, and a whisper has no pitch. Nothing is recorded,
transcribed or stored; only onset times and whether a voice was the first or
another cross into the app.

**Interview signal feed.** The feed runs on the candidate's device only. The
panel's device sends a question marker over the call's encrypted room channel
(**Ask now**, or **Mark question asked** for an unscripted one). The candidate's
app treats the question as asked once the far end has been quiet for 400 ms,
takes the candidate's next onset in their own voice as the answer, and sends
back the latency, the median of the candidate's first three answers, and
whether a gaze-down or a second voice came within 3 s before the answer; a
second voice is sent as soon as it is heard. An answer that starts before the
question ends is not timed. When an answer comes with a concern (lag of 3 s or
more over the baseline, gaze-down before, or second voice before), the panel is
offered the next follow-up authored for that question's criterion
(`interview_criteria.follow_ups_json`, written in the preparation view), or a
generic step-by-step one. The same answers feed the response tracker as spoken
answers (`voice_then_response`, `response_relay_lag`). A panel seat can also
mark a [live segment](#live-segment) done.

Leaving the call ends the session. The backend
(`hiring_interview_integrity_send`) builds the summary from the stored session,
not from anything the frontend holds: an outcome (`clear`, or `flagged` when
any warning or critical flag was raised, the score fell below 0.40, or the
session ended in `review`, `flagged` or `suspended`), the score as 0–100, start
and end, the snapshot count, the percentage of snapshots with the camera on,
each flag in the app's vocabulary with its severity and how many snapshots
raised it, and the commitment root. It signs that over the accepted invitation
under `alexandria-interview-integrity/1`, posts it to the organisation's Cloud,
and keeps the learner's copy (`interview_integrity_reports`, migration 17). The
waiting room then shows exactly what was sent, or offers to send it again.

The summary carries no frames, audio, keystrokes, mouse data, window titles or
process lists, and Cloud refuses unknown fields. The organisation and the panel
see it with the caveat that it is the candidate's app's own report: a flag is a
reason to ask, not a verdict. Interview-purpose sessions stage no evidence, so a
flagged check does not ask to keep any.

An interview held by video call, phone or in person carries no Sentinel
assurance: Sentinel sees only the device it runs on, which is not where such an
interview happens. `build_integrity_report` refuses one unless the session
reached `device_attested` (a companion phone, or a phone's own attestation), and
no UI starts a check for such interviews yet.

## Review and adjudication

Sentinel produces flags. A flag is an accusation in waiting, so what happens next
is a design decision with real consequences for the person flagged.

**The constraint, stated once:**

> A flag may travel. The evidence behind it may not, unless the learner releases
> it themselves.

### What a flag may carry

A flag that leaves the device carries the derived scores and nothing else: the
per-signal values (`typing_score`, `mouse_score`, `human_score`, `tab_score`,
`paste_score`, `camera_score`), the `composite_score`, and
timestamps. These are already what `integrity_snapshots` holds locally, so no new
class of data is created by sending them.

The legacy nullable `devtools_score` storage/serialization field remains for
historical compatibility; new submissions must leave it null. Its presence in
an old record does not authorize a new developer-tools misconduct rule.

It may **not** carry keystroke timings, mouse traces, face embeddings, gaze
estimates, or anything else from which behaviour could be reconstructed. Those are
never persisted even locally (see Privacy Guarantees 1–3), and a review queue is
not a reason to start.

### Why the reviewer does not get the evidence by default

A human reviewing an integrity flag will want the underlying evidence, because
eight numbers are thin grounds on which to fail someone. That instinct is correct
and it is exactly why the default must be refused: satisfying it means an operator
holding face frames, gaze traces and keystroke dynamics belonging to people
accused of cheating. That is the most sensitive data this system can produce,
about the people least able to object, held by the party with an interest in the
outcome.

### How an appeal works instead

The learner can release the evidence for their own session, and only they can.

1. A flagged session shows the learner what was flagged and the scores behind it.
2. If they contest it, they may release the supporting evidence to the reviewer.
   The release is explicit, per-session, and theirs to make.
3. Releasing requires the evidence to still exist locally, so a flagged session
   retains its raw signals on-device for a bounded appeal window. Nothing is
   retained for unflagged sessions, and nothing is retained after the window.

#### Being told there is something to contest

A right to contest a flag is worth what the chance of hearing about the flag is
worth. A service can decide it could not explain an assessment, open a review,
have somebody read it and reach a conclusion, without the person it is about
learning any of it — so the application checks, on unlock, whether the
Alexandria Cloud it talks to is reporting a flag it has not already told them
about.

It is learned from the learner's own export, which they can already fetch and
which answers to nobody else. Nothing is fetched in order to check: a learner
who has switched off requests from organisations, or whose app has no
Alexandria Cloud set, never has a row, and a service that cannot be reached
produces no notice rather than an error.

Each flag is raised with the learner **once** — repeating an accusation is its
own kind of pressure — and the record is of what was shown, not of what was
agreed. The column is `told_at`, not `accepted_at`: closing a notice about an
accusation is not accepting it. The notice is not styled as an alarm, because
sessions are flagged for ordinary reasons and a notice that reads like an
emergency teaches people to expect the worst of a false positive. "Not now" and
"See what they have" are equal actions; the application has no opinion about
whether somebody should defend themselves.

#### Where a release goes, and taking it back

A release is addressed to the service's identifier for the assessment, which the
device learns from that same export. It carries only evidence consent already
wrote to disk — nothing is captured or retained in order to release it — and the
learner chooses which assessment a local session belongs to. Nothing matches them
automatically: a wrong match would send camera frames of somebody to an
organisation that was never assessing them.

At the other end, released evidence is legible to the one reviewer assigned to
that case and to nobody else at the organisation. Consenting to the person
deciding your case seeing your face is not consenting to everybody who can sign
in seeing it, and opening it is recorded against that reviewer's name in the
organisation's audit chain, where the learner can ask what it says.

Release is all of it or none of it. A session too large for one request is sent
in several, and if any of them fails the part that arrived is withdrawn — half a
session's evidence reads as the whole of what somebody chose to send.

**Deleting locally withdraws the copy.** A learner who deletes released evidence
is asking for the copy on somebody else's disk to be destroyed too, and that is
what happens; a person who deletes something and is told it is gone has been told
something untrue otherwise. The request is recorded before it is attempted and
retried on every later unlock, because a device is often offline exactly when
somebody decides they want something taken down. Sending is something a person
does once, when they ask. Taking back is something they are owed, so it retries
until it succeeds.

This inverts the usual arrangement deliberately. In most proctoring systems being
accused is precisely when you lose control of your own data. Here it is the moment
control matters most, so it stays with the learner — and the appeals path becomes
the mechanism that makes the system fair rather than a formality bolted on
afterwards.

### The reviewer cannot ask

A release must be learner-initiated. That is not only a rule about consent, it is
a constraint on the interface: **a review console may not offer a control that
requests evidence.**

Forbidding the request but keeping the button achieves nothing. A reviewer who can
click *request evidence* learns, from the silence that follows, that evidence
exists, that it was obtainable, and that the learner said no. The refusal leaks
whether or not anyone records it, because the interface has already taught the
reviewer what to infer. Suppressing the log entry does not suppress the
inference.

So the reviewer is shown the scores. If a learner releases evidence, it appears.
If they do not, the reviewer never learns a decision point existed. A reviewer who
is unaware there was a choice cannot hold the choice against anyone.

This does not stop an employer asking out-of-band, and nothing can. What it stops
is this product being the instrument of the ask.

Two consequences follow:

- **A refusal is not an event in the operator's audit log.** An append-only,
  hash-chained log is exactly what makes an entry discoverable — by
  administrators, in litigation, under subpoena. Nothing records that a learner
  could have released evidence and did not, because there is no asking and so
  nothing to record; a learner who never releases anything leaves no trace of
  having decided.

  What *is* recorded is what the organisation did with data it actually held:
  that evidence arrived, that a named reviewer opened it, and that it was
  destroyed — whether the learner withdrew it or it expired. That last one was a
  deliberate choice with a cost, and the cost is that "the candidate pulled their
  evidence back" is now a discoverable sentence. It was taken because an
  organisation that destroys data about a person should be able to show that it
  did, and a custody record with a hole exactly where the destruction goes is
  worth less than the inference is worth guarding against.

  The constraint that follows is absolute and belongs beside the one below: **an
  adjudication may not cite a withdrawal.** "They sent it and then took it back"
  is the same penalty as a visible refusal, rebuilt in prose.
- **An adjudication may not cite absent evidence.** "The learner did not
  substantiate this" is the same penalty as a visible refusal, rebuilt in prose. A
  decision rests on the scores or it does not stand.

### What this forbids downstream

Any control plane consuming Sentinel flags inherits this constraint. It may store
scores, verdicts, reviewer identity, decision and rationale. It may not store raw
behavioural capture, and it may not make evidence release a condition of using the
product, of employment, or of an appeal being heard — a release extracted under
those terms is not a release.

Two further constraints follow from evidence actually being releasable:

- **Released evidence is readable by the reviewer on the case, not by the
  organisation.** A control plane that shows a candidate's camera frames to
  everyone who can sign in has widened a consent that was given to the person
  deciding. Access follows an assignment somebody made deliberately, and reading
  it is attributable to the individual who read it.
- **A withdrawal is honoured.** When the subject asks for released evidence to be
  destroyed, it is destroyed, without asking why and without the request being an
  event a reviewer is shown. What a reviewer has already read, they have read;
  that is a limit of the world, not a licence to keep the copy. The destruction
  is recorded in the organisation's audit chain — an organisation that destroys
  data about a person should be able to show that it did — and an adjudication
  may not cite that record.

If a customer asks for the raw evidence by default, the answer is no. This is not
a configuration option.

## Privacy Guarantees

These guarantees are architectural — they are enforced by the code structure, not by policy.

1. **Raw keystrokes never stored**: Only anonymized timing features (dwell/flight in ms). The `key` field is set to `'char'` for all printable characters.
2. **Raw mouse coordinates never transmitted**: Only deltas (dx, dy, dt) used for CNN features; absolute positions stay in the short-lived buffer.
3. **Video frames never leave the device**: Face processing happens on a `<canvas>` element. Frames are forwarded to the Rust backend over in-process Tauri IPC for YuNet detection + gaze estimation and processed in memory. By default they are **never written to disk** — only derived values (944-float embedding, skin ratio, gaze yaw/pitch, off-screen ratio) are stored. The gaze calibration model encodes a statistical pose→screen mapping, not recoverable imagery.

   **The one exception, and it requires the learner to ask for it.** When a snapshot is flagged, the frame that was being scored is held in memory rather than dropped. If — and only if — the learner is then shown the flag and chooses to preserve the evidence, those frames are written to the SQLCipher-encrypted profile database with a 14-day expiry (`integrity_evidence`). Decline or ignore the prompt and nothing is written; the in-memory copy is discarded on that answer, on profile lock, and on process exit.

   This exists because [Review and adjudication](#review-and-adjudication) gives a learner the right to release evidence contesting a flag, and that right is nominal if nothing was kept. It is bounded to flagged sessions, to the flagged snapshots rather than the whole session, and to an absolute deadline enforced on unlock so evidence expires on time even if the app was closed throughout. The learner can delete it earlier at any point, and deleting it can never count against them — an adjudication rests on the scores, and absent evidence may not be held against a learner.

   Frames are captured at 224px on the longest side and stored JPEG-encoded, so this is low-resolution imagery, roughly 15 KB per flagged snapshot. That reduces the exposure; it does not make it nothing, which is why it is opt-in.
4. **AI model weights are not biometric data**: Autoencoder/CNN weights encode statistical patterns of typing/movement, not recoverable input data. LBP embeddings cannot be reverse-engineered into face images.
5. **Profile keyed to device**: the behavioural profile is stored per user and device fingerprint prefix in the profile's SQLCipher database — profiles are device-specific.
6. **No server-side data**: All behavioral processing happens on-device. The Rust backend stores only numeric scores, categorical flags and verification results in local SQLite. Three paths carry anything derived from a session off the device, and the learner starts each: an evidence release during an appeal — see [Review and adjudication](#review-and-adjudication) — the summary of a [candidate integrity check](#candidate-integrity-check) they agreed to run for an organisation's interview, which holds only an outcome, a score, flag counts and camera coverage, and a generated probe (guarantee 17). There is no other automatic path, and no operator-initiated one.
7. **Inference is local**: The paste classifier runs entirely in the Rust backend via `tract` (pure Rust); the ONNX bytes are embedded at compile time with `include_bytes!`, so there is no runtime fetch from a CDN and no remote inference path. (The earlier ONNX Runtime Web / WASM backend was retired — see "Inference runtime" above.)
8. **No remote models**: No command accepts classifier weights. The only ONNX models the backend parses are embedded in the binary, so no peer or envelope can supply model bytes.
9. **Interview scope is explicit**: Sentinel runs during an interview only after a participant on the conductor device records the Sentinel choice, or after the candidate agrees to an organisation's integrity check on their own device; camera-derived checks require the separate camera choice, and voice checks the separate voice-checks choice. Interview-purpose sessions store derived signals only and never stage camera frames as appeal evidence.
10. **Process and window lists never leave the backend**: `sentinel_process_scan` returns only watchlist matches (name, identifier, category, rule) and `sentinel_hidden_overlay` only the suspicious windows (owner, reason). The enumerations that produced them are dropped in the Rust call. Window titles never cross IPC, and the allowlist matches the whole owner name, never a substring.
11. **The hotkey monitor records no text**: only key-downs with Cmd/Win/Super, Ctrl or Alt held, as a normalised combo string plus a monotonic timestamp. Plain keys, Shift-only combos and key-ups are discarded before they reach the buffer. The monitor runs only for assessment-purpose sessions, only if the OS already granted listening, and its permission prompt lives in the Sentinel wizard — never mid-assessment. Text-entry chords are refused before the buffer: Option+key on macOS, AltGr (Ctrl+Alt) on Windows and Linux.
12. **Camera device label is not persisted**: the virtual-camera check keeps the `MediaStreamTrack` label in the frontend dev view only; snapshots carry the boolean verdict.
13. **Liveness keeps nothing**: the face patch is built in memory from the same frame the gaze check already holds; only `real_prob` and a boolean cross IPC.
14. **Attestation carries no device identity**: the platform key is per-app, per-install, and the blobs stored on the session bind a nonce and an app id, not a person or a device serial. A companion phone's binding adds the learner's own DID and the phone's app endpoint id, which the learner's credential already names or which identify only the app install.
15. **Consent is per capability**: camera, microphone and companion phone are each a separate choice, recorded on the session. No capability is switched on because the device has it, and a trust profile that needs an unconsented capability cannot be selected.
16. **Voice checks are never transcribed or stored**: they listen to the call's own microphone stream in memory, and only onset times and whether a voice was the first or another leave the detector. They run only in a running interview-purpose session with microphone consent, stop while muted, and the panel receives timings and cues only, during the call.
17. **Generated probes send only what the learner consented to**: under a sponsor's `llm` probe policy, the skill name and description and the text of the option(s) the learner chose go to the sponsor's model connection, and only after the learner consents at attempt start. The item's prompt, other options and key never leave; the outbound text passes the item-bank guard first, and the age and guardian rules for AI connections apply.
18. **Response timing stays aggregate**: the per-item input timeline stays in the webview; only per-visit aggregates (times, counts) reach the backend, and nothing about which answer was chosen.
19. **Corpus capture is never from ordinary use**: a window is written to the measurement corpus only when the app process was started with the capture variable set, for a participant who signed the capture consent; it holds the same aggregates the backend already receives and no keystrokes, pointer paths, frames, audio or answers.

The live preview uses the unlocked profile and current device fingerprint when
loading gaze calibration, just like assessment monitoring. Preview frames remain
excluded from assessment evidence staging.
