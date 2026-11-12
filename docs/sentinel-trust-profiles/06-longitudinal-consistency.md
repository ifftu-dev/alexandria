# Longitudinal consistency

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/01-scoring-core.md

Spec: Signals and measurement → longitudinal consistency bullet.

- Per-skill score from recent independent practice outcomes, lesson engagement and prior attempts, compared with the attempt outcome. Computed locally from the profile database.
- Large divergence feeds the ticket-01 fusion as a feature; replaces the planned `practice_assessment_divergence` info flag.
- Exposes a satisfied/not-satisfied result for the `consistent` verification rung (ticket 08 consumes).
