# Sentinel trust profiles and verified credentials

Status: active
Updated: 2026-11-13
Triage: ready-for-agent
Repos: alexandria

Reassessment of Sentinel against three goals: a reliable signal for cheating or its absence, trust in every credential Alexandria issues, and running on every supported device rather than only the newest. The conclusion is that trust comes from attestation, verification and measurement, not from the number of signals. This spec turns that conclusion into work. Background on the current system is in `sentinel.md`; the assessment integrity rules it extends are in `ai-chat-spec.md` §13A.

## Problem Statement

A learner earns a credential in Alexandria and shows it to an employer or school. The verifier sees an integrity assertion with an assurance level of `local`, a score, and some flag counts. They cannot tell what was actually checked, how often those checks are wrong, or whether the device that produced them could have been lying. Free learning platforms issue certificates with no integrity data at all, so the verifier's only reference point is "trust the issuer's logo", which is exactly what Alexandria set out to replace.

From the learner's side, the experience is uneven and sometimes unfair:

- Thirty-four flags fire on hand-picked thresholds that have never been measured against real sessions. A learner who looks down at their keyboard can be flagged for glancing at a device. A learner on an old phone sees the camera checks stutter and gets flagged for being absent.
- The camera pipeline runs full face detection, liveness and gaze every second regardless of what the device can afford, so on a mid-range phone or a small laptop the assessment itself lags.
- The learner only finds out at credential time that their session did not satisfy a sponsor's policy, after the effort is spent.
- The honest learner on a desktop has no way to prove their client was not tampered with, so their credential can never rise above `local` however carefully they behaved.

From the sponsor's side, every flag is an opinion with no error rate attached, so sponsors either review everything or trust nothing. Off-device help (a second person, a second phone) is invisible to monitoring, and the product does not say so plainly, which is the real liability.

## Solution

Sentinel stops being a growing list of client-reported flags and becomes a small set of named **trust profiles**, each with a stated coverage, selected per session as the highest profile the device can verifiably and affordably satisfy and the policy and consent permit. Trust is raised along two independent axes that the credential records separately:

- **Device axis**: what the hardware can prove about the client. `local` (self-reported) → `device_attested` (hardware attestation on mobile, or a desktop paired with an attested phone).
- **Verification axis**: positive evidence that the learner did the work, independent of hardware. Response-process signals that work on any input type, an adaptive item draw, follow-up probes generated from the learner's own answers, and a **longitudinal consistency** score across the learner's history on the platform. A live, unscripted human segment is the top rung.

Every signal admitted to a profile has a measured error rate from a **measurement corpus** of red-team and consented clean sessions. Until measured, a signal is advisory: it informs review, never a verdict. Credential-affecting outcomes need two independent channels or a failed verification.

The camera becomes a policy-gated option inside the `camera` and `full` profiles, with a device-budget check so it is never enabled on a device that cannot run it. The `core` profile has no camera and runs on everything.

Verifiers, sponsors and learners read the same **coverage statement** per rung: what was checked, what was not, and what the measured error rates were when the profile was certified.

## User Stories

### Learner

