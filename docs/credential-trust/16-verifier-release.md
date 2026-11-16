# 16 Verifier release

Triage: ready-for-agent
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

- [ ] `cargo doc` for the crate builds without warnings; grep finds no "JWS" or rotation language in its public docs.
- [ ] Vectors regenerate byte-identically on a second run.
- [ ] Cloud `scripts/check-vendor.py --source <app>/crates` passes.

## Pointers

- `crates/alexandria-verify/` (`Cargo.toml`, `src/vc/mod.rs`, `src/lib.rs`, `tests/vectors.rs`).
- Cloud vendor: `alexandria-cloud/vendor/`, `alexandria-cloud/scripts/check-vendor.py`.
