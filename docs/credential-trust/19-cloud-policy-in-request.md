# 19 Cloud sends the role's policy with the request

Triage: ready-for-agent
Type: task
Repos: alexandria-cloud
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 17, 18

Spec sections: *Organisation-issued credentials from the hosted service*, story 28.

## What to build

- Sync the vendored `alexandria-verify` from the app integration branch (tickets 17 and 18).
- An assessment request naming a role carries the policy the run snapshots, without `probe_connection_id`, under the organisation's proof. Issuance judges the share against that same snapshot.
- A request with no role carries no policy.

## Acceptance criteria

- [ ] HTTP tests: a request for a role with a policy serves that policy in `/api/assessment-requests/for/{did}`, and its org proof verifies; a request without a role serves none; editing the role afterwards changes neither the served request nor what issuance judges.
- [ ] `scripts/check-vendor.py --source <app>/crates` passes; fmt, clippy, console checks pass.
