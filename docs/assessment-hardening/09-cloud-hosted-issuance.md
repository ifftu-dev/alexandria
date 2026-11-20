# 09 Issuance from a hosted attempt

Triage: ready-for-agent
Type: task
Repos: alexandria-cloud, alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 02, 08

Spec sections: *Issuance from a hosted attempt*, *Required rung*, *Answer custody* (hosted defaults), stories 2, 16, 17, 20, 33–35, 39.

## What to build

- Sync the vendored `alexandria-verify` from the app integration branch (tickets 01 and 02) and update its checksums.
- Role issuance policies gain required rung (default `probed` for new roles) and required answer custody (default `issuer` for new roles), edited in the console; choosing `device` custody is shown on the role and every run.
- When a hosted attempt closes, judge the role's policy with the shared rule using the service's own score, probe outcome and rung, its own verification of the platform evidence (companion evidence yields `companion_attested`), and the device's assertion for session status and trust profile.
- Policy met: issue a role credential signed with the organisation's hosted key as ADR 0008 describes, with the hosted attempt evidence entry from ticket 02 instead of a candidate credential reference, answer custody `issuer` in the assertion, and status entries as today. Delivered through the existing signed fetch.
- Policy unmet: record the first unmet requirement on the run, issue nothing, and expose the refusal code to the candidate through a signed fetch.
- A role requiring `live` holds issuance until the existing interview flow marks a live segment done for that candidate and role, then issues.
- The existing share path keeps working for roles that accept `device` custody, and is refused with the custody code for roles that require `issuer`.
- Console run view: the service's per-item timings, probe outcomes, custody, verified level and the refusal reason.
- Write the ADR in the app repo (`alexandria/docs/adr/`): for a hosted attempt the issuer holds the answers and computes the outcome; the device contributes integrity evidence only.

## Acceptance criteria

- [ ] HTTP tests: policy met issues, and the vendored verifier accepts the credential with custody `issuer`, the hosted attempt entry and organisation backing; each refusal code recorded and fetchable by the candidate; `live` waits then issues; share path refused for `issuer`-custody roles.
- [ ] `scripts/check-vendor.py --source <app>/crates` passes.
- [ ] Migrations, fmt, clippy, console checks pass; app `check-docs.sh` passes with the ADR.

## Pointers

- `src/http/assessment.rs` (share and issuance), `src/issued_credentials.rs`, `src/http/issued_credentials.rs`, `src/interview_panel.rs`, `web/src/views/RunDetailView.vue`, `RolesView.vue`; ADR 0008 and prior art [credential-trust/13](../credential-trust/13-cloud-org-issued-credentials.md).
