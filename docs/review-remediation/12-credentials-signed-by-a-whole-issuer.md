# Credentials signed by a whole issuer

Triage: ready-for-agent
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
