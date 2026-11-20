# Assessment hardening: answers the issuer holds, an honest companion level and probes that cost something

Status: draft
Updated: 2026-11-20
Triage: ready-for-agent
Repos: alexandria, alexandria-cloud

A reassessment of how easy it is to cheat an assessment after the trust profiles and credential trust work landed. It found four ways to earn a credential dishonestly that need little or no skill, one of which earns the top device label and an organisation's signature. This spec closes the ones that can be closed, and labels the one that cannot. Background: [sentinel-trust-profiles-spec.md](sentinel-trust-profiles-spec.md), [credential-trust-spec.md](credential-trust-spec.md), the course access spec (in review on its own branch), [enterprise-boundary.md](enterprise-boundary.md), [ADR 0006](adr/0006-checkable-integrity-evidence.md), [ADR 0008](adr/0008-hosted-organisation-issued-credentials.md). Vocabulary: [GLOSSARY.md](../GLOSSARY.md).

## Problem Statement

An organisation invites a candidate to a role assessment and signs the result. Today that signature can be earned without knowing the material:

- **The answers are on the candidate's device.** Every item and its answer key is in the app's local data, and the bundled banks are in the open-source repository. A candidate can read the answers before starting.
- **The device reports its own score.** The app is open source and a desktop cannot prove which build is running. A modified build can report any score and a clean session, and nothing downstream can dispute it.
- **A companion phone vouches for a desktop it cannot see.** The phone attests whatever digest the desktop hands it. A modified desktop build paired with a genuine phone earns `device_attested`, passes a policy that requires it, and receives the organisation's signature.
- **A second device beside the screen is invisible.** No signal carries weight until it is certified, so relayed answers end in a `completed` session. The default probe asks the same item again, with a time limit taken from the attempt's own pace, so the delay of relaying is already allowed for.
- **A renamed cheat tool is not recognised.** The process watchlist matches by file name.

An employer reading the credential cannot tell any of this happened.

## Solution

- **The issuer holds the answers.** An organisation can keep a role's items and answers in the hosted service. The candidate's device receives one question at a time, never an answer. The hosted service grades, times and probes the attempt itself, and ignores any score the device reports.
- **Every credential says where the answers were.** A new fact, answer custody, is `issuer` or `device`. An organisation's policy requires `issuer` unless the organisation chooses otherwise, in plain sight.
- **A companion phone gets its own level.** A desktop session backed by a companion phone is `companion_attested`: a genuine phone was present, nothing is claimed about the desktop. It no longer satisfies a policy that requires `device_attested`. A role that requires `device_attested` is taken on the phone, and the app says so before the first question.
- **Probes ask something new, against a fixed clock.** Asking the same item again no longer earns the `probed` rung. A probe's time limit comes from the item, never from the attempt.
- **Policies can require a rung.** An organisation's role requires `probed` by default and can require `live`. Below `live`, every surface says a second device or person is not ruled out, because only a person asking unscripted questions rules it out.
- **The watchlist matches who signed a tool**, so renaming it changes nothing.

Self-issued credentials stay the holder's own word and are scored as such. The hardening is for credentials an organisation signs.

## User Stories

### Verifier

1. As a verifier, I want a credential to state whether the issuer or the candidate's device held the answers, so that I know whether the candidate could have read them.
2. As a verifier, I want a credential to state who computed the score, so that I know whether it is the candidate's device's word.
3. As a verifier, I want a desktop session backed by a phone to be labelled as that and nothing more, so that I do not mistake it for a verified device.
4. As a verifier, I want `device_attested` to mean the attempt itself ran on a verified device, so that the label means one thing.
5. As a verifier, I want every credential below the `live` rung to say that a second device or person was not ruled out, so that I do not read more into it than was checked.
6. As a verifier, I want to check all of this offline with the free verifier library, so that I depend on no service.
7. As a verifier, I want a credential earned from a bank the candidate's device held to be scored as before, so that honest self-study still counts for what it is.
8. As a verifier, I want the `probed` rung to mean the candidate answered a question they had not already seen, so that the rung is evidence of understanding.

