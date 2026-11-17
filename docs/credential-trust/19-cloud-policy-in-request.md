# 19 Cloud sends the role's policy with the request

Triage: done
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

- [x] HTTP tests: a request for a role with a policy serves that policy in `/api/assessment-requests/for/{did}`, and its org proof verifies; a request without a role serves none; editing the role afterwards changes neither the served request nor what issuance judges.
- [x] `scripts/check-vendor.py --source <app>/crates` passes; fmt, clippy, console checks pass.

## Notes

2026-11-17: built.

- The vendored verifier is synced from the app with tickets 17 and 18; `check-vendor.py --source <app>/crates` passes.
- On invitation the role's stored policy goes through `IssuancePolicy::for_request()` (which drops `probe_connection_id`) before the request is checked and signed. That one value is both the request's `issuance_policy` and the run's snapshot, so what the candidate sees and what issuance judges cannot drift apart. A request without a role carries no policy and no `issuance_policy` key.
- A policy with floats survives Postgres JSONB unchanged, so a share built from the served request still matches the stored request in `verify_share`.
- Deviation: the invitation now parses the role's stored policy, so a policy that does not parse fails the invitation (500) instead of failing later at share time as `issuance_unavailable`. Policies set through the role endpoint are validated, so only rows written another way are affected.
- The console's run view does not show the policy.
