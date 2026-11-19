# 05 Attestation verification

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: none

Spec sections: *Vocabulary*, *Verification of attestation evidence*, the evidence-format part of *Attestation evidence in the credential*, stories 1–5, 16–17.

## What to build

Any verifier can check a credential's device attestation itself.

- Move the App Attest and Play Integrity verification logic from the app into `alexandria-verify` behind an optional cargo feature (suggested name `attestation`). Trust inputs (Apple root, app identifier, Play Integrity response keys, debug-build allowance) are supplied by the caller; the library still performs no I/O. The app calls the library for its own issuance-time re-verification and passes trust inputs from the network profile, so there is one implementation.
- Define attestation evidence as a typed entry in the credential's standard `evidence` property: platform, the evidence material, binding kind (`credential` or `session`), and, for a companion phone, the companion binding. The credential envelope gains `evidence`.
- Define the binding digest: a domain tag over the session's terminal commitment root, the credential subject, the claim and the integrity assertion, using the canonical JSON the proof already uses. Expose a function that computes it from a credential, so issuers (ticket 06) and verifiers agree.
- Verification: given trust inputs, check the evidence and report an attestation outcome in the verification result: `not_supplied`, `verified_credential_bound`, `verified_session_bound`, `unverifiable` (no trust inputs for that platform), or `invalid` with a stable reason code. Credential-bound evidence must be over the binding digest of this credential; session-bound evidence must be over the assertion's commitment root.
- `invalid` rejects. The other outcomes never reject.
- The result gains a verified assurance level: `device_attested` only for `verified_credential_bound`, else `local`.
- The provenance classification gains the attestation outcome alongside its existing states.
- Without the feature, evidence entries parse and the outcome is `unverifiable`.

## Acceptance criteria

- [x] The app's existing App Attest and Play Integrity tests run against the library implementation; the app keeps no second copy.
- [x] Vectors for each outcome, built with a test root and test Play keys supplied as trust inputs: not supplied, credential-bound, session-bound, unverifiable, invalid (bad chain, wrong nonce, wrong app id), and evidence lifted from another credential (invalid).
- [x] The no-I/O dependency test passes with the feature on.
- [x] Credentials without `evidence` verify exactly as before.
- [x] `cargo clippy -D warnings` passes for the library with and without the feature.

## Pointers

- Current verifiers: `src-tauri/src/sentinel/attestation/` (`app_attest.rs`, `play_integrity.rs`, `mod.rs` incl. `effective_assurance`, `TrustConfig`).
- Issuance-time re-verification: `src-tauri/src/credentials/issue.rs` (`build_integrity_assertion`).
- Envelope and result: `crates/alexandria-verify/src/vc/mod.rs`; verifier: `vc/verify.rs`; classification: `crates/alexandria-verify/src/trust.rs`.
- Commitment chain: [sentinel.md](../sentinel.md) *Commitment chain* and *Device attestation*.

## Notes

2026-11-16: built.

- `alexandria-verify` gains `attestation` (App Attest and Play Integrity checks, moved from the app with their tests) and `test-support` (deterministic synthetic App Attest chains and Play Integrity tokens, used by the app's tests and the vectors). Without `attestation`, a parseable entry is `unverifiable`; a duplicated or unparsable one is still `invalid`.
- Public surface in `alexandria_verify::attestation`: `AttestationEvidence` (entry type `DeviceAttestationEvidence`: `platform`, `binding`, `sessionStartedAt`, `appId`, `material`, optional `companion`), `binding_digest` / `binding_digest_of`, `AttestationOutcome`, `InvalidReason`, `VerifiedAssuranceLevel`, `AttestationTrust`, `verify_evidence`, `verify_record`. The envelope gains `evidence` (omitted when empty); the result gains `attestation` and `verifiedAssuranceLevel`. `verify_credential` keeps its signature and passes no trust inputs; `verify_credential_with_attestation` takes them.
- Binding digest: SHA-256 of JCS `{tag, commitmentRoot, subject, claim, integrity}`, tag `alexandria-attestation-binding-v1`. It does not cover issuer, type, id or validity. Credential-bound proof: on iOS an assertion from the session's attested key whose client data is the 32 digest bytes (counter above the session assertion's); on Android a token whose nonce is the digest.
- Certificate validity is judged at the assertion's `generatedAt` (when the issuing device verified the same evidence), so an outcome never changes with verification time.
- Deviation: session-bound evidence must attest the assertion's session (nonce from session id and `sessionStartedAt`); it covers the commitment root only where the platform can (an iOS session assertion, checked against the assertion's root when present). Android session evidence cannot cover the root.
- Invalid reasons: `malformed_evidence`, `duplicate_evidence`, `missing_integrity_assertion`, `untrusted_chain`, `wrong_nonce`, `wrong_app_id`, `key_mismatch`, `counter`, `environment_not_accepted`, `invalid_signature`, `device_integrity`, `binding_mismatch`, `companion_mismatch`.
- Classification: verified states carry `attestation`; invalid evidence adds `invalid_attestation`; a result whose outcome contradicts the credential's evidence is inconsistent. `TRUST_CALCULATION_VERSION` is 2.
- Vectors live in `tests/vectors/attestation/` with their own runner, outside the main suite and the independent verifier. The main vectors were not regenerated.
- The app passes trust inputs from `TrustConfig::inputs` (Apple root, app ids, profile Play keys) in `verify_credential_db` and offline bundle verification. CI also checks the library alone, feature off.