### Candidate

9. As a candidate, I want to see before I start what device the role requires, so that I do not finish an attempt that cannot count.
10. As a candidate on a desktop, I want to be told to take the attempt on my phone when the role requires a verified device, so that I am not refused afterwards.
11. As a candidate, I want to see before I start that my answers and the time I took on each go to the organisation, so that I can decline.
12. As a candidate, I want to see the retake rule before I start, so that I know what abandoning an attempt costs.
13. As a candidate, I want a dropped connection to let me resume at the same question for a short while, so that a bad network does not fail me.
14. As a candidate, I want to know that the clock kept running while I was disconnected, so that the rule is the same for everyone.
15. As a candidate, I want a probe to tell me its time limit before it starts, so that I can pace myself.
16. As a candidate, I want my score to come from the organisation, so that nobody can say I wrote it myself.
17. As a candidate, I want the organisation's credential delivered to my app as it is today, so that I own it.
18. As a candidate, I want my answers and timings deleted when the run's retention period ends, so that they are not kept forever.
19. As a candidate, I want a role's questions never stored on my device, so that I am not holding material I could be blamed for leaking.
20. As a candidate, I want to be told plainly why a credential was not issued, so that I know whether to retake.

### Learner

21. As a learner studying alone, I want bundled assessments to work offline as they do now, so that hardening for employers does not cost me anything.
22. As a learner, I want my self-issued credential to say honestly that the answers were on my device, so that it claims nothing false.
23. As a learner on a desktop, I want the app to stop telling me that pairing my phone raises my credential's trust, so that I am not misled.
24. As a learner, I want a failed probe to count against the session as it does now, so that the rules I know still hold.
25. As a learner in a private course, I want the course's assessments to work with no hosted service, so that a private course stays self-contained.

### Organisation administrator

26. As an administrator, I want to import a role's items and answers into the hosted service from a file, so that I can use questions we already have.
27. As an administrator, I want to be told at import that the hosted service will hold these items and answers, so that I choose that knowingly.
28. As an administrator, I want to list and retire items, so that I can remove a leaked or wrong question.
29. As an administrator, I want a role refused publication when its bank is too small for the draw, so that candidates do not all see the same questions.
30. As an administrator, I want to see how often each item has been shown, with a warning when one is overexposed, so that I know when to replace it.
31. As an administrator, I want to set how many retakes a candidate gets and how long they wait, so that a bank cannot be harvested by repetition.
32. As an administrator, I want only an invited candidate with an open attempt to receive items, so that nobody else can read the bank.
33. As an administrator, I want to require a verification rung for a role, so that I decide how much evidence a credential needs.
34. As an administrator, I want to require a live segment for high-stakes roles, so that a second device in the room is addressed by a person.
35. As an administrator, I want a role to require issuer-held answers unless I choose otherwise, so that the safe choice is the default.
36. As an administrator, I want the hosted service to keep holding no private-course key and no private-course content, so that importing role items changes nothing about our private courses.
37. As an administrator, I want code items in a hosted bank graded with test cases the candidate never sees, so that practical roles get the same protection.
38. As an administrator issuing from my own device without the hosted service, I want that path to keep working with the answers-on-device fact stated, so that I am not forced onto the hosted service.

### Reviewer

39. As a reviewer, I want to see the hosted service's own per-item timings for a run, so that I can judge a suspicious attempt from timings the candidate could not alter.
40. As a reviewer, I want a probe answered past its time limit shown as a failed probe, so that I do not have to work it out.
41. As a reviewer, I want timings shown without an automatic verdict until thresholds are measured, so that nobody is accused by an unmeasured number.

### Maintainer

42. As a maintainer, I want the new level, the new fact and the new policy requirements judged by the one shared policy rule in the verifier library, so that the app, the hosted service and independent verifiers agree.
43. As a maintainer, I want a forged score sent by a client to be ignored and tested as such, so that the main claim of this spec is under test.
44. As a maintainer, I want the words private and public kept for course visibility, so that one word does not mean two things.
45. As a maintainer, I want the watchlist to match a tool's signing identity on macOS and Windows, so that a rename does not defeat it.
46. As a maintainer, I want the Android rule recorded now, so that turning Android attestation on later needs no new decision.
47. As a maintainer, I want new coverage statement versions added and none replaced, so that every credential ever issued still resolves.
48. As a maintainer, I want each phase shippable alone, so that the companion fix does not wait on hosted banks.

