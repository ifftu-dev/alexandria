# Adaptive, exposure-weighted item draw

Triage: done
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md

Spec: Signals and measurement → adaptive draw bullet; ai-chat-spec §13A Item-bank protection.

- Exclude items previously served to the profile; exposure-weighted selection; require the bank's transfer items; next item chosen from the difficulty band suggested by prior correctness.
- Uses the existing per-attempt drawn question ids; changes selection, not schema, unless exposure counts need a column.
- Resumed attempts keep their original draw.

Done when: assessment command tests show exclusion, transfer-item inclusion, difficulty adaptation, and deterministic draws under a seeded RNG.

## Notes

2026-11-12, implementation:

- No schema change. A transfer item is marked `"transfer": true` in `assessment_items.content_public` rather than a new column, so the flag is ratified content covered by the item fingerprint. Exposure is counted from existing rows (`question_ids` plus `attempt_items`), so it needs no column either.
- Transfer items are served on top of `draw_count`, not inside it ("a pass requires the transfer items as well as the standard draw").
- Exclusion is soft: unseen items first, then served items least-exposed first, because the bundled banks are smaller than three draws. Exposure is this profile's only; cross-profile counts from Cloud can feed the same `DrawContext::exposure` map later.
- Out of scope here: grading does not yet require transfer items or a minimum of unseen items to pass, the adaptive (IRT) delivery path is unchanged, and no bundled bank carries transfer items yet (ai-chat-spec §24 leaves which banks get them first open).

- 2026-11-13: closed. Merged to feat/sentinel-trust-profiles; review findings fixed on the same branch.
