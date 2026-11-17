# 17 Camera coverage in the integrity assertion

Triage: done
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

- [x] Vectors: camera floor with coverage 0.95 and the camera degraded (met); coverage 0.85 (refused, `trust_profile_below_floor`); no coverage field and degraded (refused).
- [x] Issuance tests over an in-memory database: a camera session with one degraded window in twelve issues under a camera floor and states its coverage.
- [x] fmt, clippy, vue-tsc and the crate's vectors pass; vectors regenerate byte-identically.

## Notes

2026-11-17: built.

- `IntegrityAssertion::camera_coverage` (`cameraCoverage`, omitted when absent, so credentials without it keep their bytes and hash). Set from the session surface whenever the profile includes the camera, as covered windows over committed windows; `0` with no windows. A profile without the camera states nothing.
- `alexandria_verify::integrity::camera_coverage_counts(coverage)`: at least `MIN_CAMERA_COVERAGE` and at most 1. The policy rule credits the camera when the assertion states a coverage that counts, whatever `signalsDegraded` says; without the field a degraded camera never counts. A degraded device attestation still never counts. The session surface uses the same function for the camera it leaves uncredited, so the floor judged again at issuance and the shared rule agree.
- Deviation: a stated coverage outside `[0,1]` is malformed and never carries a floor (vector 26).
- Vectors 23 (0.95, degraded, met), 24 (0.85, refused) and 25 (exactly 0.9, met), plus 26 (out of range). The third criterion, no field and degraded, is the existing vector 12; only its description changed, to say it states no coverage. The vectors regenerate byte-identically; no other vector file changed.
- Issuance test: one degraded window in twelve issues under a camera floor and states `11/12`; one in ten states `0.9` and issues; two in four and an empty session are refused. A `core` session's credential has no `cameraCoverage` on the wire.
- Detail view: a camera coverage row after the degraded signals, as a whole percent rounded down (so 89.9% never reads 90%), in the warning colour below 100%. Keys `credentials.detail.cameraCoverage*`, English-only and listed pending in `scripts/i18n/check-parity.mjs`.
- The coverage statement texts are unchanged: the camera text says windows without camera checks are recorded as degraded, which still holds.
