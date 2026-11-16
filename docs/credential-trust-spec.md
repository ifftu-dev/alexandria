# Credential trust: checkable integrity evidence and organisation-issued credentials

Status: active
Updated: 2026-11-16
Triage: ready-for-agent
Repos: alexandria, alexandria-cloud

A review of how credentials and Sentinel fit together, against three goals in this order: trust in a credential now and years from now, transparency to anyone who views one, and interoperability with existing standards. The review found that everything Sentinel says about a session reaches the outside world as the holder's own word. This spec makes that word checkable where the platform allows it, says so plainly where it does not, and gives organisations a working way to put their own signature on a result. Background: [sentinel.md](sentinel.md), [skills-and-reputation.md](skills-and-reputation.md), [ADR 0001](adr/0001-vc-first-credentials.md), [ADR 0003](adr/0003-credential-authority.md). Vocabulary: [GLOSSARY.md](../GLOSSARY.md).

## Problem Statement

A learner passes an assessment and shows the credential to an employer. The credential carries an integrity assertion: a session status, a trust profile, a verification rung and an assurance level. The employer has no way to tell whether any of it is true.

- **The learner signs their own assessment credential.** The signing key comes from the learner's own recovery phrase. Anyone who holds a key can write a credential that says the session completed cleanly on an attested device, sign it, and have it verify. The app re-derives the assertion from the integrity session at issuance, which protects against a tampered record inside the app, not against a credential written outside it.
- **`device_attested` is a word, not evidence.** The platform attestation is verified only on the device that issues. The credential carries the resulting level and nothing a reader can check. The coverage statement says the attestation was "verified again when the credential was issued" without saying that the holder's own device did the verifying.
- **Scoring trusts the declared type.** A self-issued assessment credential is weighted above an organisation-signed role credential, because weight follows the credential type the issuer wrote down.
- **An organisation's hiring record rests on the same self-report.** When a candidate answers an assessment request, the hosted service reads the session status and score straight from the candidate's credential and uses them to decide whether a review case opens. A forged clean result produces an unflagged run.
- **No organisation can sign a result it did not run on its own device.** Role credential issuance reads attempts and integrity sessions from the issuer's local database, and nothing carries a candidate's session there. The issuance command also accepts any subject with any local session, without checking that the session is that subject's attempt for that role.
- **A suspended credential looks valid to everyone but its issuer.** Suspension is a local flag. The published status list carries revocation only.
- **An old credential can name a coverage statement nobody can read.** Earlier statement texts are not kept; only an exported bundle carries the text its credentials name.
- **Personal key rotation is half-built and wrong.** Nothing can trigger a rotation, the verifier chooses the issuer key by verification time so a rotation would invalidate every earlier credential, the inbound handler records a rotation without the new key, and a verifier outside Alexandria resolves a `did:key` from the identifier and ignores all of it.
- **The published verifier's own documentation is out of date** on the session statuses, the achievable assurance levels and the proof format.

## Solution

A credential says only what a reader can check, and says who is vouching for it.

- **Attestation evidence travels with the credential.** A session that ran on an attested device puts the platform attestation in the credential, bound to that credential's own content, so any verifier can check it against the platform's root without trusting the holder. A credential without it is labelled as self-reported, everywhere it is shown and everywhere it is scored.
- **Weight follows verified provenance, not the declared type.** A self-issued credential counts as a self-claim unless its attestation evidence verifies. Session status affects scoring only where the status itself is checkable.
- **An organisation can sign a candidate's result.** The hosted service already holds each organisation's signing key and proves it against the organisation's domain. It verifies the candidate's share, including the attestation evidence, applies the organisation's issuance policy, and issues an organisation-issued credential. The organisation's run record shows what was verified, not what was claimed.
- **Suspension is published** in a status list of its own, the way revocation is.
- **Every coverage statement ever issued stays readable**, offline from the verifier library and online from a stable address, and the statement text says who verified what.
- **A `did:key` is its key.** The rotation overlay is deleted. Organisations rotate by editing the key list on their domain.

