# Docs and glossary

Triage: done
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/09-sponsor-learner-ui.md, docs/sentinel-trust-profiles/10-interview-signal-feed.md, docs/sentinel-trust-profiles/11-camera-performance.md, docs/sentinel-trust-profiles/12-desktop-device-axis.md, docs/sentinel-trust-profiles/13-llm-probe-policy.md, docs/sentinel-trust-profiles/04-adaptive-draw.md, docs/sentinel-trust-profiles/02-measurement-corpus.md

Approved by the owner 2026-11-12 (see spec Decisions).

- Create `GLOSSARY.md` at the repo root with the spec's vocabulary; add it to the workspace `GLOSSARY-MAP.md` if that file exists.
- Update `docs/sentinel.md` (taxonomy, flag rules, platform table, assurance, privacy, coverage statements), protocol spec integrity sections, and the runbook. Fit the docs overhaul structure and `scripts/check-docs.sh`.

## Notes

- Review fix for the regression this ticket noted: since scoring moved to the backend, `behavior_shift` and `bot_suspected` fired whenever the keystroke or mouse model ran, even with advisory AI scoring off. `derive_window` again lets those two models flag only when `models.advisory_enabled` is set; the rule-based checks behind the same flags are unchanged. `docs/sentinel.md` §Runtime Toggle says so.

- 2026-11-13: closed. Merged with the rest of the spec; review findings fixed alongside.
