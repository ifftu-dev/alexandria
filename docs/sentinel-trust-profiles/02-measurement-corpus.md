# Measurement corpus, offline evaluator, certification gate

Triage: ready-for-agent
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
