# 16 Verifier release

Triage: done
Type: task
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 02, 03, 04, 05, 08, 09

Spec sections: *Verifier library housekeeping*, stories 15–16, 50.

## What to build

- Correct the doc comments in `alexandria-verify`: the integrity assertion's statuses include `review`; `device_attested` is achievable and is reported as a verified assurance level only from evidence; the proof is a Data Integrity proof, not a JWS; no rotation language remains.
- Bump `alexandria-verify` to the next minor version with a breaking-change note in its changelog (or README section, whichever the crate uses) listing: `credentialStatus` as a list with a suspension entry, store trait changes (suspension, key rotation removed), `evidence`, attestation outcome and verified assurance level, issuer object form, organisation backing, `relatedResource`, the coverage registry, the shared issuance policy.
- Regenerate the interoperability vectors and confirm the diff is only what the tickets changed.
- Sync the Cloud vendored copy to the released bytes and update its checksums.
- Do not publish to crates.io; publishing is an owner action.

## Acceptance criteria

- [x] `cargo doc` for the crate builds without warnings; grep finds no "JWS" or rotation language in its public docs.
- [x] Vectors regenerate byte-identically on a second run.
- [ ] Cloud `scripts/check-vendor.py --source <app>/crates` passes.

## Pointers

- `crates/alexandria-verify/` (`Cargo.toml`, `src/vc/mod.rs`, `src/lib.rs`, `tests/vectors.rs`).
- Cloud vendor: `alexandria-cloud/vendor/`, `alexandria-cloud/scripts/check-vendor.py`.

## Notes

2026-11-16: built (app part).

- `alexandria-verify` is 0.7.0. The crate had no changelog, so `crates/alexandria-verify/CHANGELOG.md` holds the breaking-change note; the crate README links it. Every in-repo dependent uses a path dependency without a version requirement, so only `Cargo.lock` changed.
- Doc comments corrected: the integrity assertion lists `review` and says an unrecognised status is not clean; `require_clean` refuses `review` too; the stated `assuranceLevel` is the issuer's word, and only `verifiedAssuranceLevel` from credential-bound evidence that verifies reads `device_attested`; the credential proof is a Data Integrity proof. Remaining "JWS" mentions describe formats that really are JWS (organisation request proofs, talent index records, Play Integrity tokens).
- Vectors regenerate byte-identically on two runs (main, limits, endorsements, issuance policy, attestation). Since 0.6.0 the existing credentials' bytes are unchanged; vectors 01–12 changed only in `store` (no `suspended`) and `expect` (`attestation`, `verifiedAssuranceLevel`), and 08 was replaced.
- The independent verifier resolves its vectors from its own location, so it runs from any directory; CI now runs it from the repository root.
- The Cloud vendor sync and its `check-vendor.py` criterion are left to the coordinator after the last merge. Nothing is published to crates.io.