## User Stories

### Verifier

1. As a verifier, I want to check a credential's device attestation myself, so that I do not have to trust the holder's statement that their device was genuine.
2. As a verifier, I want the verification result to say whether attestation evidence was supplied, whether it verified, and what it was bound to, so that I can tell a proven claim from an asserted one.
3. As a verifier, I want attestation evidence that is bound to this credential's subject, claim and integrity assertion, so that evidence from one honest session cannot be attached to a different result.
4. As a verifier, I want a credential with no attestation evidence to be reported as self-reported and never as failed, so that honest learners on desktops are not treated as cheats.
5. As a verifier, I want invalid attestation evidence to be a rejection with a stable reason code, so that a forged attachment is distinguishable from a missing one.
6. As a verifier, I want to see whether the issuer is the subject, so that I know when a credential is someone's statement about themselves.
7. As a verifier, I want to see that an issuer's key is listed by an organisation's domain, with the name and domain, so that I know which organisation stands behind the credential.
8. As a verifier, I want a credential whose signing key is no longer listed by the domain to be reported as lacking organisation backing while its signature still verifies, so that I can see both facts.
9. As a verifier, I want suspension to appear in a published status list, so that I learn a credential is suspended without asking its issuer.
10. As a verifier, I want a missing suspension list to make the result pending, so that an unreachable list is never read as "not suspended".
11. As a verifier, I want to read the coverage statement a credential names, whatever version it is, so that I know what the session checked and what it did not.
12. As a verifier, I want the statement to say that attestation was verified by the issuing device unless I have checked the evidence myself, so that I am not misled about who did the checking.
13. As a verifier working offline, I want the verifier library to resolve every statement digest without the network, so that verification survives the service that published it.
14. As a verifier using standard tooling, I want an issuer's `did:key` to mean exactly the key in the identifier, so that my result matches Alexandria's.
15. As an integrator, I want the verifier library's documentation to match its behaviour, so that I do not build on a status value or a proof format that no longer exists.
16. As an integrator, I want test vectors for every new outcome, so that my implementation in another language can be shown to agree.
17. As an integrator, I want attestation checking to be optional in the verifier library, so that a minimal verifier does not have to carry certificate-chain code.

### Learner

18. As a learner on an attested phone, I want my credential to carry proof of that, so that it is worth more than a bare claim.
19. As a learner on a desktop with a companion phone, I want the phone's attestation to back my credential, so that pairing is worth the effort.
20. As a learner, I want the attestation evidence not to identify my device across credentials, so that two credentials cannot be linked to one device by the evidence alone.
21. As a learner, I want my credential detail view to tell me whether my result is self-reported, device-attested or organisation-issued, so that I know what a reader will see.
22. As a learner, I want to be told when attestation evidence could not be bound to the credential, so that I understand why the credential reads as self-reported.
23. As a learner, I want a flagged or suspended session not to lower my standing when nobody could have checked that status anyway, so that honesty is not penalised relative to forgery.
24. As a learner, I want my self-issued credentials to keep verifying after this change in format, so that nothing I hold today silently breaks.
25. As a learner, I want my exported bundle to carry everything a verifier needs, including evidence and statements, so that my credential works without any Alexandria service.

### Candidate

26. As a candidate, I want the organisation that assessed me to be able to sign my result, so that I hold a credential that is more than my own word.
27. As a candidate, I want to receive the organisation-issued credential on my device, so that I own it and can present it elsewhere.
28. As a candidate, I want to know before I start what the organisation's policy requires, so that I do not finish an attempt that could never be issued.
29. As a candidate, I want a refusal to say which requirement was not met, so that I know whether pairing a phone or retaking would help.
30. As a candidate, I want my share to contain no more than it does today plus the attestation evidence, so that no behavioural data leaves my device.

### Organisation

