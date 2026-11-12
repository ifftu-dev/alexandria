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
- Budgets and the benchmark are ticket 11's (`sentinel::camera_budget`: camera 250/60/15 ms, full 150/40/10 ms, detect/liveness/track, median of three passes); the selection record stores the measured result and both budgets.
- Degradation covers the camera only (`camera_degraded`, warning). Under profiles without the camera, camera features are dropped as not run. Attestation loss surfaces at issuance, not per window.
- Sessions that never selected a profile (older rows, tests) are scored as `core`, keep windows as sent, and never meet a floor.
- Microphone and companion-phone consent are recorded; no profile needs them yet.
- Review fix: a backend flag raised before selection (the attempt gate flagging an AI surface) was committed at once, became the chain's first link and blocked both selection and pairing for good. Migration 40 adds `integrity_sessions.pending_backend_flags`: before the chain's first link a flag waits there (counted once, like a committed one), and `snapshots::flush_pending_flags` commits the queue in order right after the selection link, or ahead of the first client window or at session end when no profile is chosen. The chain then proves the order: selection (and any pairing before it) first, every flag after. A flag that would suspend the session takes effect at that point, seconds after it was raised.
