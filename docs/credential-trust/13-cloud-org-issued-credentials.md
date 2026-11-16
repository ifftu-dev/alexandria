# 13 Cloud organisation-issued credentials

Triage: ready-for-agent
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

- [ ] HTTP tests: policy satisfied → credential issued; the vendored verifier accepts it with organisation backing `verified` for the org's descriptor; policy unmet → refusal recorded, nothing issued; self-issue refused; unverified domain refused.
- [ ] Revoke, suspend and reinstate change what the hosted lists serve; each writes an audit row.
- [ ] The candidate's signed fetch returns only their own credentials.
- [ ] Migrations, fmt, clippy, console checks pass.

## Pointers

- Org identity and hosted keys: `src/` org identity modules, `docs/organisations.md`.
- Status list hosting: `src/status_lists.rs`, `src/http/status_lists.rs`.
- Signed `/for/{did}` endpoints: `src/http/api.rs`, `docs/organisations.md` *What gets signed*.
- Commits need `--signoff` (DCO).
