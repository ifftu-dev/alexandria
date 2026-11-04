# Credentials signed by a whole issuer

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Credential issuance paths take the issuer as one value from unlock to the signing call. Remove every rebuild of an issuer from a bare key in issuance paths (assessment and adaptive grading among them): a rebuild re-derives the DID from the key, so after a rotation the credential carries the wrong DID.

Splitting an issuer into key and DID stays only for non-credential signatures, such as directory proofs and organisation responses.

Plugin review listing reads the learner's DID from the profile identity without unlocking the vault.

Covers spec stories 65–67. This completes deepening ticket 08's "travel as one value" rule.

## Acceptance

- A credentials test issues an assessment credential with a rotated issuer, and its issuer and subject are the rotated DID.
- No issuance path accepts a bare signing key.
- Plugin reviews list while the vault is locked.
- The credential lifecycle tests still pass.

## Completion evidence — 2026-11-04

What changed:

- **Assessment and adaptive grading** take the `Issuer` from unlock to `credentials::issue`. The rebuild from a bare key and the "signing key does not match the learner" check are gone: an `Issuer` can only pair a DID with its own key (`from_key`) or its registry-held current key (`rotated`).
- **Other issuance paths** that took a bare key now take the issuer: completion self-claims, completion auto-issuance (`credentials::auto::tick`, `issue_for_observation`) and the birthdate self-assertion. The auto-issuance check against the mint's subject key stays; the DID-from-key check is dropped for the same reason.
- **Splitting.** `into_parts` remains only where the key signs a non-credential: Cloud and directory request proofs, presentations, talent records, grant statements, interview and offer responses, integrity reports. Commands that split only to read the DID now call `did()`.
- **Plugin reviews.** Listing, submitting and reviewing read the profile's DID from its identity (`identity::local_did`) without unlocking. A profile with no identity is refused.

Red → green:

- `assessment::tests::a_rotated_issuer_earns_the_credential_for_its_own_did` and `adaptive::tests::a_rotated_issuer_earns_the_credential_for_its_own_did`: failed with "assessment signing key does not match the learner"; now the VC's issuer and subject are the rotated DID.
- `credentials::auto::tests::a_rotated_issuer_self_issues_under_its_own_did`: failed with "completion issuance does not match the stored observation and learner"; now issued under the rotated DID.
- `plugins::review_tests::a_learner_lists_their_own_reviews_from_the_profile_identity_alone`: failed to build (no keyless listing); now lists only the learner's own reviews with no keystore.

Grep: no `Issuer::from_key(` outside test modules; no credential issuance function takes a `SigningKey`.

Checks: `cargo test -p alexandria-node --lib` for credentials (lifecycle and issuer included), adaptive, assessment, plugins, completion, profile, role assessment, snapshot, guardian and AI access (187 passed); `--test e2e_vc --test guardian_e2e` (43 passed); `cargo fmt --check`; `cargo clippy -p alexandria-node --all-targets -- -D warnings`; `npm run check:tauri-commands` and its tests; `scripts/check-docs.sh`.

Deviations:

- Scope grew past assessment and adaptive to completion, auto-issuance and the birthdate self-assertion, which also took bare keys.
- The two "mismatched signing key" test cases became one: another learner's issuer is refused, and a key that is not the learner's cannot become the learner's issuer.
- Submitting and reviewing plugin work read the identity too, not only listing. Nothing there is signed.
- The app has no rotation command yet. `Issuer::from_keystore` still derives the DID from the wallet key, so production issuers are never rotated today.
