# 17 Camera coverage in the integrity assertion

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 08

Spec sections: *One issuance policy rule*, stories 34, 39.

## What to build

Ticket 08 made the camera floor strict: a verifier cannot see coverage, so a camera listed as degraded never carries a floor, while the issuing device used to tolerate up to 10% of windows without camera checks. Owner decision (2026-11-17): keep the 90% tolerance, judged the same way by every verifier.

- Add `cameraCoverage` to the integrity assertion: the fraction of committed windows that carried camera checks, in `[0,1]`, set only when the profile includes the camera. A session with no windows states `0`.
- The shared rule credits the camera towards a floor when the assertion states `cameraCoverage` at or above `MIN_CAMERA_COVERAGE` (0.9). An assertion without the field (issued before it existed) keeps the strict reading: the camera counts only when not listed as degraded.
- `signalsDegraded` still lists the camera whenever coverage is below 1, so a reader sees the drop-out.
- The app's issuance writes the field from the same windows its session surface already counts, and its own floor check goes through the shared rule.
- The credential detail view shows the coverage beside the camera signal.

## Acceptance criteria

- [ ] Vectors: camera floor with coverage 0.95 and the camera degraded (met); coverage 0.85 (refused, `trust_profile_below_floor`); no coverage field and degraded (refused).
- [ ] Issuance tests over an in-memory database: a camera session with one degraded window in twelve issues under a camera floor and states its coverage.
- [ ] fmt, clippy, vue-tsc and the crate's vectors pass; vectors regenerate byte-identically.