31. As an organisation reviewer, I want each run to show whether the integrity result was verified or only reported, so that I weigh it correctly.
32. As an organisation reviewer, I want a run whose attestation evidence is invalid to be refused and recorded, so that a forged result does not enter the console as a clean one.
33. As an organisation administrator, I want to set an issuance policy on a role that the service enforces, so that our signature goes only on results that meet our bar.
34. As an organisation administrator, I want to require a verified assurance level, so that we never sign a self-reported session as device-attested.
35. As an organisation administrator, I want to revoke or suspend a credential we issued, so that we can withdraw our backing.
36. As an organisation administrator, I want rotating our signing key to be an edit to our domain record, so that we are not dependent on anyone else to recover from a lost key.
37. As an organisation administrator, I want to understand that removing a key withdraws our backing from everything it signed, so that I rotate deliberately.
38. As a sponsor issuing from my own device, I want issuance refused when the integrity session is not the named subject's attempt for that role, so that I cannot sign the wrong person's session by mistake.
39. As a sponsor, I want the same policy rule applied on my device and in the hosted service, so that a role means one thing.

### Issuer

40. As an issuer, I want to suspend a credential and have every verifier see it, so that suspension has an effect.
41. As an issuer, I want to reinstate a suspended credential, so that suspension stays reversible.
42. As an issuer, I want a timed suspension to lift when its time comes, so that I do not have to remember to reinstate it.
43. As an issuer, I want revocation to stay permanent and separate from suspension, so that the two purposes are never confused.

### Maintainer

44. As a maintainer, I want scoring to depend on verified provenance, so that a change in declared type cannot inflate a score.
45. As a maintainer, I want derived skill states and reputation rows to recompute when the scoring rule changes, so that old scores do not linger under a new rule.
46. As a maintainer, I want one policy rule over verifier-visible facts, shared by the app and the hosted service, so that the two cannot drift.
47. As a maintainer, I want the rotation code removed rather than left dormant, so that the published verifier has no path that would break credentials if someone enabled it.
48. As a maintainer, I want coverage statement text changes to add a version rather than replace one, so that changing a sentence cannot orphan issued credentials.
49. As a maintainer, I want the verifier library to stay free of I/O, so that the guarantee the library exists for survives this change.
50. As a maintainer, I want the hosted service to pick up the new verifier in the same change that depends on it, so that app and service never disagree about a credential.

## Implementation Decisions

### Vocabulary

These terms are used below. None is in the glossary yet; they are candidates for it.

- **Attestation evidence**: the platform's own signed material for a session (an App Attest attestation and assertion, or a Play Integrity token), carried in the credential.
- **Binding**: what the attestation evidence covers. *Credential-bound* evidence covers this credential's subject, claim and integrity assertion. *Session-bound* evidence covers only the session's terminal commitment root.
- **Self-issued credential**: one whose issuer is its subject.
- **Organisation-issued credential**: one signed by a key that an organisation's domain lists.
- **Verified assurance level**: the assurance level a verifier established from the evidence, as opposed to the level the credential states.

### Attestation evidence in the credential

- A credential issued against an integrity session with a verifying attestation carries the attestation evidence in the standard `evidence` property of the credential, as one typed entry naming the platform, the evidence material, the binding kind and, for a companion phone, the companion binding.
- At issuance the device computes a binding digest over a domain tag, the session's terminal commitment root, the credential subject, the claim and the integrity assertion, and obtains platform evidence over that digest: a fresh assertion from the session's attested key on iOS, a fresh integrity token on Android. For a session backed by a companion phone, the desktop asks the phone for it over the companion link before the link closes.
- When evidence over the binding digest cannot be obtained (the phone has gone, the platform call fails), the credential carries the session's existing evidence as session-bound. It is never upgraded later.
- The attested key stays per session, as today, so the evidence does not link credentials to a device.
- The integrity assertion keeps stating the assurance level the issuing device established. The verified assurance level is a separate fact, computed by whoever verifies.
- Evidence counts against the existing credential size limits. If it does not fit, the limits are raised deliberately and the limit vectors updated; evidence is never truncated.

### Verification of attestation evidence

