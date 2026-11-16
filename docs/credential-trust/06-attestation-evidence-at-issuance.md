# 06 Attestation evidence at issuance

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 05

Spec sections: *Attestation evidence in the credential*, stories 18–20, 22, 24–25.

## What to build

A credential issued against an attested integrity session carries attestation evidence, bound to the credential when the platform allows.

- At issuance, after the integrity assertion is built and before signing, compute the binding digest (ticket 05) and obtain platform evidence over it: a fresh App Attest assertion from the session's attested key on iOS; a fresh Play Integrity token with the digest as nonce on Android; for a companion-phone session, a request to the phone over the companion link for the same, before the link closes.
- Put the evidence in the credential's `evidence` as credential-bound. If evidence over the digest cannot be obtained, carry the session's existing evidence as session-bound. Never upgrade later.
- Obtaining evidence goes through a port so tests use a fake attester; the native calls stay behind it.
- Keep per-session attested keys (no device linkage across credentials).
- Raise credential size limits deliberately if evidence does not fit, and update the limit vectors; never truncate.
- Bundles carry the evidence as part of the credential.
- The learner can see, from the issuance result, when evidence could only be session-bound and why.

## Acceptance criteria

- [ ] With a fake attester, an attested session issues a credential whose evidence verifies `verified_credential_bound` through the library.
- [ ] With the fake attester failing, the credential carries session-bound evidence that verifies `verified_session_bound`.
- [ ] An unattested session issues a credential with no attestation evidence.
- [ ] Companion sessions request evidence from the phone; a test drives the request over the companion message types with a fake phone.
- [ ] Grading still commits atomically: a failure to obtain evidence never fails issuance.

## Pointers

- Native capture and assertion: `src-tauri/src/sentinel/attestation/native.rs` (`assert_commitment`).
- Companion link: `src-tauri/src/sentinel/companion/` (`transport.rs`, `session.rs`).
- Issuance: `src-tauri/src/credentials/issue.rs`; assessment grading: `src-tauri/src/commands/assessment.rs`.
- Size limits: `crates/alexandria-verify/tests/credential_limits.rs`, `limit_vectors.rs`.
