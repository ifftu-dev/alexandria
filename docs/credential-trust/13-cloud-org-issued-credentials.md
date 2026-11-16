# 13 Cloud organisation-issued credentials

Triage: done
Type: task
Repos: alexandria-cloud
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 03, 08, 09, 11

Spec sections: *Organisation-issued credentials from the hosted service* (remaining bullets), *Surfaces* (console refusal reason), stories 26–30, 33–37.

## What to build

The hosted service signs a candidate's accepted result as the organisation.

- Sync the vendored `alexandria-verify` from the app integration branch (tickets 03, 08, 09).
- A role (assessment request template) carries an issuance policy, edited in the console, using the shared policy type from ticket 08.
- When a share is accepted and the policy is satisfied, issue a role credential signed with the organisation's hosted key: subject = candidate DID; `issuer` object with the key's `did:key`, the organisation's name and verified domain; the candidate's integrity assertion unchanged; the attestation evidence copied; an evidence entry naming the candidate's credential by id and integrity hash. Refuse when the subject is one of the organisation's own keys. Issue only when the organisation's domain identity is verified.
- When the policy is not satisfied, record the first unmet requirement on the run and issue nothing. The console shows it.
- The issued credential carries revocation and suspension status entries in lists the service maintains and hosts for the organisation's key. Administrators can revoke, suspend (optionally until a time) and reinstate from the console; each action re-signs and republishes the list and is audited.
- The candidate fetches issued credentials with a signed GET by subject DID, like the existing `/for/{did}` endpoints: `GET /api/credentials/for/{did}` returning `[{credential}]`.
- The service records issued credentials by id for the organisation; it never stores behavioural data.

## Acceptance criteria

- [x] HTTP tests: policy satisfied → credential issued; the vendored verifier accepts it with organisation backing `verified` for the org's descriptor; policy unmet → refusal recorded, nothing issued; self-issue refused; unverified domain refused.
- [x] Revoke, suspend and reinstate change what the hosted lists serve; each writes an audit row.
- [x] The candidate's signed fetch returns only their own credentials.
- [x] Migrations, fmt, clippy, console checks pass.

## Pointers

- Org identity and hosted keys: `src/` org identity modules, `docs/organisations.md`.
- Status list hosting: `src/status_lists.rs`, `src/http/status_lists.rs`.
- Signed `/for/{did}` endpoints: `src/http/api.rs`, `docs/organisations.md` *What gets signed*.
- Commits need `--signoff` (DCO).

## Notes

2026-11-16: built on `alexandria-cloud` `ct/13-cloud-org-issued-credentials` (merged into the Cloud integration branch by the coordinator's merger).

- Roles carry an issuance policy (`PUT /api/roles/{id}/issuance-policy`, administrators); an assessment request may name a `role_id`. The run snapshots the policy at invitation, so later role edits do not change what an invited candidate is judged against.
- On an accepted share the service issues a role credential signed with the organisation's hosted key: issuer object with the key's `did:key` and the organisation's verified name and domain, the candidate's integrity assertion unchanged, and the coverage `relatedResource` copied only when it matches the assertion's digest.
- Deviation (coordinator decision): the candidate's attestation evidence is not copied, because its binding digest covers the candidate credential and would verify as `binding_mismatch`. The role credential instead carries an `AssessedCredentialEvidence` entry naming the candidate credential by id and integrity hash, with the attestation outcome and verified assurance level the service established; the organisation's signature vouches for them. That entry type is defined in Cloud only for now.
- Refusal order, first failure recorded on the run and shown in the console: `share_not_accepted` (pending shares record but do not issue), `subject_is_organisation_key`, `no_integrity_assertion`, the shared rule's codes, `organisation_not_verified`.
- Status: revocation list 1 and suspension list 2 per organisation key; revoke, suspend (optionally until a time) and reinstate are admin-only, re-sign and republish the lists and are audited; a loop lifts timed suspensions each minute. After a key rotation, credentials signed with the old key can no longer change status (409).
- `GET /api/credentials/for/{did}` returns `[{credential}]` with the same signed-GET rule as the other `/for/{did}` endpoints. Migration `0055`.
- Not done: story 28 (telling the candidate the policy before they start). `CredentialRequest` carries no policy; adding it changes the verifier library's exchange contract.

