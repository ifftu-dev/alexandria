# Integrity assertion, issuance policy and coverage statements

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/03-response-process-signals.md, docs/sentinel-trust-profiles/05-trust-profiles.md, docs/sentinel-trust-profiles/06-longitudinal-consistency.md, docs/sentinel-trust-profiles/07-probe-contract.md

Spec: Implementation Decisions → Vocabulary (rung, coverage statement), Credential and policy surface; Testing seam 2.

- Verification rung computed per session: `none` → `process` → `probed` → `consistent` → `live`.
- Integrity assertion gains trust profile, rung, coverage statement digest, signals run and degraded, probe summary. Re-derived at issuance, never copied from client text. Assurance level unchanged.
- Issuance policy gains minimum profile, minimum rung, probe policy; camera coverage minimum folds into the camera profiles.
- Coverage statements: versioned, content-addressed, bundled with the app (offline), one per profile and rung, embedding the certification manifest digest when present.
- Keep the TS mirror types in step.

## Notes

2026-11-12, deviations and decisions:

- The rung is a strict ladder (`verification_rung`): each rung needs every rung below it, so a coverage statement for a rung states everything beneath it. Insufficient history or a skipped probe leaves the session one rung lower and never counts against the learner.
- `process`: every served item observed and the `response_process` channel not over threshold. Observed is decided per item id, not by a visit count (rule in ticket 03's Notes). The adaptive-draw half has no separate record: every attempt is drawn by the backend, so it holds whenever an attempt is bound to the session.
- `probed` needs every probe result satisfied: a timed-out probe beside a satisfied one keeps the rung at `process`. `live` is a satisfied verification with step `interview` (`STEP_INTERVIEW`) in the same session; ticket 10 records it.
- Both grading paths write the attempt score to the row before issuance, inside the grading transaction, so the assertion reads consistency from the attempt itself.
- Probe policy at issuance: anything but `none` requires the probe step to have run under that policy with no failed probe; skipped is accepted. A satisfied probe is required through `min_verification_rung: probed`.
- `min_camera_coverage` is removed. `camera` and `full` carry `MIN_CAMERA_COVERAGE` (0.9 of committed windows without `camera_degraded`, an unmeasured operating constant); below it the camera does not count towards a floor, so issuance refuses with `TrustProfileBelowFloor`. `evaluate_floor` now takes the uncredited signals instead of an attestation flag. `signals_degraded` lists the camera whenever coverage is below 1 or no window was committed, and `device_attestation` when the attestation no longer verifies. The sponsor form's camera coverage input is gone; ticket 09 adds the profile, rung and probe fields.
- Coverage statements are generated from fixed text in `alexandria_studio::integrity::coverage` (20 statements), digest `blake3:` over the compact JSON bytes, resolved offline by the unscoped IPC `integrity_coverage_statement`. No certification is in force (`active_certification()` returns none until ticket 14), so statements say every signal is advisory. Earlier text versions are not kept: changing the text strands older digests, so exported bundles should carry the statement itself (ticket 09).
- Assertion fields live on `alexandria_verify::vc::IntegrityAssertion` (optional, omitted when empty); the format stays VC Data Model 2.0 with `DataIntegrityProof` `eddsa-jcs-2022`. Cloud follow-up: verifiers re-serialise the typed credential to check the proof, so a verifier on alexandria-verify 0.5.0 drops the new fields and rejects these credentials. Publish a new minor of the crate (the added public fields break struct-literal construction) and move Cloud and the website verifier to it before credentials with these fields leave the device.