## Implementation Decisions

### Vocabulary

New glossary terms, added with the code that introduces them:

- **Answer custody**: who held an assessment's answers during the attempt, `issuer` or `device`. `issuer` only when the answers never left the issuer.
- **Hosted item bank**: the items and answers an organisation imports into the hosted service for a role.
- **Hosted attempt**: an attempt whose items the hosted service draws, delivers one at a time, times and grades.
- **Companion-attested**: the assurance level of a desktop session backed by a companion phone.
- **Required rung**: the lowest verification rung an issuance policy accepts.

The words private and public stay reserved for course visibility, as the course access spec defines it.

### Scope of trust

- Hardening targets organisation-issued credentials. A self-issued credential is signed by a key its holder owns on a device its holder controls, and no change here can make it more than a self-claim.
- Every credential, self-issued or not, gains the answer custody fact and the changed probe rules.

### Companion-attested

- The assurance level gains a third value, `companion_attested`, ordered between `local` and `device_attested`.
- A desktop session backed by a companion phone states `companion_attested`. The verifier establishes the verified level from the companion binding the evidence already carries: verified evidence with a companion binding is `companion_attested`, never `device_attested`.
- `device_attested` now means the attempt ran in the attested app on the attested device.
- A policy that requires `device_attested` is not met by `companion_attested`. A policy may require `companion_attested`, which either higher level meets.
- A self-issued credential with `companion_attested` takes the plain self-claim weight. The attested weight applies only to `device_attested`. The calculation version advances.
- The app removes every statement that pairing a phone raises a credential's trust. Pairing remains available, and the session states what it is.
- The definition of the companion phone in the glossary changes from lending its attestation to being present and attested during the session.

### A role that requires a verified device

- When a role's policy requires `device_attested`, the app refuses to start the attempt on a desktop, before any item is shown, and says the role must be taken on a phone.
- The invite screen shows the device requirement, the required rung, the retake rule and the consent text before the candidate accepts.
- Until Android attestation is enabled, this means an iPhone. The screen says so.

### Answer custody

- The integrity assertion states answer custody. The device writes `device`. Only an organisation-issued credential from a hosted attempt states `issuer`, and the issuer writes it.
- A verifier treats `issuer` on a self-issued credential as invalid: nobody can hold answers away from themselves.
- The issuance policy gains a required answer custody. In the hosted service a new role requires `issuer`. An administrator can set it to `device`; the console shows that choice on the role and on every run.
- Role credential issuance from an organisation's own device has `device` custody by nature. The role form there states that candidates' devices hold the answers, and the credential says so.
- Bundled banks and any bank held in a profile have `device` custody, however few people hold them.
- An assessment inside a private course has `device` custody: members hold the course key and so the answers. That the bank is members-only is already conveyed by the course summary, and this spec adds nothing to the credential about course visibility.

### Hosted item banks

- An item bank belongs to one organisation and one role. Items carry a skill, a difficulty, the question, the options, the correct options and optionally a time limit.
- An administrator imports items from a JSON or CSV file, lists them and retires them. There is no editor in this spec. Import states that the hosted service will hold the items and answers. Each action is audited.
- Nothing moves from a course into a hosted bank on its own. The hosted service still holds no private-course key, content or membership.
- A role with a hosted bank cannot be published unless every skill it draws from holds at least three times the number of items drawn per attempt for that skill. The refusal names the skill and the shortfall.
- The service counts how often each item has been shown. The console shows the count and warns when an item is overexposed. The warning decides nothing.
- A retired item is never drawn again. Attempts that already saw it keep their record.
- Hosted item banks and hosted attempts are Enterprise in the enterprise boundary: a hosted process an organisation runs. Checking the resulting credential stays Core, free and offline.

