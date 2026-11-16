# 01 Provenance scoring

Triage: ready-for-agent
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

- [ ] A self-issued credential of every credential type scores exactly as a `SelfAssertion` with the same claim.
- [ ] An issuer-signed credential's weight is unchanged.
- [ ] Cached derived states computed under the old version are recomputed on read.
- [ ] Existing aggregation, scoring-input and reputation tests pass; changed expectations are explained in the commit.

## Pointers

- Aggregation weights and type table: `src-tauri/src/aggregation/` (`config.rs`, `weights.rs`, `mod.rs`, `store.rs`).
- Self-issued detection already exists on scoring inputs: `src-tauri/src/db/scoring_inputs.rs`.
- Reputation: `src-tauri/src/evidence/reputation.rs`.
- Calculation version `1.2` and spec `v4-verified-vc` are described in [skills-and-reputation.md](../skills-and-reputation.md) §6.1.
