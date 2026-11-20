# 02 Answer custody, required rung and hosted attempt evidence in the verifier

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 01

Spec sections: *Vocabulary*, *Answer custody*, *Required rung*, *Issuance from a hosted attempt* (evidence entry), *Verifier library*, stories 1, 2, 5, 6, 42, 47.

## What to build

- The integrity assertion gains answer custody, `issuer` or `device`. An assertion without it reads as `device`. The assertion format version advances.
- A self-issued credential stating `issuer` custody is invalid, with a stable reason code.
- The shared issuance policy gains a required rung (judged against the strict ladder) and a required answer custody, each with a stable refusal code, in the order the rule documents.
- A typed evidence entry for a hosted attempt: the attempt's identifier, answer custody `issuer`, graded by issuer, the probe outcome, and the attestation outcome and verified assurance level the issuer found. Parsed and exposed; only valid on an issuer-signed credential.
- New coverage statement versions for every trust profile and rung below `live` that state a second device or person was not ruled out. Added to the registry; no existing version replaced.
- Bump the crate to the next minor version and add the breaking-change note to its changelog. Do not publish.

## Acceptance criteria

- [ ] Policy vectors: each new refusal code; required rung met and unmet at each step; custody required `issuer` refused for `device`.
- [ ] Vectors: self-issued `issuer` custody invalid; issuer-signed credential with a hosted attempt evidence entry verifies and exposes it.
- [ ] Every previously published statement digest still resolves; the new ones resolve.
- [ ] Vectors regenerate byte-identically; library clippy with and without features; no-I/O test passes.

## Pointers

- `crates/alexandria-verify/src/vc/mod.rs` (`IntegrityAssertion`), `src/integrity.rs` (`VerificationRung`), `src/policy.rs`, `src/coverage.rs`, `src/assessed.rs` (existing evidence entry prior art), `CHANGELOG.md`.
- Prior art: [credential-trust/02](../credential-trust/02-coverage-statement-registry.md), [credential-trust/16](../credential-trust/16-verifier-release.md).