### Hosted attempts

- A candidate with an accepted invite to a role opens a hosted attempt. Every request is signed by the candidate's key, as the existing signed fetches are.
- The service draws the attempt's items when it opens. The draw is random within each skill and difficulty band. Option order is shuffled per attempt.
- The service sends one item at a time: the question, the options and the time limit. It sends the next item only after it has received an answer to the current one. It never sends an answer or says whether an answer was right until the attempt is closed.
- The service stamps its own clock when it sends an item and when it receives the answer. These timings are the run's record. Timings the device reports are kept beside them and decide nothing.
- The service grades. A score the device sends is ignored.
- **Disconnection.** An attempt stays open for ten minutes after the last request. Within that window the candidate resumes at the same item and the item's clock has kept running. After it the attempt is closed as abandoned and counts against the retake limit.
- **Retakes.** The role sets a retake limit and a wait between attempts. The default is one retake after seven days. A retake draws again and prefers items the candidate has not seen.
- **Who receives items.** Only the invited candidate, only while their attempt is open. Any other request is refused without saying whether the role or bank exists.
- **On the device.** Item text is held in memory while shown. It is never written to the profile database or to logs, and the existing guard that keeps bank items away from the in-app assistant covers it. The same rule the course access spec applies to readable private-course content.
- **Integrity session.** The device runs an integrity session for a hosted attempt as for any other. Sharing pauses, as it does for every assessment. When the attempt closes, the device sends its integrity assertion and, on an attested phone, platform evidence over a digest of the session's commitment root, the candidate and the hosted attempt.
- **Attested requests.** When the role requires `device_attested`, each answer request carries a platform assertion over the request, so the service knows each answer came from the genuine app on that device. A request without a valid assertion is refused.
- **Consent.** Before the first item the candidate sees and accepts: answers and per-item timings go to the organisation's hosted service; the device requirement; the retake rule; the retention period. Without acceptance no item is sent.
- **Retention.** Answers and timings are kept with the run for the run's retention period and deleted with it.

### Probes

- Under the `reseed` policy a probe still runs and a failed one still flags the session. It no longer earns the `probed` rung; the session stays at `process`.
- The `probed` rung needs a question the candidate has not already answered: an `authored` or `plugin` variant, or a generated probe.
- A probe's time limit is fixed by the item: the limit its author set, or a default from the item's length and difficulty using the same reading-time model the response-process signals use. It is never derived from the attempt's pace.
- In a hosted attempt a probe is another item on the same skill that the candidate has not seen, drawn, timed and graded by the service. The bank floor guarantees one exists. A probe answered after the service's time limit is a failed probe.
- The verification rung of a hosted attempt is the service's finding, not the device's.

### Timings

- One rule decides: a probe answered past the service's time limit fails, and a failed probe flags the session as today.
- Every other timing is recorded and shown to the organisation's reviewer with no automatic outcome. Thresholds come from the measurement corpus and a certification, as for every other signal.

### Required rung

- The issuance policy gains a required rung, judged by the shared rule against the rung the issuer established.
- In the hosted service a new role requires `probed`. An administrator can lower it or raise it to `consistent` or `live`.
- A role that requires `live` reuses the existing interview flow unchanged: the credential is issued once the panel marks a live segment done for that candidate and role. Nothing is added to interviews.
- Every coverage statement for a rung below `live` says a second device or person was not ruled out. New statement versions are added to the registry; none is replaced.

### Issuance from a hosted attempt

- When a hosted attempt closes, the service judges the role's issuance policy with the shared rule, using its own score, its own probe outcome and rung, its own verification of the platform evidence, and the device's integrity assertion for session status and trust profile.
- When the policy is met the service issues a role credential signed with the organisation's hosted key, as [ADR 0008](adr/0008-hosted-organisation-issued-credentials.md) describes, with these differences: there is no candidate credential to name, so the evidence entry names the hosted attempt instead and states answer custody `issuer`, that the issuer graded, the probe outcome, and the attestation outcome and verified assurance level the service found.
- When the policy is not met the run records the first unmet requirement and nothing is issued. The candidate's app shows the reason.
- The existing path, where a candidate shares a self-issued assessment credential and the service issues on it, stays for roles whose policy accepts `device` custody.
- This is a new architectural decision and is recorded as an ADR with the first ticket that implements it: for a hosted attempt the issuer holds the answers and computes the outcome, and the candidate's device contributes integrity evidence only.