1. As a learner, I want my credential to state exactly what was checked during my assessment, so that a verifier can trust it for what it is rather than guess.
2. As a learner, I want to know before I start an attempt whether my device and setup satisfy the sponsor's policy, so that I do not spend an hour and then learn the credential cannot be issued.
3. As a learner on an older phone or a small laptop, I want the assessment to run smoothly, so that the integrity checks do not themselves cause lag or false absence flags.
4. As a learner, I want to be told which trust profile my session is running under and why, so that I can choose to move to a better device or add a paired phone if the credential matters to me.
5. As a learner without a camera, I want to still earn a credential at an honest trust level, so that missing hardware does not lock me out.
6. As a learner, I want the camera to be used only when the sponsor's policy requires it and I have consented, so that biometric processing is never switched on just because my device has a camera.
7. As a learner, I want to reach the highest trust rung through verification steps (follow-up questions, consistency over time) rather than only through hardware, so that trust is not something only people with new devices can afford.
8. As a learner who looks down at the keyboard while typing, I want not to be flagged for glancing at a device, so that my habits are not mistaken for cheating.
9. As a learner, I want a single flag to lead to review rather than an automatic verdict, so that one false positive cannot cost me a credential.
10. As a learner, I want my months of lessons and practice to count toward the trust in my credential, so that consistent work is rewarded over a single exam performance.
11. As a learner who covers the camera or loses a signal mid-session, I want the session to record that the signal was lost rather than silently continue as if it had never been enabled, so that the record stays honest and so that nobody else can game it either.
12. As a learner, I want follow-up questions after an attempt to be about my own answers and short, so that verification costs me minutes, not another exam.
13. As a learner on a desktop, I want to pair my phone as an attested companion, so that my desktop session can reach `device_attested` even though desktops have no hardware attestation.
14. As a learner, I want to see which signals were measured and which are still advisory, so that I understand what is counted against me.
15. As a learner, I want to export my credential and have the coverage statement travel with it, so that a verifier offline still knows what the assurance means.

### Sponsor and instructor

16. As a sponsor, I want to set a minimum trust profile and verification rung per role assessment, so that the credential I rely on was earned under conditions I chose.
17. As a sponsor, I want a per-session summary ranked by calibrated suspicion rather than a flat list of flags, so that my reviewers look at the top decile instead of everything.
18. As a sponsor, I want the error rate of each signal at the time the profile was certified, so that I can decide how much weight to give a flag.
19. As a sponsor, I want to require a probe (follow-up) step for a role, so that relayed or dictated answers are costly to produce.
20. As a sponsor, I want to know plainly that monitoring cannot see a second person or a second device, so that I layer my own interview where it matters and am not misled.
21. As an instructor authoring items, I want to attach follow-up probes and seeded variants to an item, so that the same construct can be re-asked in a different surface form.
22. As an instructor running a panel interview in Alexandria, I want a live feed of relay latency, gaze-before-answer and second-voice presence on the candidate device, so that I can ask an unscripted follow-up at the right moment.
23. As an instructor, I want interview follow-ups authored per criterion to be suggested to me during the interview, so that verification does not depend on an external model.
24. As a sponsor, I want the policy floor evaluated when the learner starts the attempt, so that a learner on a device that cannot satisfy it is told then, not at issuance.
25. As a sponsor, I want interviews held by video call, phone or in person to carry no Sentinel assurance unless a paired attested phone was present, so that the credential never implies monitoring that did not happen.

### Verifier

26. As a verifier, I want the credential's integrity assertion to name the trust profile and verification rung separately, so that I can distinguish "attested device, no probe" from "unattested device, probe passed".
27. As a verifier, I want to resolve the coverage statement for a rung from the credential alone, so that I do not have to trust a web page that may have changed.
28. As a verifier, I want the assertion to say which signals ran under which profile and which were degraded mid-session, so that a clean score on a degraded session is not mistaken for a clean session.

### Maintainer

29. As a maintainer, I want every flag's precision and recall measured on a corpus before it can affect a credential, so that I never ship an unmeasured penalty.
30. As a maintainer, I want an offline evaluation command that replays recorded sessions through the scoring logic and prints per-flag metrics, so that threshold changes are reviewable in CI.
31. As a maintainer, I want the scoring and fusion logic to live in Rust behind a Tauri-free boundary, so that the offline evaluator, the app and the tests use one implementation.
32. As a maintainer, I want the set of profiles to be small and fixed, so that the test matrix across six platforms stays finite and sessions stay comparable.
33. As a maintainer, I want unmeasured advisory models that do not move precision to be deleted rather than kept, so that low-end devices do not pay for signals that add nothing.
34. As a maintainer, I want camera frames to cross the IPC boundary as binary, so that a frame costs kilobytes rather than a megabyte of JSON per tick.
35. As a maintainer, I want face detection to run at the frame's own resolution and track between detections, so that the camera profile fits a mid-range device's budget.
36. As a maintainer, I want a device benchmark at session start, so that a profile is never selected on a device that cannot sustain it.
37. As a maintainer, I want the probe mechanism to be a contract that graders and plugins implement, so that Sentinel never has to know the item format.
38. As a maintainer, I want the red-team corpus contributions to be explicit, opt-in, labelled sessions, so that honest learners' data never leaves their device for model improvement.

