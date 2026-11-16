# 11 Cloud verified runs

Triage: ready-for-agent
Type: task
Repos: alexandria-cloud
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 05

Spec sections: *Organisation-issued credentials from the hosted service* (first three bullets), *Surfaces* (console), stories 31–32.

## What to build

An organisation's run record shows what was verified, not only what the candidate reported.

- Sync the vendored `alexandria-verify` from the app's integration branch (including ticket 05) and update `vendor/checksums.json`; `scripts/check-vendor.py --source <app>/crates` passes.
- On an assessment share, verify the attestation evidence with the deployment's trust inputs (Apple root built in; Play Integrity keys and app id from configuration, absent meaning `unverifiable`).
- Invalid evidence refuses the share with a clear error and is audited.
- The run stores the verified assurance level and the attestation outcome beside the reported status (migration).
- The review-case rule keeps reading the reported status, and also opens a case when the credential states `device_attested` and the evidence does not verify credential-bound.
- The console run views show each run as verified or self-reported. The MCP result types gain the same two fields; no scores reach MCP (unchanged).

## Acceptance criteria

- [ ] Assessment exchange tests cover: no evidence (self-reported), credential-bound (verified), invalid (refused, audited), stated `device_attested` with session-bound evidence (review case opened).
- [ ] Vendor check passes against the app integration branch.
- [ ] `cargo fmt --check`, `cargo clippy -- -D warnings`, console `vue-tsc` and `npm test` pass.

## Pointers

- Share handler: `src/http/assessment.rs` (`share`); runs: `src/runs.rs`; review cases: `src/review*.rs`.
- Vendor rules: `CONTRIBUTING.md` *Vendored crates*.
- Tests: `tests/assessment_exchange.rs`; run with `scripts/test.sh`.
- Commits need `--signoff` (DCO).
