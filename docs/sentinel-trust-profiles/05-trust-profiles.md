# Trust profile selection, lock and degradation

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/01-scoring-core.md

Spec: Implementation Decisions → Vocabulary (trust profile), Profile selection and lifecycle.

- Profiles `core`, `attested`, `camera`, `full`, each a fixed signal set with a published per-profile compute budget.
- Backend picks the profile at session start: attestation → capability report → device benchmark → policy floor + per-capability consent → highest permitted. Written as the session's first committed snapshot.
- Profile locked; later signal loss is a degradation flag, never a downgrade. Capability absence never counts as clean.
- Device benchmark: fixed short workload (one detection pass + one liveness pass on a synthetic frame) against the budget.
- Policy floor evaluated at attempt start and interview-check acceptance, result returned to the client before the first item; issuance re-evaluates and must agree or fail closed.
- Schema: profile, selection inputs, consent per capability on the session. Regenerate `docs/database-schema.md`.

## Notes

2026-11-12, deviations from the bullets above:

- The selection is the first link of the session's commitment chain (`trust_profile|{session}|{record}` folded before any snapshot) and lives on `integrity_sessions.profile_selection_json`, not in an `integrity_snapshots` row: a snapshot row would count in camera coverage and the session average.
- A floor is met by signal-set inclusion, not by order: `camera` does not meet an `attested` floor. Selection picks the highest permitted profile that meets the floor, else the highest permitted.
- The floor is the new `IssuancePolicy.min_trust_profile`. `min_camera_coverage` is unchanged; folding it into the camera profiles is ticket 08.
- An unmet floor at attempt start refuses the attempt (no attempt is consumed) as well as returning the judgement. Issuance refuses unless its own judgement (attestation credited only if it verifies then) equals the recorded one.
- Interview invitations carry no floor (signed wire struct shared with Cloud), so interview-check acceptance records an empty floor.
- Budgets are named constants in `sentinel::trust_profile` (camera 250/60 ms, full 150/40 ms, detect/liveness), matching ticket 11; the benchmark here times one detection and one liveness pass. Use ticket 11's benchmark once both land.
- Degradation covers the camera only (`camera_degraded`, warning). Under profiles without the camera, camera features are dropped as not run. Attestation loss surfaces at issuance, not per window.
- Sessions that never selected a profile (older rows, tests) are scored as `core`, keep windows as sent, and never meet a floor.
- Microphone and companion-phone consent are recorded; no profile needs them yet.