- Attestation checking moves from the app into the verifier library as an optional feature, with the same checks the app performs today and the trust inputs (platform root, app identifier, Play Integrity response keys) supplied by the caller. The app passes them from the network profile. The library still performs no I/O.
- The verification result gains an attestation outcome: not supplied, verified credential-bound, verified session-bound, unverifiable (the caller supplied no trust inputs for that platform), or invalid with a stable reason code.
- Invalid evidence rejects the credential. Not supplied, session-bound and unverifiable never reject; they leave the verified assurance level at `local`.
- The verified assurance level is `device_attested` only for verified credential-bound evidence.
- The provenance classification gains the same outcome, so a caller asking "what kind of credential is this" gets self-issued or issuer-signed together with what the device proved.

### Scoring by verified provenance

- Aggregation weight follows verified provenance. A self-issued credential takes the self-claim weight whatever type it declares. A self-issued credential with verified credential-bound evidence takes a new weight that sits above the self-claim weight and below every issuer-signed weight; its initial value is chosen in the ticket and recorded with the calculation version.
- Session status affects scoring only for credentials whose status a reader can rely on: those with verified credential-bound evidence, and organisation-issued credentials. For these, a `suspended` or `flagged` session contributes nothing and a `review` session counts in full. For every other credential the status is ignored, because it is the holder's word either way.
- A self-issued credential still adds no independent issuer cluster.
- The derived-state calculation version and the reputation computation specification both advance, and cached states and rows are recomputed under the existing fingerprint mechanism.

### One issuance policy rule

- The part of the issuance policy that can be judged from verifier-visible facts (session status, score and flag bounds, trust profile and policy floor from the signals run and degraded, verification rung, probe outcome, required assurance level against the verified level) moves into the verifier library as one function over the policy, the integrity assertion and the attestation outcome.
- The app keeps the checks that need the session itself: that the floor judged at attempt start agrees with the one judged at issuance, and that the assertion is re-derived from the session.
- A policy that requires `device_attested` is satisfied only by a verified assurance level.

### Organisation-issued credentials from the hosted service

- When a candidate answers an assessment request, the service verifies the share as today and additionally verifies the attestation evidence with the deployment's trust inputs.
- The run record stores the verified assurance level and the attestation outcome beside the reported status, and the console shows each run as verified or self-reported. Invalid evidence refuses the share and is audited.
- The review-case rule keeps reading the reported status, and additionally opens a case when the candidate's credential states `device_attested` and the evidence does not verify credential-bound.
- A role carries an issuance policy. When the candidate's credential is accepted and the policy is satisfied, the service issues a role credential signed with the organisation's hosted key, with the candidate as subject, the candidate's integrity assertion unchanged, the attestation evidence, and an evidence entry naming the candidate's credential by identifier and integrity hash. When the policy is not satisfied the run records the first unmet requirement and nothing is issued.
- The service never issues to a subject whose key is one of the organisation's own.
- The issued credential takes a status entry for revocation and one for suspension in lists the service hosts for the organisation's key. An administrator can revoke, suspend and reinstate; each action is audited. This is consistent with [ADR 0003](adr/0003-credential-authority.md): the organisation is the issuer.
- The candidate's app collects the issued credential through the existing signed fetch for the subject and stores it like any received credential.
- The credential's issuer may be an object carrying the signing key's identifier with the organisation's name and domain. The verifier accepts the string and object forms.
- Organisation backing is checked against the organisation's domain descriptor, which the caller resolves and supplies. The result reports verified with name and domain, key not listed, or descriptor not supplied. A key removed from the domain's list withdraws organisation backing from everything it signed; the signatures still verify as plain keys.

### Role credential issuance on a sponsor's own device

- Issuance requires that the integrity session belongs to an attempt made for that role and that the attempt's subject is the credential's subject. Any mismatch refuses issuance with a specific reason.
- The policy is judged through the shared rule.

### Suspension in a status list

