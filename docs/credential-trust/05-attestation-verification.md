# 05 Attestation verification

Triage: ready-for-agent
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

- [ ] The app's existing App Attest and Play Integrity tests run against the library implementation; the app keeps no second copy.
- [ ] Vectors for each outcome, built with a test root and test Play keys supplied as trust inputs: not supplied, credential-bound, session-bound, unverifiable, invalid (bad chain, wrong nonce, wrong app id), and evidence lifted from another credential (invalid).
- [ ] The no-I/O dependency test passes with the feature on.
- [ ] Credentials without `evidence` verify exactly as before.
- [ ] `cargo clippy -D warnings` passes for the library with and without the feature.

## Pointers

- Current verifiers: `src-tauri/src/sentinel/attestation/` (`app_attest.rs`, `play_integrity.rs`, `mod.rs` incl. `effective_assurance`, `TrustConfig`).
- Issuance-time re-verification: `src-tauri/src/credentials/issue.rs` (`build_integrity_assertion`).
- Envelope and result: `crates/alexandria-verify/src/vc/mod.rs`; verifier: `vc/verify.rs`; classification: `crates/alexandria-verify/src/trust.rs`.
- Commitment chain: [sentinel.md](../sentinel.md) *Commitment chain* and *Device attestation*.