## Implementation Decisions

### Vocabulary

- **Trust profile**: the fixed set of signals a session runs. Four profiles: `core`, `attested`, `camera`, `full`. Each has a published coverage statement and, once certified, per-signal error rates.
- **Verification rung**: positive evidence independent of hardware. `none` → `process` (response-process signals and adaptive draw satisfied) → `probed` (probe policy satisfied) → `consistent` (longitudinal consistency satisfied) → `live` (unscripted human segment in an Alexandria-hosted interview).
- **Assurance level** keeps its current meaning on the device axis (`local`, `device_attested`) and the current re-verification at issuance.
- **Coverage statement**: a versioned, content-addressed document per profile and rung stating what is checked, what is not, platform ceilings and certified error rates. Its digest is carried in the assertion.
- **Measurement corpus**: labelled sessions (red-team scripted cheats, consented clean sessions) stored as snapshot sequences with ground-truth labels. Synthetic holdouts remain allowed for contract tests, never for certification.
- **Response-process signals**: input-agnostic timing and revision features (time to first input against a reading floor, time to commit, revision count, idle-then-burst, answer change after idle). They apply to a click as much as to typed text.
- **Longitudinal consistency**: agreement between an attempt's outcome and the learner's prior practice, lesson engagement and earlier attempts on the same skill, computed locally from the profile database.
- **Probe**: a short follow-up produced from the learner's own submission under a time box, graded by the same grader contract as the item.

These terms are candidates for the glossary; none exist there today.

### Profile selection and lifecycle

- At session start the backend, not the client, decides the profile. Order: attestation result → capability report → device budget benchmark → policy floor and consent → highest permitted profile. The chosen profile, the inputs to the choice and the benchmark result are written as the session's first committed snapshot, so they are in the commitment chain.
- The profile is locked for the session. Loss of a signal afterwards (camera covered, permission revoked, inference thread killed, monitor count changed) is a degradation flag on the session, never a renegotiation. A degraded session cannot report a clean camera coverage.
- Capability absence is never treated as clean. A signal a profile requires that the device cannot provide excludes that profile. A signal the profile does not include is recorded as "not run" in the assertion.
- Policy floor is evaluated at attempt start and at interview-check acceptance, and the learner is told the outcome before the first item is served. It is evaluated again at issuance as today; the two evaluations must agree or issuance fails closed.
- Consent is per capability (camera, microphone, companion phone) and is recorded in the session. A profile that needs an unconsented capability is not selectable.
- The device benchmark is a short, fixed workload (one face-detection pass on a synthetic frame, one liveness pass) with a published budget per profile. A device over budget cannot select `camera` or `full`.

### Signals and measurement

