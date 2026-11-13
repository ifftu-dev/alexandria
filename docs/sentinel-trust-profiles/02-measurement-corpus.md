# Measurement corpus, offline evaluator, certification gate

Triage: done
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/01-scoring-core.md

Spec: Implementation Decisions → Measurement corpus and evaluation; Testing seam 4.

- Corpus entry format (snapshot sequence + raw features + label + profile + platform + device class) and a manifest with digest.
- Offline evaluator (example binary, no Tauri) replaying entries through the ticket-01 module; prints per-signal precision, recall, likelihood ratio per profile, and fusion confusion matrix.
- Certification manifest format; coverage statement embeds its digest (ticket 08 consumes).
- Committed fixture corpus is synthetic and labelled `synthetic`: contract tests only, never certification.
- CI step runs the evaluator on the committed manifest and fails if an admitted signal falls below its certified rate.
- Red-team runbook: scripted attack sessions maintainers run, and the explicit opt-in clean-session capture (separate consent, never from ordinary use). Collecting real sessions is ticket 14.

Prior art: `crates/alexandria-decisions` offline evaluate example; committee holdout commands for real-world holdout sets.

## Notes

2026-11-12, deviations and choices made while implementing:

- Module: `alexandria_studio::integrity::measurement` (corpus, report, certification, cli); the binary is `examples/integrity_evaluate.rs` with `report`, `check`, `draft`, `entry` and `seal` commands. Runbook: `docs/sentinel-corpus-runbook.md`.
- Precision, recall and false-positive rate are session-level (a session counts once if the signal fired in any window). The likelihood ratio is per window, with a 0.5 continuity correction, because the fusion adds a ratio each window a signal fires.
- "Falls below its certified rate" means measured precision or recall below the certified value; a certified signal that never fires also fails. The check refuses a certification whose corpus digest differs, and a non-synthetic certification over a corpus with synthetic entries.
- A certification's digest is `blake3:` over its exact file bytes. `Certification::session_manifest()` refuses synthetic certifications; `ratio_manifest()` (replay only) does not.
- `sentinel::scoring::active_manifest()` stays uncertified: there is no real certification until ticket 14.
- Clean-session capture is a process environment variable (`ALEXANDRIA_SENTINEL_CAPTURE_DIR`) with consent signed out of band, not an in-app consent flow. Desktop and iOS (devicectl launch environment) only; Android has no capture path yet.

- 2026-11-13: closed. Merged with the rest of the spec; review findings fixed alongside.
