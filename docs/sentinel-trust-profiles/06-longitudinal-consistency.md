# Longitudinal consistency

Triage: done
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/01-scoring-core.md

Spec: Signals and measurement → longitudinal consistency bullet.

- Per-skill score from recent independent practice outcomes, lesson engagement and prior attempts, compared with the attempt outcome. Computed locally from the profile database.
- Large divergence feeds the ticket-01 fusion as a feature; replaces the planned `practice_assessment_divergence` info flag.
- Exposes a satisfied/not-satisfied result for the `consistent` verification rung (ticket 08 consumes).

## Notes

2026-11-12, deviations and choices made while implementing:

- Module: `alexandria_studio::integrity::assess_consistency` (pure rule) and `sentinel::consistency` (reads the profile database). History is practice first answers (right without a hint or reveal), scored lessons on the skill, and graded items of the learner's earlier attempts, all from before the attempt started and within 90 days, weighted by recency (14-day half-life). Unscored completed lessons add a little expected success but never count as history on their own.
- A practice question belongs to a skill when its label equals the skill's name or its thread is bound to a lesson tagged with the skill. Practice questions carry no skill id.
- Only an outcome well above what history predicts diverges. Too little recent history is `insufficient_history`: the rung is not satisfied, and nothing counts against the learner.
- The outcome is known only at grading, and grading needs a finished (possibly attested) session. So `longitudinal_divergence` is not written into the commitment chain: both grading paths re-run the fusion with it before issuance, and the status can only move up. This is the one place a finished session is decided again.
- Divergence is a fusion signal (own `longitudinal` channel, info severity, advisory until certified), not a failed verification.
- No migration: the result is derived again from the database when needed (ticket 08).

- 2026-11-13: closed. Merged to feat/sentinel-trust-profiles; review findings fixed on the same branch.
