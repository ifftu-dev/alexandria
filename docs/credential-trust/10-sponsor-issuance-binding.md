# 10 Sponsor issuance binding

Triage: ready-for-agent
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

- [ ] Tests for each refusal and for the accepted case, over an in-memory database.
- [ ] Existing role assessment tests pass.
- [ ] `vue-tsc -b --noEmit` passes if the page changes.

## Pointers

- `src-tauri/src/commands/role_assessment.rs` (`issue_role_credential_impl`, `list_role_attempts_impl`).
- `src/pages/dashboard/Sponsor.vue`, `src/composables/useSponsor.ts`.