### Item types in hosted banks

- Choice items first.
- Code and other plugin items follow as the last phase: the candidate's submission is sent to the service, which runs the same bundled grader with test cases the device never receives. This phase may ship after the rest.
- Free-text items graded by a model are not part of hosted banks.

### Watchlist

- On macOS a watched tool is matched by the team identifier of its code signature, and on Windows by the publisher of its signature, in addition to the existing name match. An unsigned process is matched by name as today.
- A match raises the same flag at the same severity as today.
- Linux keeps name matching. Wayland overlay checks and virtual machine detection are not added.

### Android

- When Android attestation is enabled, `device_attested` requires the strong integrity verdict, which is hardware-backed. The basic device verdict alone yields `local`.
- A phone that cannot give the strong verdict still reaches the highest rung through probes and a live segment, as the equity rule in the trust profiles spec requires.
- Nothing is built for Android in this spec.

### Verifier library

- The library gains the third assurance level, the answer custody fact and its validity rule, the two new policy requirements, the hosted attempt evidence entry, the new weights and the new coverage statement versions.
- The integrity assertion's format changes, so the library's version advances to the next minor and the assertion format version advances with it.
- Project data is disposable before launch: existing credentials and seeds are replaced, not migrated.

### Sequence

Each phase ships alone.

1. Verifier library: the assurance level, answer custody, required rung and custody in the policy rule, weights, statement versions, vectors.
2. App: the companion level and wording, refusal on a desktop for roles that require a verified device, the probe rules, answer custody in the assertion.
3. Watchlist signing identity match.
4. Hosted service: hosted item banks with import, list, retire, the size floor and exposure counts, and the console page.
5. Hosted attempts: the service's attempt, delivery, grading, timing, probes, disconnection and retakes; the app's flow with consent and the in-memory rule; attested requests.
6. Issuance from a hosted attempt, the ADR, the console's run view with timings, and the app's refusal reasons.
7. Plugin items in hosted banks.
8. Documentation and glossary.

## Testing Decisions

A good test here drives what a candidate's device, an administrator or a verifier can observe: a request goes in and an item, a refusal, a score or a credential comes out. Tests do not assert on table shapes, internal function calls or message layouts beyond the published formats.

Four seams, all of which exist:

1. **Hosted service HTTP API against a real database.** The bulk of the tests. The attempt lifecycle; one item at a time and no answer in any response; grading; a forged score and forged timings sent by the client are ignored; probes drawn from unseen items and failed when late; resume inside the window and abandonment after it; retake limit and wait; refusal for anyone but the invited candidate; the bank size floor; exposure counts; import and retire; each issuance refusal code; the issued credential's contents. Prior art: the service's existing integration tests for the assessment exchange, hiring and issued credentials.
2. **Verifier library's public check, with fixed vectors and no I/O.** `companion_attested` from evidence with a companion binding; a policy requiring `device_attested` refused for it; answer custody and the invalid self-issued `issuer` case; required rung and required custody; the hosted attempt evidence entry; weights; statement resolution. The hosted service's signed output is checked against the same vectors. Prior art: the existing attestation, policy and credential vectors.
3. **App issuance policy and probe modules, with an in-memory database.** A reseed probe leaves the rung at `process` and a failed one still flags; the fixed time limit does not move with the attempt's pace; a desktop refuses to start an attempt for a role that requires a verified device; a companion-backed session states `companion_attested`; hosted item text is absent from the database after an attempt. Prior art: the inline tests beside the probe, issuance and companion code.
4. **Watchlist matcher as a pure function.** A renamed process with a watched signing identity matches; an unsigned process falls back to the name. Prior art: the existing matcher tests.

