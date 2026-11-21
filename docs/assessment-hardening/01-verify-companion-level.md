# 01 Companion-attested level in the verifier

Triage: done
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

- [x] Attestation vectors: companion-bound evidence verifies as `companion_attested`; a policy requiring `device_attested` refuses it with the existing refusal code; a policy requiring `companion_attested` accepts both it and `device_attested`.
- [x] Credentials without evidence and phone-native evidence verify exactly as before.
- [x] Vectors regenerate byte-identically on a second run; the diff is only what this ticket changed.
- [x] Library clippy passes with and without the `attestation` feature; the no-I/O test passes.

## Pointers

- `crates/alexandria-verify/src/attestation/mod.rs` (`VerifiedAssuranceLevel`, `CompanionBinding`, `companion_bound_nonce`), `src/policy.rs`, `src/trust.rs`, `src/vc/mod.rs`.
- Prior art: [credential-trust/05](../credential-trust/05-attestation-verification.md), [credential-trust/08](../credential-trust/08-shared-issuance-policy.md).
- The app's own callers of the level may need to compile against the new variant; keep app behaviour changes for ticket 03.

## Notes

2026-11-20: built.

- `VerifiedAssuranceLevel` gains `CompanionAttested` (`companion_attested`), ordered `Local` < `CompanionAttested` < `DeviceAttested` (`PartialOrd`/`Ord`), plus `ALL` and `parse` (`None` for the reserved `anchored`, `high_assurance` and unknown tokens).
- `AttestationOutcome::VerifiedCredentialBound` gains `companion: bool`, set when the verified entry carries a `CompanionBinding`, omitted on the wire while false. `verified_assurance_level()` maps it to `companion_attested`; phone-native credential-bound evidence stays `device_attested`. Deviation: a field on the existing outcome rather than a new outcome, so phone-native results and every existing vector serialize byte-identically and `VerifiedCredentialBound { .. }` patterns keep matching. Session-bound outcomes carry no companion flag (their level is `local` either way).
- The policy rule judges a required level on the ladder (`level >= required`): `device_attested` refuses `companion_attested` with `assurance_below_required`; `companion_attested` accepts either higher level. `AssessedCredentialEvidence` carries the new level; an entry stating `device_attested` beside a companion outcome is judged `local`.
- Classification: trust states carry the outcome with `companion`; a result that reports `device_attested` for a companion outcome is inconsistent. `TRUST_CALCULATION_VERSION` is 4. The stated `assuranceLevel` stays a string; its doc names the three values.
- Vectors: attestation `10-companion-credential-bound` now states `companion_attested` and expects `companion: true` and `companion_attested`. New policy vectors 27 to 31 (companion refused for `device_attested`, `companion_attested` met by companion and by device evidence, stated without evidence refused, role on a companion assessment refused). Every other vector is unchanged; regeneration is byte-identical.
- App: call sites compile against the new field (test expectations, `..` in the provenance match) and `src/types/index.ts` mirrors the level and the field. App behaviour is unchanged: a companion credential still reads as `Provenance::DeviceAttested` and takes `attested_weight` until ticket 03.
- Not changed: a floor's device attestation signal is still credited by any verified evidence, companion included. README, CHANGELOG and `tests/vectors/README.md` wording is left for ticket 02's release note and ticket 12.
