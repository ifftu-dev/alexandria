# 0009. For a hosted attempt the issuer holds the answers and computes the outcome

Status: implemented
Date: 2026-11-20

## Context

Under [0008](0008-hosted-organisation-issued-credentials.md) the hosted service signs a role credential on a candidate's shared, self-issued assessment credential. Everything that credential says about the attempt comes from the candidate's device: the items and their answer key were in the device's local data (bundled banks are in the open-source repository), the device computed the score, and a modified build can report any score and a clean session. A paired phone attests whatever digest the desktop hands it. The organisation's signature could therefore be earned without knowing the material.

The assessment hardening spec adds answer custody (`issuer` or `device`), a required rung and the `companion_attested` level to the shared verifier, a hosted item bank per role, and hosted attempts in which the service draws, sends, times and grades the items one at a time.

## Decision

For a hosted attempt the issuer holds the answers and computes the outcome; the candidate's device contributes integrity evidence only.

| Aspect | Choice |
|---|---|
| Trigger | The candidate's device closes a hosted attempt. The service judges the run's issuance policy, snapshotted at invitation, with the shared rule for hosted attempts. |
| What the issuer establishes | The score (its own grading), the probe step (unseen items it drew, timed and graded), the verification rung (`probed` and above only on its own probe step; `live` only once the interview flow marks a live segment done for the candidate and role), and the verified assurance level from its own check of the platform evidence the device sent over the close digest. Evidence from a paired phone verifies as `companion_attested`, never `device_attested`. |
| What the device contributes | Its integrity assertion, read for session status, flags, integrity score and trust profile, and its platform evidence. A score or timing the device reports decides nothing. |
| Credential | A `RoleCredential` signed with the organisation's hosted key as 0008 describes, with an assertion the issuer writes: the device's session facts with answer custody `issuer`, the issuer's rung, probe step and verified level, and the coverage statement for that profile and rung. Its one evidence entry is a `HostedAttemptEvidence` naming the attempt, stating that the issuer held the answers and graded, the probe step, and the attestation outcome and level the issuer found. There is no candidate credential to name. |
| Refusal | The first unmet requirement is recorded on the attempt and its run, nothing is issued, and the candidate's app reads the refusal code through a signed fetch. |
| `live` | A role requiring `live` whose attempt meets everything else holds issuance until the interview flow marks a live segment done for that candidate and role, then issues. Nothing is added to interviews. |
| Defaults | A new role requires `issuer` custody and the `probed` rung. An administrator may accept `device` custody; the console shows that choice on the role and on every run. |
| Share path | Issuance on a shared self-issued credential stays for roles that accept `device` custody. A role requiring `issuer` refuses it with the custody code, judged before every other requirement. |

## Consequences

- A credential from a hosted attempt says the answers never left the issuer and the score is the issuer's. A self-issued credential stating `issuer` custody is invalid: nobody holds answers away from themselves.
- A hosted attempt on a desktop does not prove what software showed the questions; a modified build can still report a clean session. A role that needs more requires `device_attested`, which only the attested app on the attempt's own device meets.
- A second device or person beside the screen is not ruled out below `live`. Hosted timings and unseen-item probes raise the cost of relaying; the credential's coverage statement says so.
- Hosted attempts need a connection, and the organisation's items and answers live in the hosted service. Bundled banks, banks held in a profile and private courses keep `device` custody and stay self-claims or sponsor-device issuance.