- Every credential carries two status entries: revocation and suspension, each in a list of that purpose, both Bitstring Status List entries. This brings the implementation in line with [ADR 0001](adr/0001-vc-first-credentials.md), which already names the status list for both.
- Suspend sets the bit, reinstate clears it, and a timed suspension is lifted by the issuer's node when its time comes. Verifiers read only the bit.
- The suspension list is published and hosted exactly as the revocation list is, and travels in bundles.
- The verifier reads suspension from the list. Its lookup for a locally recorded suspension is removed. A missing or unreachable suspension list is a pending reason, as for revocation.
- The issuer's local suspension columns remain as the issuer's own bookkeeping (reason, until) and are not consulted by verification.

### Coverage statements stay resolvable

- The statement registry moves into the verifier library and keeps every text version ever published. A text change adds a version; no version is removed.
- The credential names its statement by the existing digest in the integrity assertion and additionally by the standard `relatedResource` property, with a resolvable address and a digest of the same bytes.
- The address is built from the profile's configured status host, `{host}/coverage-statements/{digest}`, and served there next to the status lists. With no status host it is `urn:alexandria:coverage-statement:{digest}`, as status lists fall back to a URN. No network profile field is added.
- The attestation line of the statement is reworded to say that the issuing device verified the attestation, and that a reader relies on it only as far as the credential's evidence verifies. This is a new statement version.

### Personal key rotation is deleted

- A `did:key` resolves to the key in its identifier, always. The rotation function, the rotated-issuer constructor, the rotation announcement handling, the validity windows and the rotated-by link are removed, in the app and the verifier library.
- The key registry remains only as the lookup for identifiers that are not self-resolving. A registry row can no longer change how a `did:key` resolves, and a row without a key can no longer be written.
- Bundles stop exporting rotation history.
- Organisation key rotation is unchanged: the domain record lists the keys allowed to sign.

### Verifier library housekeeping

- The documentation of the integrity assertion, the issuance policy and the credential envelope is corrected: the `review` status exists, `device_attested` is achievable, and the proof is a Data Integrity proof.
- The library takes a new minor version with a breaking-change note, the interoperability vectors are regenerated and extended, and the hosted service updates its vendored copy in the change that first depends on it.

### Surfaces

- The app's credential detail view and bundle view state, in plain words, one of: self-reported, device-attested and verified, or issued by a named organisation, with the suspension state and the resolved coverage statement.
- The hosted console's run view states verified or self-reported and, when issuance was refused, which requirement was unmet.

### Sequence

Each is a sub-ticket when picked up.

1. Scoring by verified provenance for self-issued credentials (the self-claim rule; no wire change).
2. Coverage statement registry in the verifier library, `relatedResource`, hosting, and the reworded statement version.
3. Suspension status entry and list.
4. Delete personal key rotation.
5. Attestation checking in the verifier library; attestation evidence and binding at issuance; attestation outcome in the result and the classification.
6. Attested weight and the session-status scoring rule (needs 1 and 5).
7. Shared issuance policy rule (needs 5).
8. Hosted service: verify evidence on share, verified run record, review-case rule (needs 5).
9. Hosted service: role issuance policy, organisation-issued credential, status entries, organisation backing in the verification result (needs 3, 7, 8).
10. Sponsor-device issuance binding (needs 7).
11. Surfaces in the app and the console (needs 5, 9).
12. Verifier library release, vendored copy, documentation (with each increment that changes the wire format; closing pass at the end).

## Testing Decisions

A good test states what a party outside the code would observe: the verification result for a given credential and inputs, the credential an issuance produced or the reason it was refused, the run an organisation's console would show. Tests do not reach into certificate parsing, weight tables or status-list bit arithmetic directly.

Three seams, all existing. No new seam is added.

