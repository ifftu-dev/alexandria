# 0006. Integrity evidence is checkable by the relying party or labelled self-reported

Status: implemented
Date: 2026-11-16

## Context

A credential's integrity assertion states a session status, a trust profile, a verification rung and an assurance level. The learner's device signs it with the learner's own key. Before this decision a reader had no way to tell whether any of it was true:

- `device_attested` was a word. The platform attestation was verified only on the device that issued, and the credential carried the resulting level, not anything a reader could check.
- Scoring weighted a self-issued assessment credential above an organisation-signed role credential, because weight followed the type the issuer wrote down.
- A hosted service deciding whether to open a review case read the session status straight from the candidate's credential.

Anyone holding a key can write a credential that says the session ran cleanly on an attested device, and have it verify.

## Decision

A credential claims only what the relying party can check, and anything it cannot check is treated as the holder's word.

| Aspect | Choice |
|---|---|
| Evidence | A session that ran on an attested device puts the platform's own material (App Attest, Play Integrity) in the credential's standard `evidence` property as a `DeviceAttestationEvidence` entry. |
| Binding | *Credential-bound* evidence is platform-signed over a digest of the credential's subject, claim, integrity assertion and commitment root, so it cannot be moved to another credential. When that cannot be obtained, the session's own attestation travels as *session-bound* and is never upgraded later. |
| Verification | Attestation checking lives in `alexandria-verify` (optional `attestation` feature, still no I/O). Trust inputs come from the caller. Outcomes: `not_supplied`, `verified_credential_bound`, `verified_session_bound`, `unverifiable`, `invalid` with a reason code. Invalid evidence rejects; the others never do. |
| Assurance | The stated `assuranceLevel` stays the issuer's word. The verified assurance level is `device_attested` only for verified credential-bound evidence. A policy that requires `device_attested` is met only by the verified level. |
| Scoring | Weight follows verified provenance. A self-issued credential takes the self-claim weight whatever type it declares, or a slightly higher attested weight with verified credential-bound evidence, always below every issuer-signed evidence weight. Session status affects scoring only where a reader can rely on it: issuer-signed or credential-bound attested credentials. |
| Policy | One issuance policy rule over verifier-visible facts (`alexandria_verify::policy`), shared by the app, the hosted service and the candidate's picker. The camera's 90% coverage rule is judged from a `cameraCoverage` field the assertion states. |
| Labels | Every surface states one of: issued by a named organisation, device-attested and verified, or self-reported. Coverage statements say that the issuing device verified the attestation. |

## Consequences

- An attested phone's credential is worth more than a bare claim to any verifier, not just to the app that issued it.
- Desktop sessions without a companion phone, and Android until the network profile carries Play Integrity response keys, stay self-reported. That is a platform ceiling, stated in the coverage statements.
- Penalising a `flagged` status on an unattested self-issued credential would only lower the scores of honest users, since a forger writes `completed`; that status is ignored.
- Verified evidence says a genuine build on a genuine device produced the credential's content. It does not say who sat at the device or that no second device was in the room.
- Real Apple and Google evidence cannot be produced in CI; vectors use synthetic roots, and real-device verification is an owner task.
- Interview integrity reports do not carry attestation evidence yet.