- Scoring and fusion move out of the frontend composable into Rust, in a module with no Tauri dependency, so the app, the offline evaluator and tests share one implementation (consistent with the Tauri-free runtime boundary ADR). The composable becomes a collector that submits raw per-window features; the backend derives scores and flags. Severity stays authoritative on the backend as today.
- Fusion replaces the fixed weighted average and per-flag penalties with per-signal log-likelihood ratios accumulated over the session and compared to a per-profile threshold. Likelihood ratios come from the measurement corpus. A signal without a measured ratio contributes zero to the decision and is shown as advisory.
- Cross-modal correlation is a first-class feature: a gaze-down or second-voice event followed by a response event within a short window is a distinct feature with its own ratio, rather than two independent flags.
- Decision rule for credential-affecting outcomes: at least two independent channels over threshold, or a failed verification step. A single flag, whatever its severity, sets the session to review, not suspended. Auto-suspend remains only for the backend-recorded tamper flags (in-app assistant or external assistant reaching the backend during an attempt).
- Response-process signals are implemented first: they need no camera, no consent change, no new permissions, and they work for every item type including choice items. The reading-time floor per item comes from item length and difficulty; the per-learner baseline comes from the first items of the attempt or from recent practice.
- Adaptive draw: the draw excludes items previously served to the profile, weights by exposure, requires the bank's transfer items, and picks the next item from the difficulty band suggested by prior correctness. Interface and storage already exist for the drawn items; this changes the selection, not the schema.
- Longitudinal consistency: a per-skill score from recent independent practice outcomes, lesson engagement and prior attempts, compared with the attempt outcome. Large divergence (practice fails consistently, assessment passes within hours) feeds the fusion as a measured feature, replacing the planned info-level divergence flag.
- Existing advisory models (keystroke autoencoder, mouse trajectory network, local-binary-pattern face embedder) are evaluated on the corpus. Any that fail to move precision at the chosen operating point are removed along with their bundled artifacts and wizard steps.
- The flag catalog is consolidated to the signals that survive measurement. Retired flags follow the existing retirement pattern: the backend rejects them in new snapshots, historical rows are preserved and not rescored.

### Measurement corpus and evaluation

- A corpus entry is a session's full snapshot sequence plus per-window raw features, a ground-truth label (which attack, or clean), the profile it ran under, platform and device class. Entries come from scripted red-team sessions run by maintainers and from consented clean sessions where the participant opts in explicitly at session start, separately from ordinary use. Nothing is collected from ordinary sessions.
- An offline evaluation command replays corpus entries through the Rust scoring module and prints per-signal precision, recall and likelihood ratio per profile, plus the fusion's confusion matrix at the configured threshold. It runs in CI against the committed corpus manifest and fails if a signal admitted to a profile falls below its certified rate.
- Certification is a reviewed manifest: profile, signal set, operating thresholds, measured rates, corpus digest, date. The coverage statement embeds the manifest's digest. Changing a threshold means re-certifying.
- Existing holdout infrastructure (committee-held encrypted evaluation sets) is reused for real-world holdout sets; synthetic generation stays for contract tests only.

### Probes