1. **Verifier interoperability vectors** (highest; existing). Each vector is self-contained: a credential, the state verification needs, the verification time, the policy and the expected result. New vectors cover: evidence not supplied; verified credential-bound; verified session-bound; unverifiable without trust inputs; invalid evidence with each reason code; evidence lifted from another credential; a suspended, a reinstated and a suspension-list-missing credential; a `did:key` issuer with stale registry state that must not affect resolution; organisation backing verified, key not listed and descriptor not supplied; each statement version resolving from its digest; the shared policy rule's refusals. Attestation vectors use a test platform root supplied as the trust input, as the app's attestation tests do today. Prior art: the existing vector suite and its regeneration switch, the credential limit vectors, and the test that asserts the library has no I/O dependencies, which must keep passing with the attestation feature on.
2. **Credential issuance over an in-memory database** (existing). Issue against a finished integrity session and assert on the signed credential and on refusals: evidence present and credential-bound when the session attests; session-bound when binding evidence is unavailable; two status entries; the related resource; sponsor-device issuance refused for a session that is not the subject's attempt for the role. The same seam carries aggregation: a self-issued credential of any declared type scores as a self-claim; verified credential-bound evidence raises it to the attested weight; session status changes the score only where the rule says. Prior art: the issuance gate tests, the role assessment tests, the aggregation and issue-then-verify end-to-end tests.
3. **Hosted assessment exchange over HTTP** (existing). Post a candidate's share and assert on the response, the run record and the audit trail: verified and self-reported runs; invalid evidence refused; the review-case rule; policy satisfied yields an organisation-issued credential that the verifier accepts with organisation backing; policy unmet records the first unmet requirement; administrator revoke and suspend change what the hosted lists serve. Prior art: the assessment exchange, status list and organisation identity tests.

Evidence produced by real Apple and Google hardware cannot be generated in CI. Running an assessment on a physical iPhone and a Play-distributed Android build and verifying the resulting credential with the released library is an owner task, tracked with the device verification already open under the trust profiles spec.

## Out of Scope

- **Open Badges 3.0.** Its certification accepts `eddsa-rdfc-2022` and `ecdsa-sd-2023` Data Integrity proofs and not `eddsa-jcs-2022`, and the RDF suite needs full JSON-LD processing and published contexts. It fits as an export format or a second proof, in its own spec.
- **A published JSON-LD context** for Alexandria's terms, and moving the existing evidence references out of the credential subject into `evidence`. Both belong with the Open Badges work. Only the new attestation evidence uses `evidence` here.
- **Recovery of a compromised personal key.** Deleting rotation leaves a learner with a lost key to start a new identity. Identity recovery needs its own design.
- **Preserving organisation backing after a key is removed from a domain**, for example by proving a credential was anchored while the key was listed.
- **Requiring an anchor** for any credential. Anchoring stays optional per [ADR 0005](adr/0005-sponsored-batched-anchoring.md).
- **Signal certification.** Measured error rates remain the work of the trust profiles spec; until a certification is in force every signal stays advisory and the statements say so.
- **Delivering a candidate's share to an organisation owner's device.** Organisation-issued credentials for remote candidates come from the hosted service.
- **The website's verifier page.** It follows once the result fields here are frozen.
- **Interview integrity reports.** They stay a signed summary checked for shape and ownership; carrying attestation evidence in them is a follow-up.
- **Standard presentation protocols**, domain-linked identifiers beyond the existing domain record, and taxonomy alignment with external skill frameworks.

## Further Notes

- **What attestation evidence proves and does not.** Verified credential-bound evidence says a genuine build of the app on a genuine device produced this credential's content. It does not say who was at the device or that no second device was in the room, and a device that defeats the platform check falls to `local`. The coverage statements keep saying this.
- **Why status is ignored for unattested self-issued credentials.** A forger writes `completed`. Penalising `flagged` there would lower only the scores of people running the unmodified app.
- **Android evidence** can be opened only with the network's Play Integrity response keys. A verifier without them reports the evidence as unverifiable, not invalid. The keys are not yet in the preprod network profile, so Android credentials stay self-reported until they are.
- **Desktop without a companion phone** has no platform attestation and stays self-reported. That is a platform ceiling, already stated in the statements.
- **Pre-launch data is disposable.** The credential envelope, the status entries and the statement versions change without a data migration. Credentials already issued in test profiles are not converted.
- **ADRs.** Three decisions here deserve a record when they land: integrity evidence must be checkable by the relying party or labelled self-reported; a `did:key` never rotates; the hosted service issues organisation-signed credentials under the organisation's domain-listed key. The second amends the identity row of [ADR 0001](adr/0001-vc-first-credentials.md).
- **Documentation touched by this work**, to be proposed with each increment: the Sentinel integrity assurance and coverage sections, the evidence and credential sections of skills and reputation, the protocol specification's key rotation and status sections, the generated schema document, the glossary, and the hosted service's organisation and assessment exchange pages.
- **Relation to earlier work.** Builds on the trust profiles spec ([sentinel-trust-profiles-spec.md](sentinel-trust-profiles-spec.md)), which defined the assertion fields and left attestation verified only at issuance.

