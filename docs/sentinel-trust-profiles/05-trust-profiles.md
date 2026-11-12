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
