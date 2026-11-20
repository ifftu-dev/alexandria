# 01 Companion-attested level in the verifier

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: none

Spec sections: *Vocabulary* (companion-attested), *Companion-attested*, *Verifier library*, stories 3, 4, 6, 42.

## What to build

A verifier stops reading a phone-backed desktop session as a verified device.

- The verified assurance level gains `companion_attested`, ordered `local` < `companion_attested` < `device_attested`. The stated assurance level accepts the same three values.
- Verified credential-bound evidence that carries a companion binding yields `companion_attested`, never `device_attested`. Evidence without a companion binding is unchanged.
- The shared issuance policy rule: a required `device_attested` is not met by `companion_attested`; a required `companion_attested` is met by either higher level.
- Provenance classification and anything that names the level (result JSON, display helpers) carry the new value.
- Advance the trust calculation version.

## Acceptance criteria

- [ ] Attestation vectors: companion-bound evidence verifies as `companion_attested`; a policy requiring `device_attested` refuses it with the existing refusal code; a policy requiring `companion_attested` accepts both it and `device_attested`.
- [ ] Credentials without evidence and phone-native evidence verify exactly as before.
- [ ] Vectors regenerate byte-identically on a second run; the diff is only what this ticket changed.
- [ ] Library clippy passes with and without the `attestation` feature; the no-I/O test passes.

## Pointers

- `crates/alexandria-verify/src/attestation/mod.rs` (`VerifiedAssuranceLevel`, `CompanionBinding`, `companion_bound_nonce`), `src/policy.rs`, `src/trust.rs`, `src/vc/mod.rs`.
- Prior art: [credential-trust/05](../credential-trust/05-attestation-verification.md), [credential-trust/08](../credential-trust/08-shared-issuance-policy.md).
- The app's own callers of the level may need to compile against the new variant; keep app behaviour changes for ticket 03.
