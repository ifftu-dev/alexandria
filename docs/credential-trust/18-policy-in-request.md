# 18 The organisation's policy in the credential request

Triage: done
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

- [x] Verifier tests: a request with a policy round-trips, its presentation verifies, and `verify_share` refuses a presented request whose policy differs from the expected one; a request without a policy serialises byte-identically to 0.6.0.
- [x] App tests: the picker marks a credential below the floor as refused with its code; an attempt started from a request with a camera floor locks a camera profile.
- [x] fmt, clippy, vue-tsc, vitest and the crate's vectors pass.

## Notes

2026-11-17: built.

- Verifier library: `CredentialRequest` gains `issuance_policy: Option<IssuancePolicy>`, last in the struct, snake_case like the other request fields (not `issuancePolicy`), omitted from JSON when absent. A test pins the exact 0.6.0 bytes of a request without one. `CredentialRequest` is now `PartialEq` only (the policy holds floats); the changelog lists that under breaking changes. There are no exchange vectors, so none were added.
- Decision: a request carrying `probe_connection_id` is refused, not cleared, since clearing would change the payload the organisation signed. `CredentialRequest::check` refuses it; `present_credential`, `verify_share`, the app's `validate_request` and the request store call it. `IssuancePolicy::for_request` builds a request's policy without the field.
- Requests were not stored on this device before. Migration 43 adds `exchange_requests` (the request as verified, by id) and `assessment_attempts.request_policy_json`. `exchange_prepare_assessment` fetches the request again, checks the organisation's proof and the request, and stores it; `exchange_start_assessment` does the same again before starting.
- Attempts reuse the role-targeted path. `ProfileRequest` gains `exchange_request_id`: profile selection takes the floor from the stored request and refuses an unknown one, or one named together with a role. Attempt start judges the request's floor before any item and stores the policy on the attempt; `effective_probe_policy` reads it when the attempt has no role. A request carries no model connection, so an `llm` probe policy is skipped as unavailable and no consent is asked.
- Picker: `exchange_credentials` returns `meets_policy`, `refusal_code` and `refusal` per credential, judged by the shared rule with the attestation outcome checked against the profile's trust inputs (a role credential through its assessed-credential entry, as at issuance). A credential with no integrity result is refused as `no_integrity_assertion`, the code the hosted service uses.
- Decision: a refused credential stays selectable. It is marked with the reason, and a note says the organisation will not issue its own credential for it. The policy decides issuance, not whether the candidate may share a result.
- Request view: the card lists the requirements in the shared rule's order. When the floor needs the camera or attestation, it asks `integrity_device_support` (new, profile-free) and the browser's device list, and says when this device has no camera or cannot attest. On desktop it points to pairing a phone instead. The assessment page stops at a camera gate (and offers phone pairing) before monitoring starts when the request's floor needs either.
- Strings are English-only, listed as pending in the i18n parity check, as tickets 06 and 15 did.
