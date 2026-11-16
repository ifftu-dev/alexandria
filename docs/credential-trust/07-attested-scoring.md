# 07 Attested scoring

Triage: ready-for-agent
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

- [ ] Self-issued, no evidence: self-claim weight, status ignored (a `suspended` session scores the same as `completed`).
- [ ] Self-issued, credential-bound evidence: attested weight; `suspended`/`flagged` contribute nothing; `review` counts.
- [ ] Self-issued, session-bound evidence only: self-claim weight, status ignored.
- [ ] Issuer-signed with `flagged` session: contributes nothing.
- [ ] Caches recompute under the new version.

## Pointers

- Ticket 01's single place for weight rules; `src-tauri/src/db/scoring_inputs.rs`; `src-tauri/src/aggregation/`; `src-tauri/src/evidence/reputation.rs`.
- Attestation outcome: ticket 05's verification result.
