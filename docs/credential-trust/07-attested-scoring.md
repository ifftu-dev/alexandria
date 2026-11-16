# 07 Attested scoring

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 01, 05

Spec sections: *Scoring by verified provenance* (attested weight and session-status bullets), stories 23, 44–45.

## What to build

- A self-issued credential whose attestation evidence verifies credential-bound takes a new attested weight: above the self-claim weight, below every issuer-signed weight. Pick the initial value, record it in the aggregation config with a comment saying why, and note it on this ticket when closing.
- Session status affects scoring only for credentials whose status a reader can rely on: verified credential-bound evidence, or issuer-signed (organisation-issued). For these, `suspended` and `flagged` contribute nothing and `review` counts in full. For every other credential the integrity status is ignored.
- Scoring inputs carry the attestation outcome and the integrity status, computed from the verified credential at scoring time.
- Advance the calculation version and reputation specification again.

## Acceptance criteria

- [x] Self-issued, no evidence: self-claim weight, status ignored (a `suspended` session scores the same as `completed`).
- [x] Self-issued, credential-bound evidence: attested weight; `suspended`/`flagged` contribute nothing; `review` counts.
- [x] Self-issued, session-bound evidence only: self-claim weight, status ignored.
- [x] Issuer-signed with `flagged` session: contributes nothing.
- [x] Caches recompute under the new version.

## Pointers

- Ticket 01's single place for weight rules; `src-tauri/src/db/scoring_inputs.rs`; `src-tauri/src/aggregation/`; `src-tauri/src/evidence/reputation.rs`.
- Attestation outcome: ticket 05's verification result.

## Notes

2026-11-16: built.

- Attested weight `0.30` (`AggregationConfig::attested_weight`). Credential-bound evidence is more than the holder's word (`SelfAssertion` 0.25), but it says nothing about who sat at the device, so it stays below the weakest evidence class another party signs (`AttestationCredential` 0.35). "Below every issuer-signed weight" is read as every issuer-signed evidence class: `DerivedCredential` and `EntitlementCredential` weigh 0 and are not evidence.
- The rule lives in `aggregation::weights`. `provenance_weight` picks the class weight and `session_status_counts` decides whether an input counts. Status is relied on for issuer-signed credentials and for credential-bound attested ones. For these, `suspended` and `flagged` contribute nothing: no weight, no issuer cluster, no active evidence, no source. Every other status counts in full, `review` and unrecognised values included. A pair where nothing counts has no derived state, as when it has no inputs.
- Scoring inputs carry the attestation outcome (from the classification, checked against this build's trust inputs) and the integrity assertion's status. Both are read at scoring time. Tests swap in synthetic Play Integrity trust inputs through a test-only hook in `db::scoring_inputs`, because the profile's trust inputs cannot verify synthetic evidence.
- Calculation version `1.3` → `1.4`; reputation computation spec `v4-verified-vc` → `v5-session-status`. Reputation rows, snapshot evidence and the talent index Bloom level apply the status rule. Reputation reads no weights, so the attested weight does not apply there.