## Children

Sub-tickets live in `credential-trust/`; each names its blockers. The Sequence above maps onto them; tickets 06, 09 and 14 split work the Sequence listed together.

| Ticket | Repo | Blocked by | Triage |
|---|---|---|---|
| [01 Provenance scoring](credential-trust/01-provenance-scoring.md) | alexandria | none | done |
| [02 Coverage statement registry](credential-trust/02-coverage-statement-registry.md) | alexandria | none | done |
| [03 Suspension status list](credential-trust/03-suspension-status-list.md) | alexandria | none | done |
| [04 Delete key rotation](credential-trust/04-delete-key-rotation.md) | alexandria | none | done |
| [05 Attestation verification](credential-trust/05-attestation-verification.md) | alexandria | none | done |
| [06 Attestation evidence at issuance](credential-trust/06-attestation-evidence-at-issuance.md) | alexandria | 05 | done |
| [07 Attested scoring](credential-trust/07-attested-scoring.md) | alexandria | 01, 05 | done |
| [08 Shared issuance policy](credential-trust/08-shared-issuance-policy.md) | alexandria | 05 | done |
| [09 Organisation backing](credential-trust/09-organisation-backing.md) | alexandria | 04 | done |
| [10 Sponsor issuance binding](credential-trust/10-sponsor-issuance-binding.md) | alexandria | 08 | done |
| [11 Cloud verified runs](credential-trust/11-cloud-verified-runs.md) | alexandria-cloud | 05 | done |
| [12 Cloud coverage statements](credential-trust/12-cloud-coverage-statements.md) | alexandria-cloud | 02 | done |
| [13 Cloud organisation-issued credentials](credential-trust/13-cloud-org-issued-credentials.md) | alexandria-cloud | 03, 08, 09, 11 | done |
| [14 Collect organisation-issued credentials](credential-trust/14-collect-org-credentials.md) | alexandria | 13 | done |
| [15 Trust surfaces](credential-trust/15-trust-surfaces.md) | alexandria | 03, 06, 09 | done |
| [16 Verifier release](credential-trust/16-verifier-release.md) | alexandria, alexandria-cloud | 02, 03, 04, 05, 08, 09 | done |

## Decisions

Owner answers, 2026-11-16:

- **Test seams confirmed**: verifier interoperability vectors, credential issuance over an in-memory database, hosted assessment exchange over HTTP. No new seam.
- **Organisation signer**: the hosted service signs with the organisation's hosted key. Sponsor-device issuance gains only the session-to-subject binding.
- **Personal key rotation**: deleted.

## Notes

2026-11-16: all sixteen tickets built and merged on `feat/credential-trust` in `alexandria` and `alexandria-cloud`, followed by a code review of each branch and one round of fixes (recorded in the affected tickets' Notes). Status stays `active` until the owner settles the open items:

- Ticket 08 made the camera floor strict: a degraded camera signal never carries a floor, because a verifier cannot see coverage. Keeping the 90% tolerance needs a verifier-visible coverage field in the integrity assertion.
- Story 28 (telling a candidate the role's policy before they start) is not built: the exchange request carries no policy.
- Markdown doc edits proposed by each ticket await approval, including regenerating the generated schema document.
- `alexandria-verify` 0.7.0 is not published to crates.io.

