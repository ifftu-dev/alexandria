# 08 Shared issuance policy

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 05

Spec sections: *One issuance policy rule*, stories 34, 39, 46.

## What to build

One rule decides whether an integrity result satisfies an issuance policy, usable by the app and the hosted service.

- Move the issuance policy type and the verifier-visible part of its evaluation into `alexandria-verify`: session status, score and flag bounds, trust profile and policy floor (from the assertion's signals run, degraded and not run), verification rung, probe outcome, and required assurance level judged against the verified assurance level from ticket 05.
- The function takes the policy, the integrity assertion and the attestation outcome, and returns the first unmet requirement as a typed refusal with a stable code.
- The app keeps the checks that need the session: floor-at-start agreement and re-derivation of the assertion. It calls the shared rule for the rest.
- A policy requiring `device_attested` is satisfied only by a verified assurance level.
- The policy's serialised shape stays compatible with roles already stored.

## Acceptance criteria

- [x] The app's existing issuance-gate tests pass through the shared rule.
- [x] Vectors for each refusal code, including `device_attested` required with session-bound evidence (refused) and with credential-bound evidence (accepted).
- [x] Stored role policies deserialize unchanged.

## Pointers

- Policy today: `src-tauri/src/credentials/issue.rs` (`IssuancePolicy`, `evaluate`, `SessionFacts`).
- Floor evaluation: `crates/alexandria-studio/src/integrity/` (`evaluate_floor`), re-exported as `sentinel::scoring`.
- Refusal enum: `src-tauri/src/credentials/error.rs` (`PolicyViolation`).

## Notes

2026-11-16: built.

- `alexandria_verify::policy`: `IssuancePolicy`, `ProbePolicy`, `PolicyRefusal` (serde-tagged by `code`, with `code()`), and `evaluate(policy, integrity, attestation)`. Order: status, score, flags, assurance, floor, rung, probes; the first unmet requirement is returned.
- `alexandria_verify::integrity`: `TrustProfile`, `ProfileSignal`, `VerificationRung`, `PolicyFloor`, `evaluate_floor`, `MIN_CAMERA_COVERAGE`, moved from `alexandria-studio`, which re-exports them (and the app's `sentinel::scoring` still does). The app's `ProbePolicy` is a re-export of the library's.
- Required assurance is a minimum against the verified level: `local` is met by every credential, `device_attested` only by verified credential-bound evidence, reserved and unknown levels never. Before, the assertion's stated level had to equal the required one.
- Floor from the assertion: the profile is `trustProfile`; a signal the floor adds counts when it is in `signalsRun`, not in `signalsNotRun` and not in `signalsDegraded`; `device_attestation` also needs evidence that verifies, credential- or session-bound.
- Decision: a degraded signal never carries a floor, because a verifier cannot see camera coverage. A camera session with one window in ten without camera checks used to meet a camera floor (coverage at least `MIN_CAMERA_COVERAGE`); it is now refused, in the app and the service alike. The test that pinned the old behaviour was changed. If partial coverage should still count, the assertion needs a verifier-visible coverage field.
- The app judges the policy on the unsigned envelope just before signing, with the outcome of `verify_evidence` over that envelope and the profile's trust inputs, then checks that the floor judged at attempt start agrees with the session now (`CredentialError::PolicyFloorDisagrees`, moved out of the refusal enum since only the device can judge it). Until ticket 06 embeds evidence the outcome is `not_supplied`, so a `device_attested` requirement or an `attested` floor is refused in the app.
- `PolicyViolation` is renamed `PolicyRefusal` (the library type). The stored policy JSON shape is unchanged, with a test over stored role rows.
- Vectors: `tests/vectors/issuance-policy/` with their own runner (`tests/policy_vectors.rs`), outside the main suite and the independent verifier. One vector per refusal code plus the `device_attested` session-bound and credential-bound pair; with `attestation` on, a test also judges the attestation vectors' iOS credential-bound and session-bound credentials end to end.
