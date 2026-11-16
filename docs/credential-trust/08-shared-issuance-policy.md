# 08 Shared issuance policy

Triage: ready-for-agent
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

- [ ] The app's existing issuance-gate tests pass through the shared rule.
- [ ] Vectors for each refusal code, including `device_attested` required with session-bound evidence (refused) and with credential-bound evidence (accepted).
- [ ] Stored role policies deserialize unchanged.

## Pointers

- Policy today: `src-tauri/src/credentials/issue.rs` (`IssuancePolicy`, `evaluate`, `SessionFacts`).
- Floor evaluation: `crates/alexandria-studio/src/integrity/` (`evaluate_floor`), re-exported as `sentinel::scoring`.
- Refusal enum: `src-tauri/src/credentials/error.rs` (`PolicyViolation`).
