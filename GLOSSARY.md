# Alexandria app

Status: live
Updated: 2026-11-12

The desktop and mobile app: learning, assessment, credentials and the integrity checks behind them. This glossary holds the app's domain vocabulary; terms enter it as they are settled.

## Language

### Sentinel trust

**Integrity session**:
One monitored stretch of work (an assessment attempt or an interview check) whose observations are committed in order and summarised into the credential it backs.
_Avoid_: Proctoring session, monitoring session

**Trust profile**:
The fixed set of signals an integrity session runs, chosen once at session start and locked for the session: `core`, `attested`, `camera` or `full`.
_Avoid_: Monitoring level, signal mode, proctoring tier

**Policy floor**:
The minimum trust profile a sponsor's policy requires, met when the session's profile includes every signal set the floor names.
_Avoid_: Minimum level, required profile

**Degradation**:
A signal the locked trust profile includes that was lost or stopped counting during the session; it is recorded against the session, never resolved by switching profile.
_Avoid_: Downgrade, fallback

**Assurance level**:
What the device can prove about the client that produced a session: `local` (self-reported) or `device_attested` (a platform attestation verified at issuance). The device axis of trust.
_Avoid_: Trust level, security level

**Companion phone**:
The learner's own attested phone joined to a desktop session, which lends the session its platform attestation.
_Avoid_: Paired device, second device

**Verification rung**:
Positive evidence that the learner did the work, independent of the device, on a strict ladder: `none`, `process`, `probed`, `consistent`, `live`. The verification axis of trust.
_Avoid_: Verification level, proof tier

**Response-process signals**:
Timing and revision features of how a learner responded to each item, measured the same way for a click, typed text, a plugin event or a spoken answer.
_Avoid_: Speed checks, typing signals

**Longitudinal consistency**:
Agreement between an attempt's outcome and the learner's own recent practice, lessons and earlier attempts on the same skill.
_Avoid_: Practice divergence, history score

**Probe**:
A short, time-boxed follow-up question made from the learner's own submission and graded by the same grader as the item.
_Avoid_: Follow-up item, challenge question

**Probe policy**:
How an assessment or role produces its probes: `none`, `reseed`, `authored`, `plugin` or `llm`.
_Avoid_: Follow-up mode

**Generated probe**:
A probe written by a sponsor-configured model connection under the `llm` probe policy.
_Avoid_: AI question, LLM follow-up

**Live segment**:
An unscripted part of an interview held in Alexandria that the panel marks done; the evidence for the `live` rung.
_Avoid_: Live check, oral exam

**Voice checks**:
Listening to an interview call's microphone for when speech starts and whether a second voice is present, without recording or transcribing anything.
_Avoid_: Audio monitoring, voice recording

**Interview signal feed**:
The live timings and cues a candidate's integrity check sends the panel during an interview held in Alexandria.
_Avoid_: Interview telemetry

### Measurement

**Signal**:
One named observation an integrity session can raise, such as a flag or a response-process feature.
_Avoid_: Check, detector

**Channel**:
A group of signals that are one piece of evidence; the decision rule counts channels, not signals.
_Avoid_: Signal family, category

**Advisory signal**:
A signal with no certified error rate: it can send a session to review, never decide it.
_Avoid_: Soft flag, unweighted signal

**Review status**:
The outcome of an integrity session that raised a warning or critical flag, or one channel over threshold: a reason for a person to look, not a verdict.
_Avoid_: Soft-flagged, pending

**Measurement corpus**:
Labelled sessions, scripted red-team attacks and consented clean sessions, that signals' error rates are measured on.
_Avoid_: Training data, dataset

**Certification**:
A reviewed record admitting signals to a trust profile with their measured rates, the operating threshold and the corpus they were measured on. A signal outside it stays advisory.
_Avoid_: Calibration, validation

**Coverage statement**:
A fixed, content-addressed text per trust profile and verification rung stating what a session checked, what it did not, the platform ceilings and the certified rates.
_Avoid_: Integrity summary, disclosure
