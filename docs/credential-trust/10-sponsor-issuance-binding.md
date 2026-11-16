# 10 Sponsor issuance binding

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 08

Spec sections: *Role credential issuance on a sponsor's own device*, stories 38–39.

## What to build

- Role credential issuance on a sponsor's own device refuses unless the integrity session belongs to an attempt made for that role assessment and the attempt's subject is the credential's subject. Each mismatch has its own refusal message: unknown session, session not an attempt for this role, attempt belongs to another subject, attempt not graded or not passed.
- The role's policy is judged through the shared rule from ticket 08.
- The Sponsor page's issue form picks from the role's attempts (it already lists them) so the binding is the default path; a hand-typed session id still goes through the same check.

## Acceptance criteria

- [x] Tests for each refusal and for the accepted case, over an in-memory database.
- [x] Existing role assessment tests pass.
- [x] `vue-tsc -b --noEmit` passes if the page changes.

## Pointers

- `src-tauri/src/commands/role_assessment.rs` (`issue_role_credential_impl`, `list_role_attempts_impl`).
- `src/pages/dashboard/Sponsor.vue`, `src/composables/useSponsor.ts`.

## Notes

2026-11-16: built.

- `issue_role_credential_impl` checks the binding before anything is written (the org DID adoption included), so the desktop command and the CLI both refuse. Order: the session exists on this device; an attempt for this role names it; one of those attempts is the subject's; the subject's attempt is graded (grade time and pass bit set) and passed. Messages: `integrity session <id> is unknown on this device`, `... is not an attempt for this role assessment`, `... belongs to another subject's attempt`, `the attempt for integrity session <id> has not been graded`, `... did not pass`.
- Decision: "not graded" and "not passed" are two messages, so a sponsor can tell a pending grade from a fail.
- The policy is still judged by `credentials::issue`, which runs the shared rule from ticket 08; nothing changed there.
- Sponsor page: Use appears only on passed attempts with a session, the newest is picked when a role is chosen, and a role change clears the previous pick. No new strings.
- Existing role issuance tests now seed a passed attempt for their subject.

2026-11-16 (review fix): a role's assurance requirement is judged against the attempt's own `AssessmentCredential` (the attempt's `credential_id`), since the sponsor's device cannot bind fresh evidence for a candidate's session. That credential must be the subject's for the session and verify with the profile's trust inputs, else issuance is refused; the role credential then carries an `AssessedCredentialEvidence` entry (`alexandria_verify::assessed`, the shape Cloud emits) naming it by id and integrity hash with the attestation outcome and verified level found, and the shared rule judges the policy from that entry (`policy::evaluate_assessed`). An attempt without a credential is judged from the role credential's own evidence, as before. `issue_role_credential_with` takes the evidence source; `credentials::issue_with_evidence` issues with extra evidence entries.
