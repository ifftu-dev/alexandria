# Adaptive, exposure-weighted item draw

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md

Spec: Signals and measurement → adaptive draw bullet; ai-chat-spec §13A Item-bank protection.

- Exclude items previously served to the profile; exposure-weighted selection; require the bank's transfer items; next item chosen from the difficulty band suggested by prior correctness.
- Uses the existing per-attempt drawn question ids; changes selection, not schema, unless exposure counts need a column.
- Resumed attempts keep their original draw.

Done when: assessment command tests show exclusion, transfer-item inclusion, difficulty adaptation, and deterministic draws under a seeded RNG.
