# 02 Answer custody, required rung and hosted attempt evidence in the verifier

Triage: done
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

- [x] Policy vectors: each new refusal code; required rung met and unmet at each step; custody required `issuer` refused for `device`.
- [x] Vectors: self-issued `issuer` custody invalid; issuer-signed credential with a hosted attempt evidence entry verifies and exposes it.
- [x] Every previously published statement digest still resolves; the new ones resolve.
- [x] Vectors regenerate byte-identically; library clippy with and without features; no-I/O test passes.

## Pointers

- `crates/alexandria-verify/src/vc/mod.rs` (`IntegrityAssertion`), `src/integrity.rs` (`VerificationRung`), `src/policy.rs`, `src/coverage.rs`, `src/assessed.rs` (existing evidence entry prior art), `CHANGELOG.md`.
- Prior art: [credential-trust/02](../credential-trust/02-coverage-statement-registry.md), [credential-trust/16](../credential-trust/16-verifier-release.md).

## Notes

2026-11-20: built. `alexandria-verify` is 0.8.0 (unreleased, not published).

- Answer custody: `integrity::AnswerCustody` (`device` < `issuer`, re-exported from `custody`). `IntegrityAssertion::answer_custody: Option<AnswerCustody>` is `answerCustody` on the wire, omitted when `None`; `answer_custody()` reads `None` as `device`. The app still writes `None` until ticket 05 sets `device`.
- Policy: `IssuancePolicy::required_answer_custody: Option<AnswerCustody>` (omitted from the stored shape while unset) with `PolicyRefusal::CustodyBelowRequired { custody, required }`, code `custody_below_required`. Refusal order is now custody, status, score, flags, assurance, floor, rung, probes: custody first, so the share path for an `issuer`-custody role is refused with the custody code whatever else is wrong.
- Required rung: not a new field. The existing `min_verification_rung` with `rung_below_minimum` is the spec's required rung; adding a second field for the same bound would split it. "Judged against the strict ladder" is new: a stated `probed`, `consistent` or `live` counts only beside a probe step with outcome `satisfied`, `satisfied > 0`, `failed == 0` and `skipped == 0`, whatever its policies (so `["reseed"]` counts, per ticket 04); otherwise the rule credits `process`. `RungBelowMinimum.rung` names the credited rung (an unknown stated rung is still named as stated). A `live` credential needs the probe step under it.
- Hosted attempt entry: `custody::HostedAttemptEvidence` (`type` `HostedAttemptEvidence`, `attemptId`, `answerCustody`, `gradedBy` (`custody::Grader`, `issuer` or `device`), optional `probe` (`ProbeSummary`), `attestation`, `verifiedAssuranceLevel`); `new(attempt_id, probe, attestation)`, `find`, `into_entry`, `assurance_level`, `states_a_hosted_attempt`. `policy::evaluate_hosted(policy, integrity, hosted)` takes the verified level, the floor's attestation and the probe step (for both the probe policy and the rung's evidence) from the entry; custody, status, score, flags, profile and rung come from the assertion the issuer writes.
- Validity: `custody::custody_defects(credential)` and `VerificationResult::custody_defects` (`custodyDefects`, omitted while empty; any one rejects). Codes, in report order: `issuer_custody_self_issued`, `hosted_attempt_self_issued`, `hosted_attempt_invalid` (duplicate, unparsable, or not `issuer`/`issuer`), `hosted_attempt_custody_mismatch` (entry beside an assertion not stating `issuer`, or none). Self-issued means the issuer DID equals the subject DID. `TrustInvalidReason` gains the same four names; `classify_credential` recomputes the defects and calls a result that disagrees inconsistent. `TRUST_CALCULATION_VERSION` is 5.
- Open question from 01, decided: companion evidence no longer credits a floor's device attestation signal. `AttestationOutcome::VerifiedSessionBound` gains `companion` (omitted while false) because a paired desktop whose phone link closed issues session-bound companion evidence, which otherwise still carried an `attested` floor. Nothing in the spec relies on the old reading. Consequence for tickets 03/05: a desktop session locked to `attested` through a companion no longer meets an `attested` floor in the library; the app's own floor judgment should agree.
- Coverage statements: version 3 (`CURRENT_VERSION`), pinned in `tests/coverage_statements.rs`. Below `live` it adds "Whether a second device or person helped: not ruled out. Only a live segment, where a person asks unscripted questions, rules it out." Deviation: version 3 also rewords the `probed` lines to say the follow-up questions were ones the learner had not already answered, with a time limit fixed by the item (ticket 04's rules), since a new version was being published anyway. `live` statements differ from version 2 only in the `probed` line and `version`.
- Deviation: the assertion has no format version field to advance; the crate's minor version carries the format change, as with `cameraCoverage` in 0.6/0.7. No field was added.
- Vectors: credential 24–29, attestation 13, policy 32–51 (and 19's description now lists custody in the order). Every other vector file is unchanged; regeneration is byte-identical. The independent verifier implements the custody rule and passes 29 credential vectors.
- App: compile-only changes. `answer_custody: None` at assertion literals, `companion: false` in test expectations of session-bound outcomes, `..` in the CLI's session-bound match, `src/types/index.ts` mirrors the new fields, levels and codes, and `CredentialDetail.vue` labels the four new trust reasons (English; other locales pending).
- For ticket 09: the vendored copy needs `custody.rs`, and Cloud's `VerifiedSessionBound` matches need `companion`. For ticket 12: the protocol specification (assertion format, §14.9.5 policy table and refusal order), `docs/sentinel.md` (policy codes, companion floor) and the glossary (answer custody, required rung).
- Pre-existing, not changed: `classify_credential` reads a credential rejected only for `coverage_statement_mismatch` as `inconsistent_verification_result`, since no trust reason names it.