Real platform evidence cannot be produced in CI. Vectors use synthetic roots, and the check on real devices joins the existing owner task for device attestation.

## Out of Scope

- The phone as a witness: a companion phone that watches the desk and so gives a desktop session real backing. It is the way pairing regains value and deserves its own spec.
- Wayland overlay and hotkey checks, virtual machine detection, capture hardware detection.
- The measurement corpus and the first certification, which have their own ticket in the trust profiles spec.
- Enabling Android attestation.
- An item editor in the console.
- An adaptive draw in hosted attempts.
- Model-graded free-text items in hosted banks.
- Any change to private courses, their keys, their membership, their completion rule or the course summary. The hosted service holding private-course content stays out of scope everywhere.
- Linking a course's assessment to a hosted bank automatically.
- Attestation evidence in interview integrity reports.
- Making self-issued credentials resistant to a modified build.

## Further Notes

- **What this does not solve.** A second device or person beside the screen is still not detected below the `live` rung. Hosted timings and new-item probes raise the cost of relaying; they do not stop a fast relay on choice items. The credential says so.
- **What a hosted attempt on a desktop proves.** The answers were not on the device and the score is the issuer's. It does not prove what software showed the questions; a modified desktop build can still report a clean session. That is why a role can require a verified device.
- **Reconciled with the course access spec.** An earlier answer in this design called answer custody "item exposure" with values private and public, and placed every organisation's bank in the hosted service. Both were changed: the words collide with course visibility, and the course access spec promises that the hosted service holds no private-course content and that private courses need no hosted service. Hosted banks are therefore role material an administrator imports deliberately, and a private course's assessments keep `device` custody.
- **Assumed, not asked.** The random draw in hosted attempts, the attested requests rule, the ADR and the Enterprise placement of hosted banks follow from the decisions taken and were not put as separate questions. Each can be revisited before its phase starts.
- **Default policies get stricter.** A hosted role now requires `issuer` custody and the `probed` rung unless an administrator changes it. Roles that relied on `reseed` to reach `probed` need a different probe policy or a lower required rung.
- **Deployment.** The verifier library, the app and the hosted service change the assertion format together, as the last two specs did.
- **Documentation to update with each phase**, proposed alongside the code: the glossary, the Sentinel document and runbook, the interview assistant document for the required-rung wait, the enterprise boundary rows, the protocol specification for the assertion format, the database schema reference, and the hosted service's organisation document.

## Decisions

Taken on 2026-11-20:

| Question | Decision |
|---|---|
| Which credentials are hardened | Organisation-issued. Self-issued stay self-claims. |
| Companion-backed desktop | Its own assurance level, `companion_attested`. The phone as a witness is a later spec. |
| Companion weight | No bonus over a plain self-claim. |
| Role requires a verified device, candidate on a desktop | Refuse before the first item; take it on the phone. |
| Where an organisation's answers live | In the hosted service, which grades. Attempts need a connection. |
| Bundled and profile-held banks | Kept; labelled `device` custody; refused by an organisation's policy by default. |
| Probes | Same-item probes no longer earn `probed`; fixed time limit per item. |
| Second device | Required rung in the policy; `probed` by default, `live` by choice; stated plainly below `live`. |
| Android | Strong integrity required for `device_attested`; decision only. |
| Desktop evasion | Signing identity match only. |
| Item delivery | One at a time, timed by the service. |
| Hosted item types | Choice first, plugin items last. |
| Probes in hosted attempts | Another unseen item on the same skill. |
| Bank floor | Three times the draw per skill; exposure warning is advisory. |
| Who fetches items | The invited candidate with an open attempt; one retake after seven days by default. |
| Authoring | Import, list, retire. |
| Score authority | The issuer. |
| Items on the device | Memory only. |
| Disconnection | Ten minutes, clock keeps running, then abandoned. |
| Service timings | One hard rule for late probes; the rest shown, not decided. |
| What counts as issuer custody | Answers never left the issuer. |
| `live` rung | Existing interview flow, plus the policy wait. |
| Consent and retention | Shown before the first item; kept with the run. |
