# 05 Answer custody and required rung in the app

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 02

Spec sections: *Answer custody*, *Required rung*, stories 22, 33, 38.

## What to build

- Every integrity assertion the app builds states answer custody `device`. The app never writes `issuer`.
- Role and request issuance policies in the app carry the required rung and required answer custody, judged through the shared rule. A role created on a sponsor's own device defaults to custody `device` (nothing else is possible there) and its form states that candidates' devices hold the answers.
- A role that requires `live` waits for the existing live segment: role credential issuance on a sponsor's device succeeds once the panel has marked a live segment done for that candidate and role, and refuses with the rung code before that.
- Credential views show answer custody and, below `live`, that a second device or person was not ruled out (from the coverage statement).

## Acceptance criteria

- [ ] Test: a built assertion carries custody `device` and verifies under the library.
- [ ] Test: sponsor-device issuance refuses below the required rung and succeeds once the rung is met, including `live` through a marked live segment.
- [ ] fmt, clippy, `vue-tsc`, `npm run check:tauri-commands` pass.

## Pointers

- `src-tauri/src/credentials/issue.rs` (`build_integrity_assertion`, `IssuancePolicy`), `src-tauri/src/commands/role_assessment.rs`, `src-tauri/src/commands/hiring.rs` (live rung), credential trust surfaces from [credential-trust/15](../credential-trust/15-trust-surfaces.md).
