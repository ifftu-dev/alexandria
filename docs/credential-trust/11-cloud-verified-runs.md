# 11 Cloud verified runs

Triage: done
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

## Notes

2026-11-16: built and merged into the Cloud integration branch (`alexandria-cloud` `feat/credential-trust`, merge `6f29d9b`).

- Vendored `alexandria-verify` synced from app commit `b8a4b80a` with features `attestation` (and `test-support` for tests). The sync also brought `alexandria-decisions` up to date, because the vendor check compares every vendored package.
- Trust inputs come from configuration: `APP_ATTEST_APP_ID`, `APP_ATTEST_ALLOW_DEVELOPMENT`, and `PLAY_INTEGRITY_PACKAGE_NAME` / `_DECRYPTION_KEY` / `_VERIFICATION_KEY` (all three or none; a partial or undecodable Play setup stops the server at start). Unset means `unverifiable`. Deviation: the iOS app id is configuration too, since iOS evidence cannot be checked without it.
- Invalid evidence refuses the share with a 400 naming the reason and is audited as `run.share_refused`; the request stays open.
- Migration `0054` stores the verified assurance level and attestation outcome on the run. A review case opens when the reported status is flagged, or when the credential states `device_attested` and the evidence does not verify credential-bound.
- The console shows "Verified" only for `device_attested`; session-bound evidence reads "Self-reported" with a note. MCP run results gain the two fields and still carry no scores.
- `verify_share` in the library takes no trust inputs, so Cloud sets the attestation fields after calling it.
