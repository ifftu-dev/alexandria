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
