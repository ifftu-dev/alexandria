# 18 The organisation's policy in the credential request

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 08

Spec sections: *Organisation-issued credentials from the hosted service*, story 28.

## What to build

A candidate sees what the organisation's issuance policy requires before starting an attempt, and which of their credentials meet it.

- `CredentialRequest` (verifier library `exchange`) gains an optional `issuancePolicy`, the policy the organisation will judge the answer against. It is covered by the organisation's request proof and by the holder's presentation signature, so neither side can change it afterwards. A request without one serialises exactly as before. A device-local `probe_connection_id` is never part of a request.
- The app's request view lists the requirements in plain words before the candidate starts (clean session, minimum score, flag limits, required assurance, trust profile floor, verification rung, probe policy), and says when this device cannot run the floor (no camera, no platform attestation).
- An attempt started from a request locks a trust profile that meets the floor and runs the request's probe policy, as role-targeted attempts already do for a sponsor's role.
- When the candidate picks a credential to share, the picker runs the shared rule (`policy::evaluate`) on each candidate credential and marks the ones that would be refused, with the reason, before anything is sent.

## Acceptance criteria

- [ ] Verifier tests: a request with a policy round-trips, its presentation verifies, and `verify_share` refuses a presented request whose policy differs from the expected one; a request without a policy serialises byte-identically to 0.6.0.
- [ ] App tests: the picker marks a credential below the floor as refused with its code; an attempt started from a request with a camera floor locks a camera profile.
- [ ] fmt, clippy, vue-tsc, vitest and the crate's vectors pass.
