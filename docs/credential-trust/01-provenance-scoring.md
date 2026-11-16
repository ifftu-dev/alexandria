# 01 Provenance scoring

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: none

Spec sections: *Scoring by verified provenance* (first two bullets and the version bullet), story 44–45.

## What to build

Aggregation weight follows who signed a credential, not the type it declares. A self-issued credential (issuer is its subject) takes the self-claim weight whatever credential type it names. Today a self-issued `AssessmentCredential` takes the assessment type weight (0.90), above an organisation-signed `RoleCredential` (0.60).

- The rule applies everywhere a credential's type weight enters a derived skill state or a reputation row.
- Self-issued credentials still add no independent issuer cluster (unchanged).
- Advance the derived-state calculation version, and the reputation computation specification if reputation reads the type weight, so cached states and rows recompute through the existing fingerprint mechanism.
- Ticket 07 adds the attested weight and the session-status rule later; leave a single obvious place for them.

## Acceptance criteria

- [x] A self-issued credential of every credential type scores exactly as a `SelfAssertion` with the same claim.
- [x] An issuer-signed credential's weight is unchanged.
- [x] Cached derived states computed under the old version are recomputed on read.
- [x] Existing aggregation, scoring-input and reputation tests pass; changed expectations are explained in the commit.

## Pointers

- Aggregation weights and type table: `src-tauri/src/aggregation/` (`config.rs`, `weights.rs`, `mod.rs`, `store.rs`).
- Self-issued detection already exists on scoring inputs: `src-tauri/src/db/scoring_inputs.rs`.
- Reputation: `src-tauri/src/evidence/reputation.rs`.
- Calculation version `1.2` and spec `v4-verified-vc` are described in [skills-and-reputation.md](../skills-and-reputation.md) §6.1.

## Notes

2026-11-16: Built. `aggregation::weights::provenance_weight` is the single place that picks a credential's class weight: self-issued takes the `SelfAssertion` weight, issuer-signed takes its declared type's weight. Ticket 07 adds the attested weight and the session-status rule there. Calculation version advances from `1.2` to `1.3`; cached states under `1.2` are dropped and recomputed through the existing fingerprint mechanism.

- Reputation does not read the type weight (rows sample raw claim scores), so the computation spec stays `v4-verified-vc`. The spec's bullet that both advance applies again in ticket 07 only if reputation starts reading weights.
- "Every credential type" covers the six types a scoring input can carry. `EntitlementCredential` is never a scoring input, because the default verification policy rejects it whoever signs it.
- A self-issued `DerivedCredential` used to weigh 0 and now takes the self-claim weight, like every other self-issued type.
