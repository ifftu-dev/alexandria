# Alexandria app

Status: live
Updated: 2026-11-21

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
What the issuing device established about the client that produced a session, on a ladder: `local` (self-reported), `companion_attested` or `device_attested` (the attempt ran in the attested app on the attested device). The device axis of trust, as the credential states it; a reader relies on the verified assurance level instead.
_Avoid_: Trust level, security level

**Companion-attested**:
The assurance level of a desktop session backed by a companion phone: a genuine phone was present, and nothing is claimed about the desktop. It never meets a requirement for `device_attested`.
_Avoid_: Phone-attested, device-attested by pairing

**Camera coverage**:
The fraction of a camera session's committed windows that carried camera checks, stated in the credential; the camera counts towards a policy floor at 90% or more.
_Avoid_: Camera uptime, camera score

**Companion phone**:
The learner's own attested phone, present and attested during a desktop session.
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
A short follow-up question made from the learner's own submission, graded by the same grader as the item, under a time limit the item fixes. Only a probe asking something the learner had not already answered counts towards the `probed` rung.
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

**Coverage statement registry**:
Every coverage statement version ever published, held in the verifier library so any digest resolves offline; a text change adds a version and never replaces one.
_Avoid_: Statement table, statement cache

### Credential trust

**Self-issued credential**:
A credential whose issuer is its subject: the holder's own word, scored as a self-claim whatever type it declares.
_Avoid_: Self-signed credential, learner credential

**Organisation-issued credential**:
A credential signed by a key that an organisation's domain lists, naming the organisation as issuer.
_Avoid_: Sponsor credential, cloud credential

**Organisation backing**:
Whether an organisation's domain descriptor lists the key that signed a credential. Removing the key withdraws backing from everything it signed; the signatures still verify.
_Avoid_: Organisation verification, endorsement

**Attestation evidence**:
The platform's own signed material for a session (App Attest or Play Integrity), carried in the credential so any verifier can check it.
_Avoid_: Attestation proof, device proof

**Credential-bound**:
Attestation evidence over this credential's subject, claim and integrity assertion, so it cannot be moved to another credential. The only evidence that raises the verified assurance level.
_Avoid_: Fully bound, strong attestation

**Session-bound**:
Attestation evidence that covers only the session's own attestation, not this credential's content. It never rejects and never raises the verified assurance level.
_Avoid_: Weak attestation, partial binding

**Verified assurance level**:
The assurance level a verifier established from the attestation evidence, as opposed to the level the credential states.
_Avoid_: Real assurance, checked level

**Assessed credential evidence**:
The entry in a role credential naming the assessment credential it was issued on, with the attestation outcome and verified assurance level the issuer found for it.
_Avoid_: Source credential, parent credential

**Issuance policy**:
The bounds a role or a credential request sets on an integrity assertion before a credential is issued, judged by one shared rule that returns the first unmet requirement as a stable refusal code.
_Avoid_: Integrity gate, role rules

**Required rung**:
The lowest verification rung an issuance policy accepts.
_Avoid_: Minimum proof, rung floor

**Answer custody**:
Who held an assessment's answers during the attempt: `issuer`, only when the answers never left the issuer, or `device`. Stated in every integrity assertion; a self-issued credential stating `issuer` is invalid.
_Avoid_: Item exposure, private or public bank

**Hosted item bank**:
The items and answers an organisation imports into the hosted service for one role.
_Avoid_: Cloud bank, private bank

**Hosted attempt**:
An attempt whose items the hosted service draws, delivers one at a time, times and grades, including code items it grades with a bundled grader over test cases the device never receives. The candidate's device only shows the items and contributes integrity evidence.
_Avoid_: Online attempt, remote assessment

**Suspension list**:
The issuer's published status list of suspended credentials, read by every verifier; suspension is reversible, revocation is not.
_Avoid_: Suspension flag, hold list