- A probe policy per assessment or role: `none`, `reseed` (same item, new seed, time proportional to the first attempt), `authored` (instructor-written follow-ups per item), `plugin` (the grader generates a perturbation of the learner's own artefact and grades the predicted outcome), `llm` (generated from the submission by a sponsor-configured model connection). Policies may be combined per item type.
- The grader contract gains an optional probe envelope beside the grade envelope: input is the item content and the learner's submission, output is a probe question with a machine-gradable expectation and a time box. Native graders (choice items) and wasm graders implement it the same way, so Sentinel never knows the item format.
- Probes run inside the integrity session after submission, before the session ends. Time box starts when the probe renders. Latency to first input on a probe is itself a response-process feature.
- Probe outcome is recorded as a verification result in the snapshot chain: satisfied, failed, skipped (provider unavailable, time out), with the policy used. Skipped never counts against the learner; it keeps the rung at `process`.
- The `llm` policy is a separate run type from chat so the attempt gate still refuses chat during an attempt. It is one-shot, grounded only in the submission and the skill description, passes the item-bank guard before leaving the device, and requires consent at attempt start because the submission leaves the device. The existing structured-decision provider may score probe answers only if its approved-task list is extended with a reviewed task and held-out evaluation; it does not generate probes.
- Interview mode: probes become suggested follow-ups to the interviewer, drawn from criteria-authored follow-ups, alongside a live signal feed (relay latency against the candidate's own baseline, gaze-down before answer, second-voice presence from voice activity detection without transcription or storage). Interviews held outside Alexandria get no Sentinel assurance unless a paired attested phone was present.

### Device axis

- Mobile attestation already built is run and verified on devices; remaining inputs (platform capability enablement and response keys in the network profile) are operator tasks tracked in Further Notes.
- A desktop session may pair a learner's attested phone over the existing peer-to-peer transport. The phone contributes its attestation to the desktop session's commitment chain, and, under `camera` or `full`, acts as the camera, running detection locally and sending only scores. A desktop with a paired attested phone reaches `device_attested`.
- Whether the desktop Apple attestation API covers the macOS build is checked as the first task on this axis; if it does, macOS desktop reaches `device_attested` without pairing.

### Credential and policy surface

- The integrity assertion gains the trust profile, the verification rung, the coverage statement digest, the list of signals run and degraded, and the probe outcome summary. Assurance level stays as is. The assertion remains inside the signed envelope and is re-derived at issuance from the session, never copied from stored client text.
- Issuance policy gains a minimum trust profile, a minimum verification rung and a probe policy. The existing camera coverage minimum becomes a property of the `camera` and `full` profiles rather than a separate knob.
- Role assessments expose the three new policy fields in the sponsor UI, with a plain-language coverage statement next to each profile and rung. The sponsor's session summary ranks sessions by fusion posterior and shows per-signal certified rates.
- The credential detail view and the exported bundle show the coverage statement resolved from its digest, bundled with the app so it is available offline.

### Performance

- Camera frames cross the IPC boundary as binary, not as JSON arrays of numbers. Capture moves to the native side where the existing camera stack allows it, so frames never enter the webview.
- Face detection runs at the frame's own resolution (re-exported model or a runtime that accepts dynamic shapes), with detection on a slow cadence and landmark tracking between detections. Liveness runs on a trigger (new face track, confidence drop) rather than every tick.
- Inference runs on its own thread with frame dropping rather than queueing, so the assessment UI never waits on a model.
- Overlay and display scans become event-driven where the platform offers notifications, replacing per-window polling and closing the gap on brief toggles.
- Budgets per profile are published and checked by the benchmark at start, so `core` has no model inference at all and `camera` fits a mid-range phone.

### Sequence

Increments, each a sub-ticket when picked up:

1. Measurement corpus format, red-team scripts, offline evaluator, CI gate.
2. Scoring and fusion in Rust behind the Tauri-free boundary; composable becomes a collector; per-signal advisory status.
3. Response-process signals and adaptive draw.
4. Trust profiles: selection at start, lock, degradation flags, benchmark, policy floor at attempt start, assertion and policy fields, coverage statements, sponsor UI.
5. Longitudinal consistency.
6. Probe contract with `reseed`, `authored`, `plugin`; interview signal feed and suggested follow-ups.
7. Device axis: mobile attestation runtime verification; macOS attestation check; paired phone.
8. Camera profile performance work; advisory model pruning from corpus results.
9. `llm` probe policy, enterprise only.

## Testing Decisions

A good test drives a whole integrity session through its public boundary and asserts on what a verifier or sponsor would see: the committed snapshots, the session outcome, the assertion built at issuance, the evaluator's printed metrics. Tests do not inspect fusion internals, thresholds or model outputs directly.

Seams, highest first. Existing seams are preferred; one new seam is proposed.

1. **Integrity session lifecycle over an in-memory database** (existing). Start a session with a given capability report, attestation result and policy; submit snapshots carrying raw features; end the session; read the committed chain and outcome. Profile selection, locking, degradation, fusion decisions, response-process scoring and longitudinal consistency are all asserted here. Prior art: the inline tests in the integrity command module that build a session, submit snapshots and assert on status and counters, and the attestation tests that mint synthetic attestation material.
2. **Credential issuance with a policy over an in-memory database** (existing). Issue against a finished session and assert on the integrity assertion fields and on refusal reasons. Prior art: the credential and verifiable-credential test modules that exercise the issuance gate.
3. **Grader contract envelope** (existing, extended). A native and a wasm grader each produce a probe from a submission and grade a response; tests assert the envelope round-trips and that the host treats the probe's details as opaque. Prior art: the grade envelope tests and the bundled plugin manifest and grader checks.
4. **Offline evaluator over a corpus manifest** (new). Feed a fixture corpus with known labels and assert the printed per-signal rates and the fail-closed behaviour when a certified rate is not met. This is the one new seam; it is also the only place where thresholds are visible, as inputs rather than assertions. Prior art: the offline evaluation example in the structured-decision crate, which takes a manifest and labelled examples and reports metrics.
5. **Frontend collector** (existing vitest). Assert that the composable submits the expected raw features for a sequence of DOM events and that it reports degradation events; nothing about scores. Existing composable tests shrink as scoring leaves the frontend.

Platform-native probes (overlay, process, hotkey, display, attestation on device) remain verified by the existing per-platform recipes and the development live panel; they are inputs to the seam-1 tests as recorded capability reports, not re-tested through the UI.

## Out of Scope

- Cloud-side changes: hosted coverage statements, sponsor dashboards in the hosted service, and the applicant tracking surface. The coverage statement digest is designed so Cloud can serve the same document later.
- Website verifier changes to display the new assertion fields. Follows once the field set is frozen.
- Kernel-level or driver-level anti-cheat, mandatory room scans, continuous recording, audio transcription or storage, emotion or attention inference. Excluded by design and by law.
- Face recognition against an identity document. Blocked on a commercial licence and on the enterprise identity-match decision.
- Federated or differentially private model training across users. The corpus here is explicit, opt-in and labelled; it is not a federation scheme.
- Detecting AI-written prose in open-ended answers.
- Voice-answer and asynchronous video item types; noted as a future format in the per-format analysis, not built here.
- Windows and Linux Wayland runtime verification beyond what the tagged pipeline already covers.

## Further Notes

- **Honesty is the product.** Sentinel cannot see a second person or a second device. The coverage statement says so for every rung. Overclaiming once would cost more trust than every signal gains.
- **Why fixed profiles rather than dynamic signal selection.** Every optional signal doubles the calibration space and makes sessions incomparable; an attacker can also toggle capabilities to confuse the record. Four named profiles keep the measurement and test matrix finite and let "dynamic" mean "highest honest profile" rather than "everything the device has".
- **Trust is not monotonic in signal count.** Adding an unmeasured, high-false-positive signal lowers the reliability of the whole. Admission to a profile requires a measured likelihood ratio.
- **Equity.** Hardware raises the floor; verification raises the ceiling. The highest rung must stay reachable on the cheapest device through verification steps and a live segment.
- **Legal.** Camera and microphone are consent-gated per session and never enabled by capability alone. Voice activity detection runs without transcription or storage. No attention or emotion inference.
- **Operator inputs still needed** for the device axis: Apple App Attest capability enabled on the app identifier, and Play Integrity response keys placed in the preprod network profile. Android stays `local` until the keys are present.
- **Relation to earlier decisions.** Consistent with the issuer-bound credential authority ADR: probes and verification rungs change what an issuer's policy requires; they grant no third party any power over a credential. Consistent with the Tauri-free runtime boundary ADR: scoring moves behind that boundary so the evaluator can link it without a desktop stack.
- The `sentinel.md` taxonomy, flag rules, platform table, assurance section and privacy guarantees need an update when increments 2 and 4 land; the schema document regenerates from migrations in CI.

## Children

Sub-tickets live in `sentinel-trust-profiles/`; each names its blockers.

| Ticket | Blocked by | Triage |
|---|---|---|
| [01 Scoring and fusion in Rust](sentinel-trust-profiles/01-scoring-core.md) | none | done |
| [02 Measurement corpus and evaluator](sentinel-trust-profiles/02-measurement-corpus.md) | 01 | done |
| [03 Response-process signals](sentinel-trust-profiles/03-response-process-signals.md) | 01 | done |
| [04 Adaptive draw](sentinel-trust-profiles/04-adaptive-draw.md) | none | done |
| [05 Trust profiles](sentinel-trust-profiles/05-trust-profiles.md) | 01 | done |
| [06 Longitudinal consistency](sentinel-trust-profiles/06-longitudinal-consistency.md) | 01 | done |
| [07 Probe contract](sentinel-trust-profiles/07-probe-contract.md) | none | done |
| [08 Credential surface](sentinel-trust-profiles/08-credential-surface.md) | 03, 05, 06, 07 | done |
| [09 Sponsor and learner UI](sentinel-trust-profiles/09-sponsor-learner-ui.md) | 08 | done |
| [10 Interview signal feed](sentinel-trust-profiles/10-interview-signal-feed.md) | 03, 07 | done |
| [11 Camera performance](sentinel-trust-profiles/11-camera-performance.md) | none | done |
| [12 Desktop device axis](sentinel-trust-profiles/12-desktop-device-axis.md) | 05 | done |
| [13 LLM probe policy](sentinel-trust-profiles/13-llm-probe-policy.md) | 07, 08 | done |
| [14 Real corpus and pruning](sentinel-trust-profiles/14-real-corpus-and-pruning.md) | 02 | ready-for-human |
| [15 Docs and glossary](sentinel-trust-profiles/15-docs-and-glossary.md) | 02, 04, 09–13 | done |
| [16 Device runtime verification](sentinel-trust-profiles/16-device-runtime-verification.md) | 12 | ready-for-human |

## Decisions

Owner answers, 2026-11-12:

- **Test seams confirmed** as listed in Testing Decisions, including the one new seam (offline evaluator over a corpus manifest).
- **Glossary approved.** Create the repo glossary with this spec's vocabulary (ticket 15).
- **Doc updates approved.** `sentinel.md`, the protocol spec integrity sections, the runbook and the generated schema document are updated as part of this work, not proposed separately (ticket 15; schema regeneration with each migration).
- **Probe generation.** Mechanical probes (reseed, authored, plugin) come first. The structured-decision provider never generates probes and may only score probe answers through a newly approved, evaluated task. Generated probes use a sponsor-configured model connection, enterprise only.

Owner actions, not agent work (ticket 16):

1. Enable the App Attest capability on the app identifier in the Apple developer portal, then regenerate the provisioning profiles the iOS build signs with. The entitlement is already in the iOS project.
2. Link the Android app to a Google Cloud project in Play Console under App integrity, turn on the Play Integrity API, and choose to manage response encryption yourself. Download the decryption and verification keys.
3. Put both keys, base64, into `play_integrity_response_keys` in the preprod network profile. The profile loader rejects anything that is not base64.
4. Install a Play-distributed build (internal testing track) on an Android device and a signed build on an iPhone, run an assessment, and read the assurance level in the Sentinel dev panel.
5. Run the red-team and clean-session protocol from the ticket-02 runbook to build the first real corpus (ticket 14).

## Notes

2026-11-12, cross-repo follow-up (ticket 15):

- **Republish `alexandria-verify`.** The integrity assertion gained `trustProfile`, `verificationRung`, `coverageStatementDigest`, `signalsRun`, `signalsDegraded`, `signalsNotRun` and `probe` on `alexandria_verify::vc::IntegrityAssertion`. Verifiers re-serialise the typed credential to check its proof, so a verifier on the published 0.5.0 drops these fields and rejects every credential that carries them. Publish a new minor of the crate (the added public fields break struct-literal construction), then move Cloud and the website verifier to it, before credentials with these fields leave the device. The website's display of the new fields stays out of scope until then.
- Doc updates for the work landed in `sentinel.md`, the protocol specification (§12, §14.7, §14.9.5), `ai-chat-spec.md` §13A, `skills-and-reputation.md`, `project-structure.md`, both Sentinel runbooks and the new repo `GLOSSARY.md`. `interview-assistant.md` still describes the panel without the live signal feed, authored follow-ups and **Ask now**; `sentinel.md` §Candidate integrity check covers them.

- 2026-11-13: tickets 01–13 and 15 implemented on feat/sentinel-trust-profiles and closed; a code review found eleven issues, all fixed on the same branch. Tickets 14 and 16 stay open for the owner. Spec stays active until they close.
