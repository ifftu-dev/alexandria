# 0008. The hosted service issues organisation-signed credentials under the organisation's domain-listed key

Status: implemented
Date: 2026-11-16

## Context

An organisation could put its signature on a candidate's result only from its own device: role credential issuance read attempts and integrity sessions from the issuer's local database, and nothing carried a remote candidate's session there. Candidates assessed for an organisation through the hosted service therefore held only their own self-issued credential, and the organisation's hiring record rested on the candidate's word.

The hosted service already holds each organisation's signing key and proves it against the organisation's domain (DNS TXT descriptor). [0003](0003-credential-authority.md) makes status issuer-bound.

## Decision

The hosted service issues organisation-signed credentials, and the organisation is their issuer.

| Aspect | Choice |
|---|---|
| Trigger | A candidate's accepted answer to an assessment request that names a role. The service verifies the share, including attestation evidence ([0006](0006-checkable-integrity-evidence.md)). |
| Policy | The role's issuance policy, snapshotted when the candidate is invited. Judged by the shared rule; the first unmet requirement is recorded and nothing is issued. A credential request may carry the policy, covered by both signatures, so the candidate sees it before starting. |
| Credential | A `RoleCredential` signed with the organisation's hosted key, whose `did:key` the domain lists. The issuer is an object with that DID, the organisation's name and its domain. It carries the candidate's integrity assertion unchanged and an `AssessedCredentialEvidence` entry naming the candidate's credential by id and integrity hash, with the attestation outcome and verified assurance level the service established. |
| Exclusions | Never issued to a subject whose key is one of the organisation's own. |
| Status | Revocation (list 1) and suspension (list 2) entries in lists the service hosts for the organisation's key. Administrators revoke, suspend and reinstate; each action is audited. |
| Delivery | The candidate's app collects the credential through the signed `GET /api/credentials/for/{did}`, verifies it and stores it like any received credential. |
| Sponsor devices | Issuance from an organisation's own device stays, with one addition: the session must belong to the named subject's graded, passed attempt for that role. |

## Consequences

- A remote candidate holds a credential that is more than their own word, and owns it.
- The candidate's attestation evidence is not copied: its binding covers the candidate's credential and would not verify on the role credential. The organisation's signature vouches for the outcome it records instead.
- Organisation backing depends on the domain's key list. Rotating a hosted key withdraws backing from credentials the old key signed, and their status can no longer be changed.
- Showing the candidate the policy up front needs the service to put it in the request. The app and the verifier contract support it; the hosted service does not send it yet.
- Delivering a candidate's share to an organisation owner's device is not built; remote candidates' organisation-issued credentials come from the hosted service.
